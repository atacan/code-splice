use std::collections::{BTreeMap, HashMap};
use std::mem::size_of;

use sha2::{Digest, Sha256};

use crate::{
    AbsentPathSnapshot, Anchor, BatchSpecification, ByteRange, CoreError, EditPlan, FileSnapshot,
    Operation, OperationEffect, OperationKind, OperationSpecification, OutputChange, OutputSegment,
    PlanDigest, PlannedOutput, PlanningUsage, Precondition, ResolvedOperation, ResourceBudget,
    Selector, Sha256Digest, SnapshotFileId, WorkspaceRelativePath, WorkspaceSnapshot, plan_digest,
};

#[derive(Clone, Copy)]
enum SnapshotState<'a> {
    Existing(&'a FileSnapshot),
    Absent(&'a AbsentPathSnapshot),
}

type StatesByPath<'a> = BTreeMap<&'a str, SnapshotState<'a>>;
type FilesById<'a> = HashMap<SnapshotFileId, &'a FileSnapshot>;

#[derive(Clone, Copy)]
struct Insertion {
    operation_index: u64,
    snapshot_file_id: SnapshotFileId,
    range: ByteRange,
    payload_digest: Sha256Digest,
}

struct OutputEdits {
    path: WorkspaceRelativePath,
    existing_file_id: Option<SnapshotFileId>,
    original_length: u64,
    original_digest: Option<Sha256Digest>,
    deletions: Vec<(u64, ByteRange)>,
    insertions: Vec<(u64, Insertion)>,
}

#[derive(Default)]
struct PointEvents {
    deletion_end: bool,
    insertions: Vec<Insertion>,
    deletion_start: bool,
}

/// Resolves a batch against one immutable snapshot and returns segment recipes.
///
/// The function performs no filesystem access. Every coordinate, equality check,
/// output digest, classification, and plan hash is derived solely from `snapshot`
/// and `batch`.
///
/// # Errors
///
/// Returns [`CoreError::EditConflict`] for invalid coordinates, preconditions, or
/// incompatible edits; [`CoreError::HardLinkNotSupported`] for a changing linked
/// output; and [`CoreError::ResourceLimitExceeded`] before retaining a plan beyond
/// `budget`.
pub fn plan(
    snapshot: &WorkspaceSnapshot,
    batch: &BatchSpecification,
    budget: ResourceBudget,
) -> Result<EditPlan, CoreError> {
    let (states, files_by_id) = index_snapshot(snapshot)?;
    validate_precondition_consistency(batch)?;

    let mut resolved = Vec::with_capacity(batch.operations.len());
    let mut edits = BTreeMap::<WorkspaceRelativePath, OutputEdits>::new();

    for (index, operation) in batch.operations.iter().enumerate() {
        let operation_index = u64::try_from(index).map_err(|_| limit_overflow("operations"))?;
        let (kind, specification) = operation_parts(operation);
        let source = existing_source(&states, specification, operation_index)?;
        let source_range =
            resolve_selector(source, specification.source.selector, operation_index)?;
        let selected_digest = digest_range(source, source_range)?;
        let destination_state = destination_state(&states, specification, operation_index)?;
        let destination_offset = resolve_anchor(
            destination_state,
            specification.destination.anchor,
            operation_index,
        )?;

        let effect = if kind == OperationKind::Move
            && specification.source.path == specification.destination.path
            && (destination_offset == source_range.start || destination_offset == source_range.end)
        {
            OperationEffect::NoOp
        } else {
            OperationEffect::Changed
        };

        resolved.push(ResolvedOperation {
            operation_index,
            kind,
            source_path: specification.source.path.clone(),
            selector: specification.source.selector,
            source_precondition: specification.source.precondition.clone(),
            source_range,
            selected_digest,
            destination_path: specification.destination.path.clone(),
            anchor: specification.destination.anchor,
            destination_precondition: specification.destination.precondition.clone(),
            destination_offset,
            effect,
        });

        if effect == OperationEffect::NoOp {
            continue;
        }

        if kind == OperationKind::Move {
            let source_edits = output_edits(
                &mut edits,
                &specification.source.path,
                SnapshotState::Existing(source),
            )?;
            source_edits.deletions.push((operation_index, source_range));
        }

        let destination_edits = output_edits(
            &mut edits,
            &specification.destination.path,
            destination_state,
        )?;
        destination_edits.insertions.push((
            destination_offset,
            Insertion {
                operation_index,
                snapshot_file_id: source.id,
                range: source_range,
                payload_digest: selected_digest,
            },
        ));
    }

    let mut outputs = Vec::with_capacity(edits.len());
    for edits in edits.into_values() {
        validate_edit_conflicts(&edits)?;
        let segments = build_segments(&edits)?;
        let (resulting_length, resulting_digest, equals_original) =
            stream_output(&segments, &files_by_id, edits.existing_file_id)?;
        let change = classify_output(
            edits.existing_file_id.is_some(),
            resulting_length,
            equals_original,
        );

        if change != OutputChange::Unchanged
            && let Some(file_id) = edits.existing_file_id
        {
            let file = files_by_id
                .get(&file_id)
                .copied()
                .ok_or(CoreError::InvalidDomainValue {
                    field: "output_snapshot_file_id",
                })?;
            if file.link_count != 1 {
                return Err(CoreError::HardLinkNotSupported {
                    path: edits.path,
                    link_count: file.link_count,
                });
            }
        }

        outputs.push(PlannedOutput {
            path: edits.path,
            original_digest: edits.original_digest,
            change,
            resulting_length,
            resulting_digest,
            segments: segments.into(),
        });
    }

    let usage = calculate_usage(&resolved, &outputs)?;
    enforce_usage(usage, budget)?;

    let mut edit_plan = EditPlan {
        operations: resolved.into(),
        outputs: outputs.into(),
        usage,
        digest: PlanDigest(Sha256Digest([0; 32])),
    };
    edit_plan.digest = plan_digest(snapshot, &edit_plan)?;
    Ok(edit_plan)
}

fn index_snapshot<'a>(
    snapshot: &'a WorkspaceSnapshot,
) -> Result<(StatesByPath<'a>, FilesById<'a>), CoreError> {
    let mut states = BTreeMap::new();
    let mut files_by_id = HashMap::new();
    for file in snapshot.files.iter() {
        if states
            .insert(file.path.value.as_str(), SnapshotState::Existing(file))
            .is_some()
        {
            return Err(CoreError::InvalidDomainValue {
                field: "duplicate_snapshot_path",
            });
        }
        if files_by_id.insert(file.id, file).is_some() {
            return Err(CoreError::InvalidDomainValue {
                field: "duplicate_snapshot_file_id",
            });
        }
    }
    for absent in snapshot.absent_paths.iter() {
        if states
            .insert(absent.path.value.as_str(), SnapshotState::Absent(absent))
            .is_some()
        {
            return Err(CoreError::EditConflict {
                reason: "path_used_as_absent_and_existing",
                operation_index: None,
            });
        }
    }
    Ok((states, files_by_id))
}

