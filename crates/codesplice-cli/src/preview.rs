use std::collections::{BTreeMap, HashMap, VecDeque};

use codesplice_core::{
    EditPlan, OperationEffect, OperationKind, OutputChange, OutputSegment, PlannedOutput,
    Sha256Digest, WorkspaceSnapshot,
};
use codesplice_fs::FsError;
use codesplice_protocol::{
    DiffResponse, OutputResponse, PreviewResponse, ResolvedOperationResponse, WarningCode,
    WarningDto, escape_terminal_text,
};
use serde_json::{Value, json};

const DETAILED_DIFF_INPUT_BYTES: u64 = 8 * 1024 * 1024;
const DIFF_WORK_UNITS: u64 = 10_000_000;
const DIFF_OUTPUT_BYTES: usize = 4 * 1024 * 1024;
const SAMPLE_BYTES: usize = 48;

pub(crate) struct PreviewArtifacts {
    pub(crate) response: PreviewResponse,
    pub(crate) human: String,
}

pub(crate) fn build_preview(
    snapshot: &WorkspaceSnapshot,
    plan: &EditPlan,
    workspace_identity_hash: Sha256Digest,
    no_diff: bool,
    mut warnings: Vec<WarningDto>,
) -> Result<PreviewArtifacts, FsError> {
    let files = snapshot
        .files
        .iter()
        .map(|file| (file.id.0, file.bytes.as_ref()))
        .collect::<HashMap<_, _>>();
    let diff = build_diff(snapshot, plan, &files, no_diff)?;
    if let Some(warning) = diff.warning {
        warnings.push(warning);
    }

    let outputs = plan
        .outputs
        .iter()
        .map(|output| {
            let before_length = original_bytes(snapshot, &output.path.value)
                .map(|bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
            OutputResponse::new(
                output.path.value.clone(),
                output.change,
                before_length,
                output.original_digest,
                output.resulting_length,
                output.resulting_digest,
            )
        })
        .collect();
    let resolved_operations = plan
        .operations
        .iter()
        .map(ResolvedOperationResponse::from_resolved)
        .collect();
    let human = render_human(
        snapshot,
        plan,
        workspace_identity_hash,
        &diff.human,
        &warnings,
    );
    let response = PreviewResponse::new(
        plan.digest.0,
        workspace_identity_hash,
        resolved_operations,
        outputs,
        diff.response,
        warnings,
    );
    Ok(PreviewArtifacts { response, human })
}

struct BuiltDiff {
    response: DiffResponse,
    human: String,
    warning: Option<WarningDto>,
}

fn build_diff(
    snapshot: &WorkspaceSnapshot,
    plan: &EditPlan,
    files: &HashMap<u64, &[u8]>,
    no_diff: bool,
) -> Result<BuiltDiff, FsError> {
    if no_diff {
        return Ok(BuiltDiff {
            response: DiffResponse::omitted(),
            human: "omitted (--no-diff)\n".to_owned(),
            warning: None,
        });
    }

    let changed = plan
        .outputs
        .iter()
        .filter(|output| output.change != OutputChange::Unchanged)
        .collect::<Vec<_>>();
    if changed.is_empty() {
        return Ok(BuiltDiff {
            response: DiffResponse::text(Some(String::new()), None),
            human: "(no byte changes)\n".to_owned(),
            warning: None,
        });
    }

    let detailed_input_limited = changed.iter().any(|output| {
        let before = original_bytes(snapshot, &output.path.value)
            .map_or(0, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        before > DETAILED_DIFF_INPUT_BYTES || output.resulting_length > DETAILED_DIFF_INPUT_BYTES
    });
    if detailed_input_limited {
        let summary = summary_value(snapshot, &changed, files, "detailed_input_limit")?;
        return Ok(BuiltDiff {
            response: DiffResponse::text(None, Some(summary.clone())),
            human: format!(
                "summary (detailed input limit): {}\n",
                escape_terminal_text(&summary.to_string())
            ),
            warning: Some(diff_truncated_warning("detailed_input_limit")),
        });
    }

    let mut binary = false;
    for output in &changed {
        let before = original_bytes(snapshot, &output.path.value).unwrap_or_default();
        let after = materialize_output(output, files)?;
        if is_binary(before) || is_binary(&after) {
            binary = true;
            break;
        }
    }
    if binary {
        let summary = summary_value(snapshot, &changed, files, "binary_content")?;
        return Ok(BuiltDiff {
            response: DiffResponse::binary(summary.clone()),
            human: format!(
                "binary summary: {}\n",
                escape_terminal_text(&summary.to_string())
            ),
            warning: None,
        });
    }

    let mut writer = DiffWriter::default();
    for output in changed {
        let before = original_bytes(snapshot, &output.path.value).unwrap_or_default();
        let after = materialize_output(output, files)?;
        if !writer.write_output(&output.path.value, before, &after)? {
            break;
        }
    }
    let truncated = writer.truncated;
    let text = writer.text;
    let summary = truncated.then(|| {
        json!({
            "reason": "diff_budget",
            "maximum_diff_bytes": DIFF_OUTPUT_BYTES,
            "maximum_work_units": DIFF_WORK_UNITS
        })
    });
    Ok(BuiltDiff {
        response: DiffResponse::text(Some(text.clone()), summary),
        human: if text.is_empty() {
            "(no byte changes)\n".to_owned()
        } else {
            text
        },
        warning: truncated.then(|| diff_truncated_warning("diff_budget")),
    })
}

fn diff_truncated_warning(reason: &'static str) -> WarningDto {
    WarningDto::new(
        WarningCode::DiffTruncated,
        "the bounded preview diff omitted detail",
        BTreeMap::from([("reason".to_owned(), json!(reason))]),
    )
}

fn is_binary(bytes: &[u8]) -> bool {
    bytes.contains(&0) || std::str::from_utf8(bytes).is_err()
}

fn original_bytes<'a>(snapshot: &'a WorkspaceSnapshot, path: &str) -> Option<&'a [u8]> {
    snapshot
        .files
        .iter()
        .find(|file| file.path.value == path)
        .map(|file| file.bytes.as_ref())
}

