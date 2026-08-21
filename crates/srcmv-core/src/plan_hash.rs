use std::collections::HashMap;

use sha2::{Digest, Sha256};

use crate::{
    Anchor, CoreError, EditPlan, FileSnapshot, OperationEffect, OperationKind, OutputChange,
    OutputSegment, PlanDigest, Precondition, Selector, Sha256Digest, SnapshotFileId,
    WorkspaceSnapshot,
};

/// Deterministic CBOR plan-record format implemented by this crate.
pub const PLAN_HASH_VERSION: u64 = 1;
const PROTOCOL_VERSION: u64 = 1;
const DOMAIN_PREFIX: &[u8] = b"SRCMV-PLAN-V1\0";

enum InputRecord<'a> {
    Existing(&'a FileSnapshot),
    Absent(&'a crate::AbsentPathSnapshot),
}

impl InputRecord<'_> {
    fn path(&self) -> &str {
        match self {
            Self::Existing(file) => &file.path.value,
            Self::Absent(absent) => &absent.path.value,
        }
    }
}

/// Encodes the positional plan-hash version 1 record as RFC 8949 deterministic CBOR.
///
/// Only definite arrays, unsigned integers, byte/text strings, and `null` are
/// emitted. Integer arguments always use their shortest encoding.
///
/// # Errors
///
/// Returns [`CoreError::InvalidDomainValue`] if the snapshot contains duplicate
/// file identifiers or a segment references a file outside the encoded inputs.
pub fn encode_plan_record(
    snapshot: &WorkspaceSnapshot,
    plan: &EditPlan,
) -> Result<Vec<u8>, CoreError> {
    let mut inputs = snapshot
        .files
        .iter()
        .map(InputRecord::Existing)
        .chain(snapshot.absent_paths.iter().map(InputRecord::Absent))
        .collect::<Vec<_>>();
    inputs.sort_by(|left, right| left.path().as_bytes().cmp(right.path().as_bytes()));

    let mut input_indexes = HashMap::with_capacity(snapshot.files.len());
    for (index, input) in inputs.iter().enumerate() {
        if let InputRecord::Existing(file) = input {
            let index = u64::try_from(index).map_err(|_| invalid("snapshot_input_index"))?;
            if input_indexes.insert(file.id, index).is_some() {
                return Err(invalid("duplicate_snapshot_file_id"));
            }
        }
    }

    let mut encoder = CborEncoder::default();
    encoder.array(6)?;
    encoder.unsigned(PLAN_HASH_VERSION);
    encoder.unsigned(PROTOCOL_VERSION);
    encode_workspace_identity(&mut encoder, snapshot);
    encode_inputs(&mut encoder, &inputs)?;
    encode_operations(&mut encoder, plan)?;
    encode_outputs(&mut encoder, plan, &input_indexes)?;
    Ok(encoder.bytes)
}

/// Computes the domain-separated SHA-256 digest of a deterministic plan record.
///
/// # Errors
///
/// Returns the validation errors documented by [`encode_plan_record`].
pub fn plan_digest(snapshot: &WorkspaceSnapshot, plan: &EditPlan) -> Result<PlanDigest, CoreError> {
    let record = encode_plan_record(snapshot, plan)?;
    let mut hasher = Sha256::new();
    hasher.update(DOMAIN_PREFIX);
    hasher.update(record);
    let digest: [u8; 32] = hasher.finalize().into();
    Ok(PlanDigest(Sha256Digest(digest)))
}

fn encode_workspace_identity(encoder: &mut CborEncoder, snapshot: &WorkspaceSnapshot) {
    encoder.array_infallible(2);
    encoder.unsigned(snapshot.workspace_identity.device);
    encoder.unsigned(snapshot.workspace_identity.inode);
}