fn validate_precondition_consistency(batch: &BatchSpecification) -> Result<(), CoreError> {
    let mut by_path = BTreeMap::<&str, &Precondition>::new();
    for (index, operation) in batch.operations.iter().enumerate() {
        let operation_index = u64::try_from(index).map_err(|_| limit_overflow("operations"))?;
        let (_, specification) = operation_parts(operation);
        for (path, precondition) in [
            (
                &specification.source.path.value,
                &specification.source.precondition,
            ),
            (
                &specification.destination.path.value,
                &specification.destination.precondition,
            ),
        ] {
            if let Some(previous) = by_path.insert(path, precondition)
                && previous != precondition
            {
                return Err(CoreError::EditConflict {
                    reason: "incompatible_preconditions",
                    operation_index: Some(operation_index),
                });
            }
        }
    }
    Ok(())
}

fn operation_parts(operation: &Operation) -> (OperationKind, &OperationSpecification) {
    match operation {
        Operation::Move(specification) => (OperationKind::Move, specification),
        Operation::Copy(specification) => (OperationKind::Copy, specification),
    }
}

fn existing_source<'a>(
    states: &BTreeMap<&str, SnapshotState<'a>>,
    specification: &OperationSpecification,
    operation_index: u64,
) -> Result<&'a FileSnapshot, CoreError> {
    let Precondition::Sha256(expected) = specification.source.precondition else {
        return Err(conflict("source_must_exist", operation_index));
    };
    let Some(SnapshotState::Existing(file)) = states
        .get(specification.source.path.value.as_str())
        .copied()
    else {
        return Err(conflict("source_not_in_snapshot", operation_index));
    };
    if file.digest != expected {
        return Err(conflict("source_precondition_failed", operation_index));
    }
    Ok(file)
}

