use std::collections::{BTreeMap, HashMap, VecDeque};

use codesplice_core::{
    EditPlan, FileSnapshot, LineMetrics, OperationEffect, OperationKind, OutputChange,
    OutputSegment, PlannedOutput, Sha256Digest, WorkspaceSnapshot,
};
use codesplice_fs::FsError;
use codesplice_protocol::{
    DiffResponse, InsertionGroupResponse, MAX_RESPONSE_BYTES, OutputResponse, PreviewResponse,
    ResolvedOperationResponse, ReviewOperationResponse, ReviewOutputResponse,
    ReviewSummaryResponse, WarningCode, WarningDto, escape_terminal_text, to_json_line,
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
    include_summary: bool,
    mut warnings: Vec<WarningDto>,
) -> Result<PreviewArtifacts, FsError> {
    let files = snapshot
        .files
        .iter()
        .map(|file| (file.id.0, file))
        .collect::<HashMap<_, _>>();
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
        .collect::<Vec<_>>();
    let resolved_operations = plan
        .operations
        .iter()
        .map(ResolvedOperationResponse::from_resolved)
        .collect::<Vec<_>>();
    let review = include_summary
        .then(|| build_review_summary(snapshot, plan, &files))
        .transpose()?;
    let review_value = review
        .as_ref()
        .map(serde_json::to_value)
        .transpose()
        .map_err(|_| FsError::InternalInvariant {
            invariant: "review_summary_serializes",
        })?;
    let response_budget_context = review_value.as_ref().map(|_| ResponseBudgetContext {
        plan,
        workspace_identity_hash,
        resolved_operations: &resolved_operations,
        outputs: &outputs,
        warnings: &warnings,
    });
    let diff = build_diff(
        snapshot,
        plan,
        &files,
        no_diff,
        review_value.as_ref(),
        response_budget_context,
    )?;
    if let Some(warning) = diff.warning {
        warnings.push(warning);
    }
    let human = render_human(
        snapshot,
        plan,
        workspace_identity_hash,
        &diff.human,
        &warnings,
        review.as_ref(),
    )?;
    let response = PreviewResponse::new(
        plan.digest.0,
        workspace_identity_hash,
        resolved_operations,
        outputs,
        diff.response,
        warnings,
    );
    if include_summary {
        enforce_complete_summary_response(&response)?;
    }
    Ok(PreviewArtifacts { response, human })
}

fn build_review_summary(
    snapshot: &WorkspaceSnapshot,
    plan: &EditPlan,
    files: &HashMap<u64, &FileSnapshot>,
) -> Result<ReviewSummaryResponse, FsError> {
    let operations = plan
        .operations
        .iter()
        .map(|operation| {
            let source = snapshot_file_for_path(snapshot, &operation.source_path.value)?;
            let metrics = source
                .line_index
                .metrics_for_range(&source.bytes, operation.source_range)?;
            Ok(ReviewOperationResponse::new(
                operation.operation_index,
                metrics.byte_count(),
                metrics.line_count(),
            ))
        })
        .collect::<Result<Vec<_>, FsError>>()?;
    let outputs = plan
        .outputs
        .iter()
        .enumerate()
        .map(|(output_index, output)| {
            build_review_output(output_index, output, snapshot, plan, files)
        })
        .collect::<Result<Vec<_>, FsError>>()?;
    Ok(ReviewSummaryResponse::v1(
        plan.digest.0,
        operations,
        outputs,
    ))
}