fn materialize_output(
    output: &PlannedOutput,
    files: &HashMap<u64, &[u8]>,
) -> Result<Vec<u8>, FsError> {
    let capacity =
        usize::try_from(output.resulting_length).map_err(|_| FsError::InternalInvariant {
            invariant: "preview_output_length_fits_usize",
        })?;
    let mut bytes = Vec::with_capacity(capacity);
    visit_output_chunks(output, files, |chunk| bytes.extend_from_slice(chunk))?;
    if bytes.len() != capacity {
        return Err(FsError::InternalInvariant {
            invariant: "preview_output_recipe_length",
        });
    }
    Ok(bytes)
}

fn visit_output_chunks(
    output: &PlannedOutput,
    files: &HashMap<u64, &[u8]>,
    mut visit: impl FnMut(&[u8]),
) -> Result<(), FsError> {
    for segment in output.segments.iter() {
        let (file_id, range) = match segment {
            OutputSegment::OriginalSlice {
                snapshot_file_id,
                range,
            }
            | OutputSegment::PayloadSlice {
                snapshot_file_id,
                range,
                ..
            } => (snapshot_file_id.0, range),
        };
        let bytes = files.get(&file_id).ok_or(FsError::InternalInvariant {
            invariant: "preview_segment_snapshot_file",
        })?;
        let start = usize::try_from(range.start).map_err(|_| FsError::InternalInvariant {
            invariant: "preview_segment_start_fits_usize",
        })?;
        let end = usize::try_from(range.end).map_err(|_| FsError::InternalInvariant {
            invariant: "preview_segment_end_fits_usize",
        })?;
        let chunk = bytes.get(start..end).ok_or(FsError::InternalInvariant {
            invariant: "preview_segment_range",
        })?;
        visit(chunk);
    }
    Ok(())
}

fn summary_value(
    snapshot: &WorkspaceSnapshot,
    outputs: &[&PlannedOutput],
    files: &HashMap<u64, &[u8]>,
    reason: &'static str,
) -> Result<Value, FsError> {
    let entries = outputs
        .iter()
        .map(|output| summary_entry(snapshot, output, files))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(json!({"reason": reason, "outputs": entries}))
}

fn summary_entry(
    snapshot: &WorkspaceSnapshot,
    output: &PlannedOutput,
    files: &HashMap<u64, &[u8]>,
) -> Result<Value, FsError> {
    let before = original_bytes(snapshot, &output.path.value);
    let before_samples = before.map(samples_for_bytes);
    let after_samples = samples_for_output(output, files)?;
    Ok(json!({
        "path": output.path.value,
        "before_length": before.map(|bytes| bytes.len()),
        "before_sha256": output.original_digest.map(Sha256Digest::to_prefixed_hex),
        "after_length": output.resulting_length,
        "after_sha256": output.resulting_digest.to_prefixed_hex(),
        "before_samples": before_samples,
        "after_samples": after_samples
    }))
}