fn destination_state<'a>(
    states: &BTreeMap<&str, SnapshotState<'a>>,
    specification: &OperationSpecification,
    operation_index: u64,
) -> Result<SnapshotState<'a>, CoreError> {
    let Some(state) = states
        .get(specification.destination.path.value.as_str())
        .copied()
    else {
        return Err(conflict("destination_not_in_snapshot", operation_index));
    };
    match (&specification.destination.precondition, state) {
        (Precondition::Sha256(expected), SnapshotState::Existing(file))
            if *expected == file.digest =>
        {
            Ok(state)
        }
        (Precondition::MustNotExist, SnapshotState::Absent(_)) => Ok(state),
        (Precondition::Sha256(_), SnapshotState::Existing(_)) => {
            Err(conflict("destination_precondition_failed", operation_index))
        }
        (Precondition::Sha256(_), SnapshotState::Absent(_))
        | (Precondition::MustNotExist, SnapshotState::Existing(_)) => {
            Err(conflict("destination_state_mismatch", operation_index))
        }
    }
}

fn resolve_selector(
    source: &FileSnapshot,
    selector: Selector,
    operation_index: u64,
) -> Result<ByteRange, CoreError> {
    let length = u64::try_from(source.bytes.len()).map_err(|_| limit_overflow("snapshot_bytes"))?;
    match selector {
        Selector::Lines { start, end } => {
            if start == 0 || start > end {
                return Err(conflict("line_selector_out_of_range", operation_index));
            }
            let Some(start) = source.line_index.line_start(start) else {
                return Err(conflict("line_selector_out_of_range", operation_index));
            };
            let Some(end) = source.line_index.line_end(end) else {
                return Err(conflict("line_selector_out_of_range", operation_index));
            };
            Ok(ByteRange { start, end })
        }
        Selector::Bytes { start, end } if start < end && end <= length => {
            Ok(ByteRange { start, end })
        }
        Selector::Bytes { .. } => Err(conflict("byte_selector_out_of_range", operation_index)),
    }
}

fn resolve_anchor(
    destination: SnapshotState<'_>,
    anchor: Anchor,
    operation_index: u64,
) -> Result<u64, CoreError> {
    match destination {
        SnapshotState::Existing(file) => {
            let length =
                u64::try_from(file.bytes.len()).map_err(|_| limit_overflow("snapshot_bytes"))?;
            match anchor {
                Anchor::FileStart => Ok(0),
                Anchor::FileEnd => Ok(length),
                Anchor::BeforeLine(line) => file
                    .line_index
                    .line_start(line)
                    .ok_or_else(|| conflict("line_anchor_out_of_range", operation_index)),
                Anchor::AfterLine(line) => file
                    .line_index
                    .line_end(line)
                    .ok_or_else(|| conflict("line_anchor_out_of_range", operation_index)),
                Anchor::ByteOffset(offset) if offset <= length => Ok(offset),
                Anchor::ByteOffset(_) => Err(conflict("byte_anchor_out_of_range", operation_index)),
            }
        }
        SnapshotState::Absent(absent) => match anchor {
            Anchor::FileStart | Anchor::FileEnd | Anchor::ByteOffset(0) => Ok(0),
            Anchor::BeforeLine(_) | Anchor::AfterLine(_) | Anchor::ByteOffset(_) => {
                let _ = absent;
                Err(conflict("new_file_anchor_out_of_range", operation_index))
            }
        },
    }
}