fn build_review_output(
    output_index: usize,
    output: &PlannedOutput,
    snapshot: &WorkspaceSnapshot,
    plan: &EditPlan,
    files: &HashMap<u64, &FileSnapshot>,
) -> Result<ReviewOutputResponse, FsError> {
    let before_logical_line_count = snapshot
        .files
        .iter()
        .find(|file| file.path.value == output.path.value)
        .map(|file| file.line_index.line_count());
    let mut after_metrics = LineMetrics::default();
    let mut insertion_groups = Vec::<(u64, Vec<u64>)>::new();

    for segment in output.segments.iter() {
        let (snapshot_file_id, range) = match segment {
            OutputSegment::OriginalSlice {
                snapshot_file_id,
                range,
            }
            | OutputSegment::PayloadSlice {
                snapshot_file_id,
                range,
                ..
            } => (snapshot_file_id.0, *range),
        };
        let file = files
            .get(&snapshot_file_id)
            .copied()
            .ok_or(FsError::InternalInvariant {
                invariant: "review_segment_snapshot_file",
            })?;
        let segment_metrics = file.line_index.metrics_for_range(&file.bytes, range)?;
        after_metrics.try_append(segment_metrics)?;

        if let OutputSegment::PayloadSlice {
            operation_index, ..
        } = segment
        {
            let operation_position =
                usize::try_from(*operation_index).map_err(|_| FsError::InternalInvariant {
                    invariant: "review_operation_index_fits_usize",
                })?;
            let operation =
                plan.operations
                    .get(operation_position)
                    .ok_or(FsError::InternalInvariant {
                        invariant: "review_segment_operation_index",
                    })?;
            if operation.operation_index != *operation_index
                || operation.effect != OperationEffect::Changed
                || operation.destination_path != output.path
            {
                return Err(FsError::InternalInvariant {
                    invariant: "review_effectful_insertion",
                });
            }
            let destination_offset = operation.destination_offset;
            if let Some((group_offset, operation_indices)) = insertion_groups.last_mut()
                && *group_offset == destination_offset
            {
                operation_indices.push(*operation_index);
            } else {
                insertion_groups.push((destination_offset, vec![*operation_index]));
            }
        }
    }
    if after_metrics.byte_count() != output.resulting_length {
        return Err(FsError::InternalInvariant {
            invariant: "review_output_recipe_length",
        });
    }
    let output_index = u64::try_from(output_index).map_err(|_| FsError::InternalInvariant {
        invariant: "review_output_index_fits_u64",
    })?;
    Ok(ReviewOutputResponse::new(
        output_index,
        before_logical_line_count,
        after_metrics.line_count(),
        insertion_groups
            .into_iter()
            .map(|(offset, operation_indices)| {
                InsertionGroupResponse::new(offset, operation_indices)
            })
            .collect(),
    ))
}

fn snapshot_file_for_path<'a>(
    snapshot: &'a WorkspaceSnapshot,
    path: &str,
) -> Result<&'a FileSnapshot, FsError> {
    snapshot
        .files
        .iter()
        .find(|file| file.path.value == path)
        .ok_or(FsError::InternalInvariant {
            invariant: "review_operation_source_file",
        })
}

fn review_only_summary_value(review: &Value) -> Value {
    let mut summary = serde_json::Map::new();
    summary.insert("review".to_owned(), review.clone());
    Value::Object(summary)
}

fn merge_review_summary(
    legacy: Option<Value>,
    review: Option<&Value>,
) -> Result<Option<Value>, FsError> {
    match review {
        Some(review) => match legacy {
            Some(Value::Object(mut summary)) => {
                summary.insert("review".to_owned(), review.clone());
                Ok(Some(Value::Object(summary)))
            }
            None => Ok(Some(review_only_summary_value(review))),
            Some(_) => Err(FsError::InternalInvariant {
                invariant: "preview_summary_is_object",
            }),
        },
        None => Ok(legacy),
    }
}