#[derive(serde::Serialize)]
struct EncodedSamples {
    head_base64: String,
    tail_base64: String,
}

fn samples_for_bytes(bytes: &[u8]) -> EncodedSamples {
    let head_end = bytes.len().min(SAMPLE_BYTES);
    let tail_start = bytes.len().saturating_sub(SAMPLE_BYTES);
    EncodedSamples {
        head_base64: base64_encode(&bytes[..head_end]),
        tail_base64: base64_encode(&bytes[tail_start..]),
    }
}

fn samples_for_output(
    output: &PlannedOutput,
    files: &HashMap<u64, &[u8]>,
) -> Result<EncodedSamples, FsError> {
    let mut head = Vec::with_capacity(SAMPLE_BYTES);
    let mut tail = VecDeque::with_capacity(SAMPLE_BYTES);
    visit_output_chunks(output, files, |chunk| {
        let remaining = SAMPLE_BYTES.saturating_sub(head.len());
        head.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
        for byte in chunk {
            if tail.len() == SAMPLE_BYTES {
                tail.pop_front();
            }
            tail.push_back(*byte);
        }
    })?;
    Ok(EncodedSamples {
        head_base64: base64_encode(&head),
        tail_base64: base64_encode(&tail.into_iter().collect::<Vec<_>>()),
    })
}

fn base64_encode(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut encoded = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let first = chunk[0];
        let second = chunk.get(1).copied().unwrap_or(0);
        let third = chunk.get(2).copied().unwrap_or(0);
        encoded.push(char::from(ALPHABET[usize::from(first >> 2)]));
        encoded.push(char::from(
            ALPHABET[usize::from(((first & 0x03) << 4) | (second >> 4))],
        ));
        if chunk.len() > 1 {
            encoded.push(char::from(
                ALPHABET[usize::from(((second & 0x0f) << 2) | (third >> 6))],
            ));
        } else {
            encoded.push('=');
        }
        if chunk.len() > 2 {
            encoded.push(char::from(ALPHABET[usize::from(third & 0x3f)]));
        } else {
            encoded.push('=');
        }
    }
    encoded
}

#[derive(Default)]
struct DiffWriter {
    text: String,
    encoded_bytes: usize,
    work_units: u64,
    truncated: bool,
}

impl DiffWriter {
    fn write_output(&mut self, path: &str, before: &[u8], after: &[u8]) -> Result<bool, FsError> {
        self.charge(
            u64::try_from(before.len())
                .unwrap_or(u64::MAX)
                .saturating_add(u64::try_from(after.len()).unwrap_or(u64::MAX)),
        );
        if self.truncated {
            return Ok(false);
        }
        let before_lines = text_lines(before)?;
        let after_lines = text_lines(after)?;
        let prefix = before_lines
            .iter()
            .zip(&after_lines)
            .take_while(|(left, right)| left.full == right.full)
            .count();
        let suffix = before_lines[prefix..]
            .iter()
            .rev()
            .zip(after_lines[prefix..].iter().rev())
            .take_while(|(left, right)| left.full == right.full)
            .count();
        let before_end = before_lines.len().saturating_sub(suffix);
        let after_end = after_lines.len().saturating_sub(suffix);

        let safe_path = escape_terminal_text(path);
        if !self.push(&format!("--- {safe_path} (before)\n"))
            || !self.push(&format!("+++ {safe_path} (after)\n"))
            || !self.push(&format!(
                "@@ -{},{} +{},{} @@\n",
                prefix.saturating_add(1),
                before_end.saturating_sub(prefix),
                prefix.saturating_add(1),
                after_end.saturating_sub(prefix)
            ))
        {
            return Ok(false);
        }
        for line in &before_lines[prefix..before_end] {
            if !self.write_line('-', line) {
                return Ok(false);
            }
        }
        for line in &after_lines[prefix..after_end] {
            if !self.write_line('+', line) {
                return Ok(false);
            }
        }
        Ok(true)
    }

    fn write_line(&mut self, prefix: char, line: &TextLine<'_>) -> bool {
        let content = escape_terminal_text(line.content);
        self.push(&format!("{prefix}{content} [{}]\n", line.terminator))
    }

    fn charge(&mut self, units: u64) {
        self.work_units = self.work_units.saturating_add(units);
        if self.work_units > DIFF_WORK_UNITS {
            self.truncated = true;
        }
    }