fn encode_inputs(encoder: &mut CborEncoder, inputs: &[InputRecord<'_>]) -> Result<(), CoreError> {
    encoder.array(inputs.len())?;
    for input in inputs {
        encoder.array_infallible(2);
        encoder.text(input.path())?;
        match input {
            InputRecord::Absent(absent) => {
                encoder.array_infallible(3);
                encoder.unsigned(0);
                encoder.unsigned(absent.parent_identity.device);
                encoder.unsigned(absent.parent_identity.inode);
            }
            InputRecord::Existing(file) => {
                encoder.array_infallible(7);
                encoder.unsigned(1);
                encoder.unsigned(file.parent_identity.device);
                encoder.unsigned(file.parent_identity.inode);
                encoder.unsigned(file.identity.device);
                encoder.unsigned(file.identity.inode);
                encoder.unsigned(
                    u64::try_from(file.bytes.len()).map_err(|_| invalid("input_length"))?,
                );
                encoder.byte_string(&file.digest.0)?;
            }
        }
    }
    Ok(())
}

fn encode_operations(encoder: &mut CborEncoder, plan: &EditPlan) -> Result<(), CoreError> {
    encoder.array(plan.operations.len())?;
    for operation in plan.operations.iter() {
        encoder.array_infallible(13);
        encoder.unsigned(operation.operation_index);
        encoder.unsigned(match operation.kind {
            OperationKind::Move => 1,
            OperationKind::Copy => 2,
        });
        encoder.text(&operation.source_path.value)?;
        encode_selector(encoder, operation.selector);
        encode_precondition(encoder, &operation.source_precondition)?;
        encoder.unsigned(operation.source_range.start);
        encoder.unsigned(operation.source_range.end);
        encoder.byte_string(&operation.selected_digest.0)?;
        encoder.text(&operation.destination_path.value)?;
        encode_anchor(encoder, operation.anchor);
        encode_precondition(encoder, &operation.destination_precondition)?;
        encoder.unsigned(operation.destination_offset);
        encoder.unsigned(match operation.effect {
            OperationEffect::Changed => 1,
            OperationEffect::NoOp => 2,
        });
    }
    Ok(())
}

fn encode_outputs(
    encoder: &mut CborEncoder,
    plan: &EditPlan,
    input_indexes: &HashMap<SnapshotFileId, u64>,
) -> Result<(), CoreError> {
    let mut outputs = plan.outputs.iter().collect::<Vec<_>>();
    outputs.sort_by(|left, right| left.path.value.as_bytes().cmp(right.path.value.as_bytes()));
    encoder.array(outputs.len())?;
    for output in outputs {
        encoder.array_infallible(6);
        encoder.text(&output.path.value)?;
        if let Some(digest) = output.original_digest {
            encoder.byte_string(&digest.0)?;
        } else {
            encoder.null();
        }
        encoder.byte_string(&output.resulting_digest.0)?;
        encoder.unsigned(output.resulting_length);
        encoder.unsigned(match output.change {
            OutputChange::Unchanged => 1,
            OutputChange::ModifiedExisting => 2,
            OutputChange::CreatedNew => 3,
            OutputChange::EmptiedExisting => 4,
        });
        encoder.array(output.segments.len())?;
        for segment in output.segments.iter() {
            match segment {
                OutputSegment::OriginalSlice {
                    snapshot_file_id,
                    range,
                } => {
                    encoder.array_infallible(4);
                    encoder.unsigned(1);
                    encoder.unsigned(input_index(input_indexes, *snapshot_file_id)?);
                    encoder.unsigned(range.start);
                    encoder.unsigned(range.end);
                }
                OutputSegment::PayloadSlice {
                    operation_index,
                    snapshot_file_id,
                    range,
                    payload_digest,
                } => {
                    encoder.array_infallible(6);
                    encoder.unsigned(2);
                    encoder.unsigned(*operation_index);
                    encoder.unsigned(input_index(input_indexes, *snapshot_file_id)?);
                    encoder.unsigned(range.start);
                    encoder.unsigned(range.end);
                    encoder.byte_string(&payload_digest.0)?;
                }
            }
        }
    }
    Ok(())
}

fn input_index(
    input_indexes: &HashMap<SnapshotFileId, u64>,
    file_id: SnapshotFileId,
) -> Result<u64, CoreError> {
    input_indexes
        .get(&file_id)
        .copied()
        .ok_or_else(|| invalid("segment_snapshot_input_index"))
}

fn encode_selector(encoder: &mut CborEncoder, selector: Selector) {
    match selector {
        Selector::Lines { start, end } => {
            encoder.array_infallible(3);
            encoder.unsigned(1);
            encoder.unsigned(start);
            encoder.unsigned(end);
        }
        Selector::Bytes { start, end } => {
            encoder.array_infallible(3);
            encoder.unsigned(2);
            encoder.unsigned(start);
            encoder.unsigned(end);
        }
    }
}

fn encode_anchor(encoder: &mut CborEncoder, anchor: Anchor) {
    match anchor {
        Anchor::FileStart => {
            encoder.array_infallible(1);
            encoder.unsigned(1);
        }
        Anchor::FileEnd => {
            encoder.array_infallible(1);
            encoder.unsigned(2);
        }
        Anchor::BeforeLine(line) => {
            encoder.array_infallible(2);
            encoder.unsigned(3);
            encoder.unsigned(line);
        }
        Anchor::AfterLine(line) => {
            encoder.array_infallible(2);
            encoder.unsigned(4);
            encoder.unsigned(line);
        }
        Anchor::ByteOffset(offset) => {
            encoder.array_infallible(2);
            encoder.unsigned(5);
            encoder.unsigned(offset);
        }
    }
}

fn encode_precondition(
    encoder: &mut CborEncoder,
    precondition: &Precondition,
) -> Result<(), CoreError> {
    match precondition {
        Precondition::Sha256(digest) => {
            encoder.array_infallible(2);
            encoder.unsigned(1);
            encoder.byte_string(&digest.0)?;
        }
        Precondition::MustNotExist => {
            encoder.array_infallible(1);
            encoder.unsigned(2);
        }
    }
    Ok(())
}

#[derive(Default)]
struct CborEncoder {
    bytes: Vec<u8>,
}

impl CborEncoder {
    fn unsigned(&mut self, value: u64) {
        self.major_value(0, value);
    }

    fn array(&mut self, length: usize) -> Result<(), CoreError> {
        let length = u64::try_from(length).map_err(|_| invalid("cbor_array_length"))?;
        self.major_value(4, length);
        Ok(())
    }

    fn array_infallible(&mut self, length: u64) {
        self.major_value(4, length);
    }

    fn text(&mut self, value: &str) -> Result<(), CoreError> {
        let length = u64::try_from(value.len()).map_err(|_| invalid("cbor_text_length"))?;
        self.major_value(3, length);
        self.bytes.extend_from_slice(value.as_bytes());
        Ok(())
    }

    fn byte_string(&mut self, value: &[u8]) -> Result<(), CoreError> {
        let length = u64::try_from(value.len()).map_err(|_| invalid("cbor_bytes_length"))?;
        self.major_value(2, length);
        self.bytes.extend_from_slice(value);
        Ok(())
    }

    fn null(&mut self) {
        self.bytes.push(0xf6);
    }

    fn major_value(&mut self, major: u8, value: u64) {
        let prefix = major << 5;
        match value {
            0..=23 => self.bytes.push(prefix | value as u8),
            24..=0xff => self.bytes.extend_from_slice(&[prefix | 24, value as u8]),
            0x100..=0xffff => {
                self.bytes.push(prefix | 25);
                self.bytes.extend_from_slice(&(value as u16).to_be_bytes());
            }
            0x1_0000..=0xffff_ffff => {
                self.bytes.push(prefix | 26);
                self.bytes.extend_from_slice(&(value as u32).to_be_bytes());
            }
            _ => {
                self.bytes.push(prefix | 27);
                self.bytes.extend_from_slice(&value.to_be_bytes());
            }
        }
    }
}

fn invalid(field: &'static str) -> CoreError {
    CoreError::InvalidDomainValue { field }
}