fn response_aware_diff_budget(
    plan: &EditPlan,
    workspace_identity_hash: Sha256Digest,
    resolved_operations: &[ResolvedOperationResponse],
    outputs: &[OutputResponse],
    review: &Value,
    warnings: &[WarningDto],
) -> Result<usize, FsError> {
    let mut reserved_warnings = warnings.to_vec();
    reserved_warnings.push(diff_truncated_warning("response_budget"));
    let reserved_summary = merge_review_summary(
        Some(json!({
            "reason": "response_budget",
            "maximum_diff_bytes": DIFF_OUTPUT_BYTES,
            "maximum_work_units": DIFF_WORK_UNITS
        })),
        Some(review),
    )?
    .ok_or(FsError::InternalInvariant {
        invariant: "reserved_review_summary",
    })?;
    let reserved = PreviewResponse::new(
        plan.digest.0,
        workspace_identity_hash,
        resolved_operations.to_vec(),
        outputs.to_vec(),
        DiffResponse::text(Some(String::new()), Some(reserved_summary)),
        reserved_warnings,
    );
    let reserved_bytes = to_json_line(&reserved)
        .map_err(|_| FsError::InternalInvariant {
            invariant: "preview_response_budget_serializes",
        })?
        .len();
    let maximum_response_bytes =
        usize::try_from(MAX_RESPONSE_BYTES).map_err(|_| FsError::InternalInvariant {
            invariant: "maximum_response_bytes_fits_usize",
        })?;
    let remaining = maximum_response_bytes.checked_sub(reserved_bytes).ok_or(
        codesplice_core::CoreError::ResourceLimitExceeded {
            resource: "serialized_json_response",
            actual: u64::try_from(reserved_bytes).unwrap_or(u64::MAX),
            limit: MAX_RESPONSE_BYTES,
        },
    )?;
    Ok(remaining.min(DIFF_OUTPUT_BYTES))
}

fn enforce_complete_summary_response(response: &PreviewResponse) -> Result<(), FsError> {
    let actual = to_json_line(response)
        .map_err(|_| FsError::InternalInvariant {
            invariant: "complete_review_response_serializes",
        })?
        .len();
    if u64::try_from(actual).unwrap_or(u64::MAX) > MAX_RESPONSE_BYTES {
        return Err(codesplice_core::CoreError::ResourceLimitExceeded {
            resource: "serialized_json_response",
            actual: u64::try_from(actual).unwrap_or(u64::MAX),
            limit: MAX_RESPONSE_BYTES,
        }
        .into());
    }
    Ok(())
}

struct BuiltDiff {
    response: DiffResponse,
    human: String,
    warning: Option<WarningDto>,
}

#[derive(Clone, Copy)]
struct ResponseBudgetContext<'a> {
    plan: &'a EditPlan,
    workspace_identity_hash: Sha256Digest,
    resolved_operations: &'a [ResolvedOperationResponse],
    outputs: &'a [OutputResponse],
    warnings: &'a [WarningDto],
}