fn digest_range(file: &FileSnapshot, range: ByteRange) -> Result<Sha256Digest, CoreError> {
    let bytes = slice_range(&file.bytes, range)?;
    let digest: [u8; 32] = Sha256::digest(bytes).into();
    Ok(Sha256Digest(digest))
}

fn output_edits<'a>(
    edits: &'a mut BTreeMap<WorkspaceRelativePath, OutputEdits>,
    path: &WorkspaceRelativePath,
    state: SnapshotState<'_>,
) -> Result<&'a mut OutputEdits, CoreError> {
    let (existing_file_id, original_length, original_digest) = match state {
        SnapshotState::Existing(file) => (
            Some(file.id),
            u64::try_from(file.bytes.len()).map_err(|_| limit_overflow("snapshot_bytes"))?,
            Some(file.digest),
        ),
        SnapshotState::Absent(_) => (None, 0, None),
    };
    let output = edits.entry(path.clone()).or_insert_with(|| OutputEdits {
        path: path.clone(),
        existing_file_id,
        original_length,
        original_digest,
        deletions: Vec::new(),
        insertions: Vec::new(),
    });
    if output.existing_file_id != existing_file_id {
        return Err(CoreError::EditConflict {
            reason: "path_used_as_absent_and_existing",
            operation_index: None,
        });
    }
    Ok(output)
}

fn validate_edit_conflicts(edits: &OutputEdits) -> Result<(), CoreError> {
    let mut deletions = edits.deletions.clone();
    deletions.sort_by_key(|(operation_index, range)| (range.start, range.end, *operation_index));
    for adjacent in deletions.windows(2) {
        let (_, previous) = adjacent[0];
        let (operation_index, current) = adjacent[1];
        if current.start < previous.end {
            return Err(conflict("overlapping_move_deletions", operation_index));
        }
    }
    for (offset, insertion) in &edits.insertions {
        if deletions
            .iter()
            .any(|(_, range)| range.start < *offset && *offset < range.end)
        {
            return Err(conflict(
                "insertion_inside_move_deletion",
                insertion.operation_index,
            ));
        }
    }
    Ok(())
}

fn build_segments(edits: &OutputEdits) -> Result<Vec<OutputSegment>, CoreError> {
    let mut points = BTreeMap::<u64, PointEvents>::new();
    for (_, range) in &edits.deletions {
        points.entry(range.start).or_default().deletion_start = true;
        points.entry(range.end).or_default().deletion_end = true;
    }
    for (offset, insertion) in &edits.insertions {
        points
            .entry(*offset)
            .or_default()
            .insertions
            .push(*insertion);
    }
    for point in points.values_mut() {
        point
            .insertions
            .sort_by_key(|insertion| insertion.operation_index);
    }

    let mut segments = Vec::new();
    let mut cursor = 0;
    let mut deleting = false;
    for (offset, point) in points {
        if offset > edits.original_length {
            return Err(CoreError::InvalidDomainValue {
                field: "event_offset",
            });
        }
        if cursor < offset && !deleting {
            let snapshot_file_id = edits
                .existing_file_id
                .ok_or(CoreError::InvalidDomainValue {
                    field: "absent_output_original_slice",
                })?;
            segments.push(OutputSegment::OriginalSlice {
                snapshot_file_id,
                range: ByteRange {
                    start: cursor,
                    end: offset,
                },
            });
        }
        if point.deletion_end {
            deleting = false;
        }
        segments.extend(point.insertions.into_iter().map(|insertion| {
            OutputSegment::PayloadSlice {
                operation_index: insertion.operation_index,
                snapshot_file_id: insertion.snapshot_file_id,
                range: insertion.range,
                payload_digest: insertion.payload_digest,
            }
        }));
        if point.deletion_start {
            deleting = true;
        }
        cursor = offset;
    }
    if cursor < edits.original_length && !deleting {
        let snapshot_file_id = edits
            .existing_file_id
            .ok_or(CoreError::InvalidDomainValue {
                field: "absent_output_original_slice",
            })?;
        segments.push(OutputSegment::OriginalSlice {
            snapshot_file_id,
            range: ByteRange {
                start: cursor,
                end: edits.original_length,
            },
        });
    }
    Ok(segments)
}