    fn push(&mut self, value: &str) -> bool {
        self.charge(u64::try_from(value.len()).unwrap_or(u64::MAX));
        let encoded_bytes = json_string_content_bytes(value);
        if self.truncated
            || self.text.len().saturating_add(value.len()) > DIFF_OUTPUT_BYTES
            || self.encoded_bytes.saturating_add(encoded_bytes) > DIFF_OUTPUT_BYTES
        {
            self.truncated = true;
            return false;
        }
        self.text.push_str(value);
        self.encoded_bytes += encoded_bytes;
        true
    }
}

fn json_string_content_bytes(value: &str) -> usize {
    value
        .as_bytes()
        .iter()
        .map(|byte| match byte {
            b'"' | b'\\' | b'\n' | b'\r' | b'\t' | 0x08 | 0x0c => 2,
            0x00..=0x1f => 6,
            _ => 1,
        })
        .sum()
}

struct TextLine<'a> {
    full: &'a [u8],
    content: &'a str,
    terminator: &'static str,
}

fn text_lines(bytes: &[u8]) -> Result<Vec<TextLine<'_>>, FsError> {
    let text = std::str::from_utf8(bytes).map_err(|_| FsError::InternalInvariant {
        invariant: "text_diff_received_binary_bytes",
    })?;
    let mut lines = Vec::new();
    let mut start = 0;
    let mut offset = 0;
    while offset < bytes.len() {
        let (end, content_end, terminator) = match bytes[offset] {
            b'\n' => (offset + 1, offset, "LF"),
            b'\r' if bytes.get(offset + 1) == Some(&b'\n') => (offset + 2, offset, "CRLF"),
            b'\r' => (offset + 1, offset, "CR"),
            _ => {
                offset += 1;
                continue;
            }
        };
        lines.push(TextLine {
            full: &bytes[start..end],
            content: &text[start..content_end],
            terminator,
        });
        start = end;
        offset = end;
    }
    if start < bytes.len() {
        lines.push(TextLine {
            full: &bytes[start..],
            content: &text[start..],
            terminator: "NONE",
        });
    }
    Ok(lines)
}

fn render_human(
    snapshot: &WorkspaceSnapshot,
    plan: &EditPlan,
    workspace_identity_hash: Sha256Digest,
    diff: &str,
    warnings: &[WarningDto],
) -> String {
    let mut report = format!(
        "CodeSplice preview\nplan_hash_version: 1\nplan_sha256: {}\nworkspace_identity_hash: {}\nresolved_operations:\n",
        plan.digest.0.to_prefixed_hex(),
        workspace_identity_hash.to_prefixed_hex()
    );
    for operation in plan.operations.iter() {
        report.push_str(&format!(
            "- [{}] {} {}[{}..{}) -> {}@{} effect={} selected_payload_sha256={}\n",
            operation.operation_index,
            match operation.kind {
                OperationKind::Move => "move",
                OperationKind::Copy => "copy",
            },
            escape_terminal_text(&operation.source_path.value),
            operation.source_range.start,
            operation.source_range.end,
            escape_terminal_text(&operation.destination_path.value),
            operation.destination_offset,
            match operation.effect {
                OperationEffect::Changed => "changed",
                OperationEffect::NoOp => "no_op",
            },
            operation.selected_digest.to_prefixed_hex()
        ));
    }
    report.push_str("outputs:\n");
    for output in plan.outputs.iter() {
        let before_length = original_bytes(snapshot, &output.path.value)
            .map(|bytes| bytes.len().to_string())
            .unwrap_or_else(|| "null".to_owned());
        let before_digest = output
            .original_digest
            .map(Sha256Digest::to_prefixed_hex)
            .unwrap_or_else(|| "null".to_owned());
        report.push_str(&format!(
            "- {} change={} before_length={} before_sha256={} after_length={} after_sha256={}\n",
            escape_terminal_text(&output.path.value),
            output_change_name(output.change),
            before_length,
            before_digest,
            output.resulting_length,
            output.resulting_digest.to_prefixed_hex()
        ));
    }
    report.push_str("diff:\n");
    report.push_str(diff);
    report.push_str("warnings:\n");
    for warning in warnings {
        report.push_str(&format!(
            "- {}: {}\n",
            warning.code().as_str(),
            escape_terminal_text(warning.message())
        ));
    }
    report
}

fn output_change_name(change: OutputChange) -> &'static str {
    match change {
        OutputChange::Unchanged => "unchanged",
        OutputChange::ModifiedExisting => "modified_existing",
        OutputChange::CreatedNew => "created_new",
        OutputChange::EmptiedExisting => "emptied_existing",
    }
}

#[cfg(test)]
mod tests {
    use super::base64_encode;

    #[test]
    fn base64_should_encode_padding_boundaries() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }
}