fn build_diff(
    snapshot: &WorkspaceSnapshot,
    plan: &EditPlan,
    files: &HashMap<u64, &FileSnapshot>,
    no_diff: bool,
    review: Option<&Value>,
    response_budget_context: Option<ResponseBudgetContext<'_>>,
) -> Result<BuiltDiff, FsError> {
    if no_diff {
        let response = if let Some(review) = review {
            DiffResponse::omitted_with_summary(review_only_summary_value(review))
        } else {
            DiffResponse::omitted()
        };
        return Ok(BuiltDiff {
            response,
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
            response: DiffResponse::text(
                Some(String::new()),
                review.map(review_only_summary_value),
            ),
            human: "(no byte changes)\n".to_owned(),
            warning: None,
        });
    }

    let detailed_input_limited = changed.iter().any(|output| {
        let before = original_bytes(snapshot, &output.path.value)
            .map_or(0, |bytes| u64::try_from(bytes.len()).unwrap_or(u64::MAX));
        detailed_input_exceeded(before) || detailed_input_exceeded(output.resulting_length)
    });
    if detailed_input_limited {
        let legacy_summary = summary_value(snapshot, &changed, files, "detailed_input_limit")?;
        let summary = merge_review_summary(Some(legacy_summary.clone()), review)?;
        return Ok(BuiltDiff {
            response: DiffResponse::text(None, summary),
            human: format!(
                "summary (detailed input limit): {}\n",
                escape_terminal_text(&legacy_summary.to_string())
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
        let legacy_summary = summary_value(snapshot, &changed, files, "binary_content")?;
        let summary = merge_review_summary(Some(legacy_summary.clone()), review)?.ok_or(
            FsError::InternalInvariant {
                invariant: "binary_preview_summary",
            },
        )?;
        return Ok(BuiltDiff {
            response: DiffResponse::binary(summary),
            human: format!(
                "binary summary: {}\n",
                escape_terminal_text(&legacy_summary.to_string())
            ),
            warning: None,
        });
    }

    let maximum_output_bytes = match (response_budget_context, review) {
        (Some(context), Some(review)) => response_aware_diff_budget(
            context.plan,
            context.workspace_identity_hash,
            context.resolved_operations,
            context.outputs,
            review,
            context.warnings,
        )?,
        (None, _) => DIFF_OUTPUT_BYTES,
        (Some(_), None) => {
            return Err(FsError::InternalInvariant {
                invariant: "response_budget_requires_review",
            });
        }
    };
    let mut writer = DiffWriter::with_maximum_output_bytes(maximum_output_bytes);
    for output in changed {
        let before = original_bytes(snapshot, &output.path.value).unwrap_or_default();
        let after = materialize_output(output, files)?;
        if !writer.write_output(&output.path.value, before, &after)? {
            break;
        }
    }
    let truncation = writer.truncation;
    let text = writer.text;
    let truncation_reason =
        truncation.map(|cause| diff_truncation_reason(cause, maximum_output_bytes));
    let legacy_summary = truncation_reason.map(|reason| {
        json!({
            "reason": reason,
            "maximum_diff_bytes": DIFF_OUTPUT_BYTES,
            "maximum_work_units": DIFF_WORK_UNITS
        })
    });
    let summary = merge_review_summary(legacy_summary, review)?;
    Ok(BuiltDiff {
        response: DiffResponse::text(Some(text.clone()), summary),
        human: if text.is_empty() {
            "(no byte changes)\n".to_owned()
        } else {
            text
        },
        warning: truncation_reason.map(diff_truncated_warning),
    })
}

fn diff_truncation_reason(truncation: DiffTruncation, maximum_output_bytes: usize) -> &'static str {
    if truncation == DiffTruncation::Output && maximum_output_bytes < DIFF_OUTPUT_BYTES {
        "response_budget"
    } else {
        "diff_budget"
    }
}

const fn detailed_input_exceeded(length: u64) -> bool {
    length > DETAILED_DIFF_INPUT_BYTES
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
    files: &HashMap<u64, &FileSnapshot>,
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
    files: &HashMap<u64, &FileSnapshot>,
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
        let file = files.get(&file_id).ok_or(FsError::InternalInvariant {
            invariant: "preview_segment_snapshot_file",
        })?;
        let start = usize::try_from(range.start).map_err(|_| FsError::InternalInvariant {
            invariant: "preview_segment_start_fits_usize",
        })?;
        let end = usize::try_from(range.end).map_err(|_| FsError::InternalInvariant {
            invariant: "preview_segment_end_fits_usize",
        })?;
        let chunk = file
            .bytes
            .get(start..end)
            .ok_or(FsError::InternalInvariant {
                invariant: "preview_segment_range",
            })?;
        visit(chunk);
    }
    Ok(())
}

fn summary_value(
    snapshot: &WorkspaceSnapshot,
    outputs: &[&PlannedOutput],
    files: &HashMap<u64, &FileSnapshot>,
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
    files: &HashMap<u64, &FileSnapshot>,
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
    files: &HashMap<u64, &FileSnapshot>,
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

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum DiffTruncation {
    Work,
    Output,
}

struct DiffWriter {
    text: String,
    encoded_bytes: usize,
    work_units: u64,
    maximum_output_bytes: usize,
    truncation: Option<DiffTruncation>,
}

impl Default for DiffWriter {
    fn default() -> Self {
        Self::with_maximum_output_bytes(DIFF_OUTPUT_BYTES)
    }
}

impl DiffWriter {
    fn with_maximum_output_bytes(maximum_output_bytes: usize) -> Self {
        Self {
            text: String::new(),
            encoded_bytes: 0,
            work_units: 0,
            maximum_output_bytes,
            truncation: None,
        }
    }

    fn write_output(&mut self, path: &str, before: &[u8], after: &[u8]) -> Result<bool, FsError> {
        self.charge(
            u64::try_from(before.len())
                .unwrap_or(u64::MAX)
                .saturating_add(u64::try_from(after.len()).unwrap_or(u64::MAX)),
        );
        if self.truncation.is_some() {
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
        if self.work_units > DIFF_WORK_UNITS && self.truncation.is_none() {
            self.truncation = Some(DiffTruncation::Work);
        }
    }

    fn push(&mut self, value: &str) -> bool {
        self.charge(u64::try_from(value.len()).unwrap_or(u64::MAX));
        let encoded_bytes = json_string_content_bytes(value);
        if self.truncation.is_some()
            || self.text.len().saturating_add(value.len()) > self.maximum_output_bytes
            || self.encoded_bytes.saturating_add(encoded_bytes) > self.maximum_output_bytes
        {
            if self.truncation.is_none() {
                self.truncation = Some(DiffTruncation::Output);
            }
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
    review: Option<&ReviewSummaryResponse>,
) -> Result<String, FsError> {
    let mut report = format!(
        "CodeSplice preview\nplan_hash_version: 1\nplan_sha256: {}\nworkspace_identity_hash: {}\nresolved_operations:\n",
        plan.digest.0.to_prefixed_hex(),
        workspace_identity_hash.to_prefixed_hex()
    );
    for (operation_position, operation) in plan.operations.iter().enumerate() {
        let review_metrics = review
            .map(|review| {
                review
                    .operations()
                    .get(operation_position)
                    .filter(|metrics| metrics.operation_index() == operation.operation_index)
                    .ok_or(FsError::InternalInvariant {
                        invariant: "human_review_operation_index",
                    })
            })
            .transpose()?;
        let mut row = format!(
            "- [{}] {} {}[{}..{}) -> {}@{} effect={} selected_payload_sha256={}",
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
        );
        if let Some(metrics) = review_metrics {
            row.push_str(&format!(
                " selected_byte_length={} selected_logical_line_count={}",
                metrics.selected_byte_length(),
                metrics.selected_logical_line_count()
            ));
        }
        row.push('\n');
        report.push_str(&row);
    }
    report.push_str("outputs:\n");
    for (output_position, output) in plan.outputs.iter().enumerate() {
        let review_metrics = review
            .map(|review| {
                review
                    .outputs()
                    .get(output_position)
                    .filter(|metrics| {
                        usize::try_from(metrics.output_index()).ok() == Some(output_position)
                    })
                    .ok_or(FsError::InternalInvariant {
                        invariant: "human_review_output_index",
                    })
            })
            .transpose()?;
        let before_length = original_bytes(snapshot, &output.path.value)
            .map(|bytes| bytes.len().to_string())
            .unwrap_or_else(|| "null".to_owned());
        let before_digest = output
            .original_digest
            .map(Sha256Digest::to_prefixed_hex)
            .unwrap_or_else(|| "null".to_owned());
        let safe_path = escape_terminal_text(&output.path.value);
        let mut row = if review_metrics.is_some() {
            format!(
                "- [{output_position}] {safe_path} change={} before_length={} before_sha256={} after_length={} after_sha256={}",
                output_change_name(output.change),
                before_length,
                before_digest,
                output.resulting_length,
                output.resulting_digest.to_prefixed_hex()
            )
        } else {
            format!(
                "- {safe_path} change={} before_length={} before_sha256={} after_length={} after_sha256={}",
                output_change_name(output.change),
                before_length,
                before_digest,
                output.resulting_length,
                output.resulting_digest.to_prefixed_hex()
            )
        };
        if let Some(metrics) = review_metrics {
            let before_lines = metrics
                .before_logical_line_count()
                .map(|count| count.to_string())
                .unwrap_or_else(|| "null".to_owned());
            row.push_str(&format!(
                " before_logical_line_count={} after_logical_line_count={}",
                before_lines,
                metrics.after_logical_line_count()
            ));
        }
        row.push('\n');
        report.push_str(&row);
        if let Some(metrics) = review_metrics {
            for group in metrics.insertion_groups_in_output_order() {
                let operation_indices = group
                    .operation_indices()
                    .iter()
                    .map(u64::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                report.push_str(&format!(
                    "  insertion_group destination_offset={} operation_indices=[{}]\n",
                    group.destination_offset(),
                    operation_indices
                ));
            }
        }
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
    Ok(report)
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
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn base64_should_encode_padding_boundaries() {
        assert_eq!(base64_encode(b""), "");
        assert_eq!(base64_encode(b"f"), "Zg==");
        assert_eq!(base64_encode(b"fo"), "Zm8=");
        assert_eq!(base64_encode(b"foo"), "Zm9v");
    }

    #[test]
    fn phase9_diff_limits_cover_below_at_and_above_boundaries() {
        assert!(!detailed_input_exceeded(DETAILED_DIFF_INPUT_BYTES - 1));
        assert!(!detailed_input_exceeded(DETAILED_DIFF_INPUT_BYTES));
        assert!(detailed_input_exceeded(DETAILED_DIFF_INPUT_BYTES + 1));

        let exact = "x".repeat(DIFF_OUTPUT_BYTES);
        let mut writer = DiffWriter::default();
        assert!(writer.push(&exact));
        assert!(!writer.push("x"));

        let mut work_limited = DiffWriter {
            work_units: DIFF_WORK_UNITS - 1,
            ..DiffWriter::default()
        };
        work_limited.charge(1);
        assert_eq!(work_limited.truncation, None);
        work_limited.charge(1);
        assert_eq!(work_limited.truncation, Some(DiffTruncation::Work));
    }

    #[test]
    fn response_budget_should_truncate_only_diff_detail_with_the_existing_warning_reason() {
        let mut writer = DiffWriter::with_maximum_output_bytes(4);
        assert!(writer.push("xxxx"));
        assert!(!writer.push("x"));

        assert_eq!(writer.text, "xxxx");
        assert_eq!(writer.truncation, Some(DiffTruncation::Output));
        assert_eq!(
            diff_truncation_reason(DiffTruncation::Output, 4),
            "response_budget"
        );
        assert_eq!(
            diff_truncation_reason(DiffTruncation::Work, 4),
            "diff_budget"
        );
    }

    #[test]
    fn complete_summary_response_should_cover_exact_serialization_boundary() {
        let empty = PreviewResponse::new(
            Sha256Digest([0; 32]),
            Sha256Digest([1; 32]),
            Vec::new(),
            Vec::new(),
            DiffResponse::text(Some(String::new()), Some(json!({"review":{"version":1}}))),
            Vec::new(),
        );
        let fixed_bytes = to_json_line(&empty)
            .expect("fixture should serialize")
            .len();
        let maximum = usize::try_from(MAX_RESPONSE_BYTES).expect("response limit should fit");
        let exact_text_bytes = maximum - fixed_bytes;
        let exact = PreviewResponse::new(
            Sha256Digest([0; 32]),
            Sha256Digest([1; 32]),
            Vec::new(),
            Vec::new(),
            DiffResponse::text(
                Some("x".repeat(exact_text_bytes)),
                Some(json!({"review":{"version":1}})),
            ),
            Vec::new(),
        );
        let above = PreviewResponse::new(
            Sha256Digest([0; 32]),
            Sha256Digest([1; 32]),
            Vec::new(),
            Vec::new(),
            DiffResponse::text(
                Some("x".repeat(exact_text_bytes + 1)),
                Some(json!({"review":{"version":1}})),
            ),
            Vec::new(),
        );

        assert!(enforce_complete_summary_response(&exact).is_ok());
        assert!(matches!(
            enforce_complete_summary_response(&above),
            Err(FsError::Core(
                codesplice_core::CoreError::ResourceLimitExceeded {
                    resource: "serialized_json_response",
                    actual,
                    limit: MAX_RESPONSE_BYTES,
                }
            )) if actual == MAX_RESPONSE_BYTES + 1
        ));
    }

    proptest! {
        #[test]
        fn text_diff_decoder_fuzz_regression_is_total(bytes in prop::collection::vec(any::<u8>(), 0..4096)) {
            let decoded = text_lines(&bytes);
            if let Ok(lines) = decoded {
                let reconstructed = lines
                    .iter()
                    .flat_map(|line| line.full.iter().copied())
                    .collect::<Vec<_>>();
                prop_assert_eq!(reconstructed, bytes);
            } else {
                prop_assert!(std::str::from_utf8(&bytes).is_err());
            }
        }
    }
}