fn stream_output(
    segments: &[OutputSegment],
    files_by_id: &HashMap<SnapshotFileId, &FileSnapshot>,
    original_file_id: Option<SnapshotFileId>,
) -> Result<(u64, Sha256Digest, bool), CoreError> {
    let original = original_file_id
        .map(|id| {
            files_by_id.get(&id).map(|file| file.bytes.as_ref()).ok_or(
                CoreError::InvalidDomainValue {
                    field: "output_snapshot_file_id",
                },
            )
        })
        .transpose()?;
    let mut hasher = Sha256::new();
    let mut resulting_length = 0_u64;
    let mut comparison_offset = 0_usize;
    let mut equals_original = original.is_some();

    for segment in segments {
        let (file_id, range) = match segment {
            OutputSegment::OriginalSlice {
                snapshot_file_id,
                range,
            }
            | OutputSegment::PayloadSlice {
                snapshot_file_id,
                range,
                ..
            } => (*snapshot_file_id, *range),
        };
        let file = files_by_id
            .get(&file_id)
            .copied()
            .ok_or(CoreError::InvalidDomainValue {
                field: "segment_snapshot_file_id",
            })?;
        let bytes = slice_range(&file.bytes, range)?;
        let segment_length =
            u64::try_from(bytes.len()).map_err(|_| limit_overflow("planned_output_bytes"))?;
        resulting_length = resulting_length
            .checked_add(segment_length)
            .ok_or_else(|| limit_overflow("planned_output_bytes"))?;
        hasher.update(bytes);

        if let Some(original) = original {
            let comparison_end = comparison_offset.saturating_add(bytes.len());
            if comparison_end > original.len()
                || original.get(comparison_offset..comparison_end) != Some(bytes)
            {
                equals_original = false;
            }
            comparison_offset = comparison_end;
        }
    }
    if original.is_some_and(|bytes| comparison_offset != bytes.len()) {
        equals_original = false;
    }
    let digest: [u8; 32] = hasher.finalize().into();
    Ok((resulting_length, Sha256Digest(digest), equals_original))
}

fn classify_output(existing: bool, resulting_length: u64, equals_original: bool) -> OutputChange {
    if !existing {
        OutputChange::CreatedNew
    } else if equals_original {
        OutputChange::Unchanged
    } else if resulting_length == 0 {
        OutputChange::EmptiedExisting
    } else {
        OutputChange::ModifiedExisting
    }
}

fn calculate_usage(
    operations: &[ResolvedOperation],
    outputs: &[PlannedOutput],
) -> Result<PlanningUsage, CoreError> {
    let mut usage = PlanningUsage {
        planning_memory_bytes: checked_mul(
            to_u64(operations.len(), "planning_memory_bytes")?,
            to_u64(size_of::<ResolvedOperation>(), "planning_memory_bytes")?,
            "planning_memory_bytes",
        )?,
        projected_response_bytes: 1_024,
        ..PlanningUsage::default()
    };

    for operation in operations {
        let path_bytes = checked_add(
            to_u64(
                operation.source_path.value.len(),
                "projected_response_bytes",
            )?,
            to_u64(
                operation.destination_path.value.len(),
                "projected_response_bytes",
            )?,
            "projected_response_bytes",
        )?;
        usage.projected_response_bytes = checked_add(
            usage.projected_response_bytes,
            checked_add(1_024, path_bytes, "projected_response_bytes")?,
            "projected_response_bytes",
        )?;
        usage.planning_memory_bytes = checked_add(
            usage.planning_memory_bytes,
            path_bytes,
            "planning_memory_bytes",
        )?;
    }

    usage.planning_memory_bytes = checked_add(
        usage.planning_memory_bytes,
        checked_mul(
            to_u64(outputs.len(), "planning_memory_bytes")?,
            to_u64(size_of::<PlannedOutput>(), "planning_memory_bytes")?,
            "planning_memory_bytes",
        )?,
        "planning_memory_bytes",
    )?;
    for output in outputs {
        let segment_count = to_u64(output.segments.len(), "segments")?;
        usage.maximum_output_bytes = usage.maximum_output_bytes.max(output.resulting_length);
        usage.total_output_bytes = checked_add(
            usage.total_output_bytes,
            output.resulting_length,
            "planned_output_bytes",
        )?;
        usage.maximum_output_segments = usage.maximum_output_segments.max(segment_count);
        usage.total_segments = checked_add(usage.total_segments, segment_count, "segments")?;
        if output.change != OutputChange::Unchanged {
            usage.changed_targets = checked_add(usage.changed_targets, 1, "changed_targets")?;
        }

        let path_bytes = to_u64(output.path.value.len(), "projected_response_bytes")?;
        let segment_projection = checked_mul(segment_count, 512, "projected_response_bytes")?;
        usage.projected_response_bytes = checked_add(
            usage.projected_response_bytes,
            checked_add(
                checked_add(512, path_bytes, "projected_response_bytes")?,
                segment_projection,
                "projected_response_bytes",
            )?,
            "projected_response_bytes",
        )?;
        usage.planning_memory_bytes = checked_add(
            usage.planning_memory_bytes,
            checked_add(
                path_bytes,
                checked_mul(
                    segment_count,
                    to_u64(size_of::<OutputSegment>(), "planning_memory_bytes")?,
                    "planning_memory_bytes",
                )?,
                "planning_memory_bytes",
            )?,
            "planning_memory_bytes",
        )?;
    }
    Ok(usage)
}

fn enforce_usage(usage: PlanningUsage, budget: ResourceBudget) -> Result<(), CoreError> {
    enforce_limit(
        "resulting_bytes_per_output",
        usage.maximum_output_bytes,
        budget.resulting_bytes_per_output,
    )?;
    enforce_limit(
        "planned_output_bytes",
        usage.total_output_bytes,
        budget.planned_output_bytes,
    )?;
    enforce_limit(
        "segments_per_output",
        usage.maximum_output_segments,
        budget.segments_per_output,
    )?;
    enforce_limit("segments", usage.total_segments, budget.segments)?;
    enforce_limit(
        "changed_targets",
        usage.changed_targets,
        budget.changed_targets,
    )?;
    enforce_limit(
        "projected_response_bytes",
        usage.projected_response_bytes,
        budget.projected_response_bytes,
    )?;
    enforce_limit(
        "planning_memory_bytes",
        usage.planning_memory_bytes,
        budget.planning_memory_bytes,
    )
}

fn enforce_limit(resource: &'static str, actual: u64, limit: u64) -> Result<(), CoreError> {
    if actual > limit {
        Err(CoreError::ResourceLimitExceeded {
            resource,
            actual,
            limit,
        })
    } else {
        Ok(())
    }
}

fn slice_range(bytes: &[u8], range: ByteRange) -> Result<&[u8], CoreError> {
    let start = usize::try_from(range.start).map_err(|_| CoreError::InvalidDomainValue {
        field: "segment_range",
    })?;
    let end = usize::try_from(range.end).map_err(|_| CoreError::InvalidDomainValue {
        field: "segment_range",
    })?;
    bytes.get(start..end).ok_or(CoreError::InvalidDomainValue {
        field: "segment_range",
    })
}

fn checked_add(left: u64, right: u64, resource: &'static str) -> Result<u64, CoreError> {
    left.checked_add(right)
        .ok_or_else(|| limit_overflow(resource))
}

fn checked_mul(left: u64, right: u64, resource: &'static str) -> Result<u64, CoreError> {
    left.checked_mul(right)
        .ok_or_else(|| limit_overflow(resource))
}

fn to_u64(value: usize, resource: &'static str) -> Result<u64, CoreError> {
    u64::try_from(value).map_err(|_| limit_overflow(resource))
}

fn conflict(reason: &'static str, operation_index: u64) -> CoreError {
    CoreError::EditConflict {
        reason,
        operation_index: Some(operation_index),
    }
}

fn limit_overflow(resource: &'static str) -> CoreError {
    CoreError::ResourceLimitExceeded {
        resource,
        actual: u64::MAX,
        limit: u64::MAX - 1,
    }
}
