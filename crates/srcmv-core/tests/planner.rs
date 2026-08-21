//! Phase 4 planner fixtures and property tests.

use std::collections::HashMap;
use std::sync::Arc;

use proptest::prelude::*;
use sha2::{Digest, Sha256};
use srcmv_core::{
    AbsentPathSnapshot, Anchor, BatchSpecification, ByteRange, CoreError, Destination,
    FileIdentity, FileSnapshot, LineIndex, Operation, OperationEffect, OperationSpecification,
    OutputChange, OutputSegment, Precondition, ResourceBudget, Selector, Sha256Digest,
    SnapshotFileId, SourceSelection, WorkspaceRelativePath, WorkspaceSnapshot, encode_plan_record,
    plan,
};

fn path(value: &str) -> WorkspaceRelativePath {
    WorkspaceRelativePath {
        value: value.to_owned(),
    }
}

fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest(Sha256::digest(bytes).into())
}

fn file(id: u64, value: &str, bytes: &[u8]) -> FileSnapshot {
    FileSnapshot {
        id: SnapshotFileId(id),
        path: path(value),
        parent_identity: FileIdentity {
            device: 1,
            inode: 10 + id,
        },
        parent_identities: Arc::new([]),
        identity: FileIdentity {
            device: 1,
            inode: 100 + id,
        },
        link_count: 1,
        bytes: Arc::from(bytes),
        digest: digest(bytes),
        line_index: LineIndex::from_bytes_with_limits(bytes, u64::MAX, u64::MAX)
            .expect("fixture line index should build"),
    }
}

fn absent(value: &str) -> AbsentPathSnapshot {
    AbsentPathSnapshot {
        path: path(value),
        parent_identity: FileIdentity {
            device: 1,
            inode: 9,
        },
        parent_identities: Arc::new([]),
        basename: value.to_owned(),
    }
}

fn snapshot(files: Vec<FileSnapshot>, absent_paths: Vec<AbsentPathSnapshot>) -> WorkspaceSnapshot {
    WorkspaceSnapshot {
        workspace_identity: FileIdentity {
            device: 1,
            inode: 1,
        },
        files: files.into(),
        absent_paths: absent_paths.into(),
    }
}

fn operation(
    kind: &str,
    source: &FileSnapshot,
    selector: Selector,
    destination_path: &str,
    anchor: Anchor,
    destination_precondition: Precondition,
) -> Operation {
    let specification = OperationSpecification {
        source: SourceSelection {
            path: source.path.clone(),
            selector,
            precondition: Precondition::Sha256(source.digest),
        },
        destination: Destination {
            path: path(destination_path),
            anchor,
            precondition: destination_precondition,
        },
    };
    match kind {
        "move" => Operation::Move(specification),
        "copy" => Operation::Copy(specification),
        _ => panic!("unknown fixture operation kind"),
    }
}

fn batch(operations: Vec<Operation>) -> BatchSpecification {
    BatchSpecification {
        operations: operations.into(),
    }
}

fn render(plan: &srcmv_core::EditPlan, snapshot: &WorkspaceSnapshot, output: &str) -> Vec<u8> {
    let files = snapshot
        .files
        .iter()
        .map(|file| (file.id, file))
        .collect::<HashMap<_, _>>();
    let output = plan
        .outputs
        .iter()
        .find(|planned| planned.path.value == output)
        .expect("expected planned output");
    let mut bytes = Vec::new();
    for segment in output.segments.iter() {
        let (file_id, ByteRange { start, end }) = match segment {
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
        let file = files.get(&file_id).expect("segment file should exist");
        bytes.extend_from_slice(&file.bytes[start as usize..end as usize]);
    }
    bytes
}

#[test]
fn planner_resolves_same_file_forward_and_backward_moves() {
    let forward_file = file(0, "forward", b"abcdef");
    let backward_file = file(1, "backward", b"abcdef");
    let workspace = snapshot(vec![forward_file.clone(), backward_file.clone()], vec![]);
    let request = batch(vec![
        operation(
            "move",
            &forward_file,
            Selector::Bytes { start: 1, end: 3 },
            "forward",
            Anchor::FileEnd,
            Precondition::Sha256(forward_file.digest),
        ),
        operation(
            "move",
            &backward_file,
            Selector::Bytes { start: 3, end: 5 },
            "backward",
            Anchor::FileStart,
            Precondition::Sha256(backward_file.digest),
        ),
    ]);

    let result = plan(&workspace, &request, ResourceBudget::default()).expect("moves should plan");

    assert_eq!(render(&result, &workspace, "forward"), b"adefbc");
    assert_eq!(render(&result, &workspace, "backward"), b"deabcf");
}

#[test]
fn planner_reports_start_and_end_anchored_same_file_moves_as_no_ops() {
    let source = file(0, "same", b"abcdef");
    let workspace = snapshot(vec![source.clone()], vec![]);
    let request = batch(vec![
        operation(
            "move",
            &source,
            Selector::Bytes { start: 1, end: 3 },
            "same",
            Anchor::ByteOffset(1),
            Precondition::Sha256(source.digest),
        ),
        operation(
            "move",
            &source,
            Selector::Bytes { start: 3, end: 5 },
            "same",
            Anchor::ByteOffset(5),
            Precondition::Sha256(source.digest),
        ),
    ]);

    let result = plan(&workspace, &request, ResourceBudget::default()).expect("no-ops should plan");

    assert!(result.outputs.is_empty());
    assert!(
        result
            .operations
            .iter()
            .all(|operation| operation.effect == OperationEffect::NoOp)
    );
}

#[test]
fn planner_composes_cross_file_move_and_copy_from_initial_bytes() {
    let moved = file(0, "moved", b"abc");
    let copied = file(1, "copied", b"123");
    let destination = file(2, "destination", b"XY");
    let workspace = snapshot(
        vec![moved.clone(), copied.clone(), destination.clone()],
        vec![],
    );
    let request = batch(vec![
        operation(
            "move",
            &moved,
            Selector::Bytes { start: 1, end: 3 },
            "destination",
            Anchor::FileEnd,
            Precondition::Sha256(destination.digest),
        ),
        operation(
            "copy",
            &copied,
            Selector::Bytes { start: 0, end: 2 },
            "destination",
            Anchor::FileStart,
            Precondition::Sha256(destination.digest),
        ),
    ]);

    let result = plan(&workspace, &request, ResourceBudget::default()).expect("batch should plan");

    assert_eq!(render(&result, &workspace, "moved"), b"a");
    assert_eq!(render(&result, &workspace, "destination"), b"12XYbc");
    assert!(
        result
            .outputs
            .iter()
            .all(|output| output.path.value != "copied")
    );
}

#[test]
fn planner_accepts_overlapping_copies_but_rejects_overlapping_move_deletions() {
    let source = file(0, "source", b"abcdef");
    let destination = file(1, "destination", b"");
    let workspace = snapshot(vec![source.clone(), destination.clone()], vec![]);
    let copies = batch(vec![
        operation(
            "copy",
            &source,
            Selector::Bytes { start: 0, end: 4 },
            "destination",
            Anchor::FileStart,
            Precondition::Sha256(destination.digest),
        ),
        operation(
            "copy",
            &source,
            Selector::Bytes { start: 2, end: 6 },
            "destination",
            Anchor::FileStart,
            Precondition::Sha256(destination.digest),
        ),
    ]);
    let moves = batch(vec![
        operation(
            "move",
            &source,
            Selector::Bytes { start: 0, end: 4 },
            "destination",
            Anchor::FileStart,
            Precondition::Sha256(destination.digest),
        ),
        operation(
            "move",
            &source,
            Selector::Bytes { start: 2, end: 6 },
            "destination",
            Anchor::FileStart,
            Precondition::Sha256(destination.digest),
        ),
    ]);

    let copy_plan = plan(&workspace, &copies, ResourceBudget::default())
        .expect("overlapping copies should plan");
    let move_error = plan(&workspace, &moves, ResourceBudget::default())
        .expect_err("overlapping moves should conflict");

    assert_eq!(render(&copy_plan, &workspace, "destination"), b"abcdcdef");
    assert!(matches!(
        move_error,
        CoreError::EditConflict {
            reason: "overlapping_move_deletions",
            ..
        }
    ));
}

#[test]
fn planner_accepts_insertions_at_deletion_boundaries_and_rejects_inside() {
    let source = file(0, "source", b"abcdef");
    let payload = file(1, "payload", b"XY");
    let workspace = snapshot(vec![source.clone(), payload.clone()], vec![]);
    let boundaries = batch(vec![
        operation(
            "move",
            &source,
            Selector::Bytes { start: 2, end: 4 },
            "source",
            Anchor::FileEnd,
            Precondition::Sha256(source.digest),
        ),
        operation(
            "copy",
            &payload,
            Selector::Bytes { start: 0, end: 1 },
            "source",
            Anchor::ByteOffset(2),
            Precondition::Sha256(source.digest),
        ),
        operation(
            "copy",
            &payload,
            Selector::Bytes { start: 1, end: 2 },
            "source",
            Anchor::ByteOffset(4),
            Precondition::Sha256(source.digest),
        ),
    ]);
    let inside = batch(vec![
        operation(
            "move",
            &source,
            Selector::Bytes { start: 2, end: 4 },
            "source",
            Anchor::FileEnd,
            Precondition::Sha256(source.digest),
        ),
        operation(
            "copy",
            &payload,
            Selector::Bytes { start: 0, end: 1 },
            "source",
            Anchor::ByteOffset(3),
            Precondition::Sha256(source.digest),
        ),
    ]);

    let boundary_plan = plan(&workspace, &boundaries, ResourceBudget::default())
        .expect("boundary insertions should plan");
    let inside_error = plan(&workspace, &inside, ResourceBudget::default())
        .expect_err("interior insertion should conflict");

    assert_eq!(render(&boundary_plan, &workspace, "source"), b"abXYefcd");
    assert!(matches!(
        inside_error,
        CoreError::EditConflict {
            reason: "insertion_inside_move_deletion",
            ..
        }
    ));
}

#[test]
fn planner_handles_adjacent_deletions_repeated_offsets_eof_and_whole_file_move() {
    let source = file(0, "source", b"abcdef");
    let payload = file(1, "payload", b"12");
    let workspace = snapshot(vec![source.clone(), payload.clone()], vec![absent("new")]);
    let request = batch(vec![
        operation(
            "move",
            &source,
            Selector::Bytes { start: 0, end: 3 },
            "new",
            Anchor::FileStart,
            Precondition::MustNotExist,
        ),
        operation(
            "move",
            &source,
            Selector::Bytes { start: 3, end: 6 },
            "new",
            Anchor::FileEnd,
            Precondition::MustNotExist,
        ),
        operation(
            "copy",
            &payload,
            Selector::Bytes { start: 0, end: 1 },
            "new",
            Anchor::ByteOffset(0),
            Precondition::MustNotExist,
        ),
        operation(
            "copy",
            &payload,
            Selector::Bytes { start: 1, end: 2 },
            "new",
            Anchor::ByteOffset(0),
            Precondition::MustNotExist,
        ),
    ]);

    let result = plan(&workspace, &request, ResourceBudget::default()).expect("batch should plan");

    assert_eq!(render(&result, &workspace, "source"), b"");
    assert_eq!(render(&result, &workspace, "new"), b"abcdef12");
    assert_eq!(
        result
            .outputs
            .iter()
            .find(|output| output.path.value == "source")
            .expect("source output")
            .change,
        OutputChange::EmptiedExisting
    );
}

#[test]
fn planner_copies_bytes_that_another_move_removes() {
    let source = file(0, "source", b"abcdef");
    let moved_to = file(1, "moved-to", b"");
    let copied_to = file(2, "copied-to", b"");
    let workspace = snapshot(
        vec![source.clone(), moved_to.clone(), copied_to.clone()],
        vec![],
    );
    let request = batch(vec![
        operation(
            "move",
            &source,
            Selector::Bytes { start: 1, end: 5 },
            "moved-to",
            Anchor::FileStart,
            Precondition::Sha256(moved_to.digest),
        ),
        operation(
            "copy",
            &source,
            Selector::Bytes { start: 2, end: 4 },
            "copied-to",
            Anchor::FileStart,
            Precondition::Sha256(copied_to.digest),
        ),
    ]);

    let result = plan(&workspace, &request, ResourceBudget::default()).expect("batch should plan");

    assert_eq!(render(&result, &workspace, "source"), b"af");
    assert_eq!(render(&result, &workspace, "moved-to"), b"bcde");
    assert_eq!(render(&result, &workspace, "copied-to"), b"cd");
}

#[test]
fn planner_orders_multiple_new_file_insertions_by_request_index() {
    let first = file(0, "first", b"A");
    let second = file(1, "second", b"B");
    let workspace = snapshot(vec![first.clone(), second.clone()], vec![absent("new")]);
    let request = batch(vec![
        operation(
            "copy",
            &first,
            Selector::Bytes { start: 0, end: 1 },
            "new",
            Anchor::FileStart,
            Precondition::MustNotExist,
        ),
        operation(
            "copy",
            &second,
            Selector::Bytes { start: 0, end: 1 },
            "new",
            Anchor::FileEnd,
            Precondition::MustNotExist,
        ),
    ]);

    let result = plan(&workspace, &request, ResourceBudget::default()).expect("copies should plan");

    assert_eq!(render(&result, &workspace, "new"), b"AB");
}

#[test]
fn planner_classifies_effectful_byte_identical_output_as_unchanged() {
    let original = file(0, "original", b"abc");
    let replacement = file(1, "replacement", b"abc");
    let destination = file(2, "destination", b"");
    let workspace = snapshot(
        vec![original.clone(), replacement.clone(), destination.clone()],
        vec![],
    );
    let request = batch(vec![
        operation(
            "move",
            &original,
            Selector::Bytes { start: 0, end: 3 },
            "destination",
            Anchor::FileStart,
            Precondition::Sha256(destination.digest),
        ),
        operation(
            "copy",
            &replacement,
            Selector::Bytes { start: 0, end: 3 },
            "original",
            Anchor::FileStart,
            Precondition::Sha256(original.digest),
        ),
    ]);

    let result = plan(&workspace, &request, ResourceBudget::default()).expect("batch should plan");
    let original_output = result
        .outputs
        .iter()
        .find(|output| output.path.value == "original")
        .expect("effectful output should remain represented");

    assert_eq!(original_output.change, OutputChange::Unchanged);
    assert_eq!(result.usage.changed_targets, 1);
}

#[test]
fn planner_rejects_only_non_unchanged_hard_link_outputs() {
    let mut unchanged = file(0, "unchanged", b"abc");
    unchanged.link_count = 2;
    let replacement = file(1, "replacement", b"abc");
    let destination = file(2, "destination", b"");
    let workspace = snapshot(
        vec![unchanged.clone(), replacement.clone(), destination.clone()],
        vec![],
    );
    let unchanged_request = batch(vec![
        operation(
            "move",
            &unchanged,
            Selector::Bytes { start: 0, end: 3 },
            "destination",
            Anchor::FileStart,
            Precondition::Sha256(destination.digest),
        ),
        operation(
            "copy",
            &replacement,
            Selector::Bytes { start: 0, end: 3 },
            "unchanged",
            Anchor::FileStart,
            Precondition::Sha256(unchanged.digest),
        ),
    ]);
    let changing_request = batch(vec![operation(
        "copy",
        &replacement,
        Selector::Bytes { start: 0, end: 1 },
        "unchanged",
        Anchor::FileStart,
        Precondition::Sha256(unchanged.digest),
    )]);

    plan(&workspace, &unchanged_request, ResourceBudget::default())
        .expect("byte-identical hard-link output should be allowed");
    let error = plan(&workspace, &changing_request, ResourceBudget::default())
        .expect_err("changing hard-link output should fail");

    assert!(matches!(error, CoreError::HardLinkNotSupported { .. }));
}

#[test]
fn planner_resolves_line_and_byte_coordinate_boundaries() {
    let source = file(0, "source", b"a\r\nb\rc\nlast");
    let new = absent("new");
    let workspace = snapshot(vec![source.clone()], vec![new]);
    let request = batch(vec![operation(
        "copy",
        &source,
        Selector::Lines { start: 2, end: 3 },
        "new",
        Anchor::ByteOffset(0),
        Precondition::MustNotExist,
    )]);

    let result =
        plan(&workspace, &request, ResourceBudget::default()).expect("lines should resolve");

    assert_eq!(
        result.operations[0].source_range,
        ByteRange { start: 3, end: 7 }
    );
    assert_eq!(render(&result, &workspace, "new"), b"b\rc\n");
}

#[test]
fn planner_rejects_out_of_range_coordinates_and_incompatible_preconditions() {
    let source = file(0, "source", b"a\nb\n");
    let destination = file(1, "destination", b"XY");
    let workspace = snapshot(
        vec![source.clone(), destination.clone()],
        vec![absent("new")],
    );
    let cases = vec![
        (
            batch(vec![operation(
                "copy",
                &source,
                Selector::Bytes { start: 0, end: 99 },
                "destination",
                Anchor::FileStart,
                Precondition::Sha256(destination.digest),
            )]),
            "byte_selector_out_of_range",
        ),
        (
            batch(vec![operation(
                "copy",
                &source,
                Selector::Lines { start: 0, end: 1 },
                "destination",
                Anchor::FileStart,
                Precondition::Sha256(destination.digest),
            )]),
            "line_selector_out_of_range",
        ),
        (
            batch(vec![operation(
                "copy",
                &source,
                Selector::Bytes { start: 0, end: 1 },
                "destination",
                Anchor::ByteOffset(3),
                Precondition::Sha256(destination.digest),
            )]),
            "byte_anchor_out_of_range",
        ),
        (
            batch(vec![operation(
                "copy",
                &source,
                Selector::Bytes { start: 0, end: 1 },
                "new",
                Anchor::BeforeLine(1),
                Precondition::MustNotExist,
            )]),
            "new_file_anchor_out_of_range",
        ),
        (
            batch(vec![operation(
                "move",
                &source,
                Selector::Bytes { start: 0, end: 3 },
                "source",
                Anchor::ByteOffset(1),
                Precondition::Sha256(source.digest),
            )]),
            "insertion_inside_move_deletion",
        ),
        (
            batch(vec![
                operation(
                    "copy",
                    &source,
                    Selector::Bytes { start: 0, end: 1 },
                    "destination",
                    Anchor::FileStart,
                    Precondition::Sha256(destination.digest),
                ),
                operation(
                    "copy",
                    &source,
                    Selector::Bytes { start: 1, end: 2 },
                    "destination",
                    Anchor::FileEnd,
                    Precondition::MustNotExist,
                ),
            ]),
            "incompatible_preconditions",
        ),
    ];

    for (request, expected_reason) in cases {
        let error = plan(&workspace, &request, ResourceBudget::default())
            .expect_err("invalid planning case should conflict");
        assert!(matches!(
            error,
            CoreError::EditConflict { reason, .. } if reason == expected_reason
        ));
    }
}

#[test]
fn planner_rejects_snapshot_path_marked_existing_and_absent() {
    let source = file(0, "source", b"a");
    let workspace = snapshot(vec![source.clone()], vec![absent("source")]);
    let request = batch(vec![operation(
        "copy",
        &source,
        Selector::Bytes { start: 0, end: 1 },
        "source",
        Anchor::FileStart,
        Precondition::Sha256(source.digest),
    )]);

    let error = plan(&workspace, &request, ResourceBudget::default())
        .expect_err("mixed existence state should conflict");

    assert!(matches!(
        error,
        CoreError::EditConflict {
            reason: "path_used_as_absent_and_existing",
            ..
        }
    ));
}

#[test]
fn planner_enforces_every_phase_four_resource_boundary() {
    let source = file(0, "source", b"abcd");
    let destination = file(1, "destination", b"XY");
    let workspace = snapshot(vec![source.clone(), destination.clone()], vec![]);
    let request = batch(vec![operation(
        "move",
        &source,
        Selector::Bytes { start: 1, end: 3 },
        "destination",
        Anchor::FileEnd,
        Precondition::Sha256(destination.digest),
    )]);
    let baseline = plan(&workspace, &request, ResourceBudget::default())
        .expect("baseline plan should fit defaults");
    let usage = baseline.usage;
    let cases = [
        ("resulting_bytes_per_output", usage.maximum_output_bytes),
        ("planned_output_bytes", usage.total_output_bytes),
        ("segments_per_output", usage.maximum_output_segments),
        ("segments", usage.total_segments),
        ("changed_targets", usage.changed_targets),
        ("projected_response_bytes", usage.projected_response_bytes),
        ("planning_memory_bytes", usage.planning_memory_bytes),
    ];

    for (resource, exact) in cases {
        let mut exact_budget = ResourceBudget::default();
        set_limit(&mut exact_budget, resource, exact);
        plan(&workspace, &request, exact_budget).expect("exact boundary should pass");

        let mut below_budget = ResourceBudget::default();
        set_limit(&mut below_budget, resource, exact - 1);
        let error = plan(&workspace, &request, below_budget)
            .expect_err("one below actual usage should fail");
        assert!(matches!(
            error,
            CoreError::ResourceLimitExceeded {
                resource: found,
                actual,
                limit,
            } if found == resource && actual == exact && limit == exact - 1
        ));
    }
}

fn set_limit(budget: &mut ResourceBudget, resource: &str, value: u64) {
    match resource {
        "resulting_bytes_per_output" => budget.resulting_bytes_per_output = value,
        "planned_output_bytes" => budget.planned_output_bytes = value,
        "segments_per_output" => budget.segments_per_output = value,
        "segments" => budget.segments = value,
        "changed_targets" => budget.changed_targets = value,
        "projected_response_bytes" => budget.projected_response_bytes = value,
        "planning_memory_bytes" => budget.planning_memory_bytes = value,
        _ => panic!("unknown resource fixture"),
    }
}

proptest! {
    #[test]
    fn planner_property_preserves_exact_move_or_copy_payload_and_digest(
        source_bytes in prop::collection::vec(any::<u8>(), 1..64),
        destination_bytes in prop::collection::vec(any::<u8>(), 0..64),
        is_move in any::<bool>(),
        raw_start in any::<usize>(),
        raw_length in any::<usize>(),
        raw_offset in any::<usize>(),
    ) {
        let start = raw_start % source_bytes.len();
        let length = 1 + raw_length % (source_bytes.len() - start);
        let end = start + length;
        let offset = raw_offset % (destination_bytes.len() + 1);
        let source = file(0, "source", &source_bytes);
        let destination = file(1, "destination", &destination_bytes);
        let workspace = snapshot(vec![source.clone(), destination.clone()], vec![]);
        let request = batch(vec![operation(
            if is_move { "move" } else { "copy" },
            &source,
            Selector::Bytes { start: start as u64, end: end as u64 },
            "destination",
            Anchor::ByteOffset(offset as u64),
            Precondition::Sha256(destination.digest),
        )]);

        let first = plan(&workspace, &request, ResourceBudget::default())
            .expect("generated copy should plan");
        let second = plan(&workspace, &request, ResourceBudget::default())
            .expect("same generated copy should plan again");
        let first_cbor = encode_plan_record(&workspace, &first)
            .expect("generated plan should encode");
        let second_cbor = encode_plan_record(&workspace, &second)
            .expect("same generated plan should encode");
        let output = render(&first, &workspace, "destination");
        let expected = [&destination_bytes[..offset], &source_bytes[start..end], &destination_bytes[offset..]].concat();
        let payload = first.outputs.iter().flat_map(|output| output.segments.iter()).find_map(|segment| {
            if let OutputSegment::PayloadSlice { payload_digest, .. } = segment {
                Some(*payload_digest)
            } else {
                None
            }
        }).expect("copy should retain one payload segment");

        prop_assert_eq!(output, expected);
        prop_assert_eq!(payload, digest(&source_bytes[start..end]));
        prop_assert_eq!(&first, &second);
        prop_assert_eq!(&first_cbor, &second_cbor);
        prop_assert_eq!(strict_cbor_item_length(&first_cbor), Ok(first_cbor.len()));
        if is_move {
            let source_output = render(&first, &workspace, "source");
            let expected_source = [&source_bytes[..start], &source_bytes[end..]].concat();
            prop_assert_eq!(source_output, expected_source);
        }
    }

    #[test]
    fn line_index_fuzz_property_matches_a_reference_model(
        bytes in prop::collection::vec(any::<u8>(), 0..512),
    ) {
        let index = LineIndex::from_bytes_with_limits(&bytes, u64::MAX, u64::MAX)
            .expect("bounded generated input should index");
        let expected = reference_line_boundaries(&bytes);

        prop_assert_eq!(index.line_count(), expected.len() as u64);
        for (position, end) in expected.iter().copied().enumerate() {
            let line = position as u64 + 1;
            let start = if position == 0 { 0 } else { expected[position - 1] };
            prop_assert_eq!(index.line_start(line), Some(start));
            prop_assert_eq!(index.line_end(line), Some(end));
        }
        prop_assert_eq!(index.line_start(0), None);
        prop_assert_eq!(index.line_end(index.line_count().saturating_add(1)), None);
    }

    #[test]
    fn event_composition_fuzz_property_matches_request_order_at_shared_offsets(
        source_bytes in prop::collection::vec(any::<u8>(), 1..32),
        destination_bytes in prop::collection::vec(any::<u8>(), 0..32),
        raw_insertions in prop::collection::vec((any::<usize>(), any::<usize>()), 1..16),
    ) {
        let source = file(0, "source", &source_bytes);
        let destination = file(1, "destination", &destination_bytes);
        let workspace = snapshot(vec![source.clone(), destination.clone()], vec![]);
        let insertions = raw_insertions
            .iter()
            .map(|(raw_offset, raw_source)| {
                (raw_offset % (destination_bytes.len() + 1), raw_source % source_bytes.len())
            })
            .collect::<Vec<_>>();
        let request = batch(insertions.iter().map(|(offset, source_index)| {
            operation(
                "copy",
                &source,
                Selector::Bytes {
                    start: *source_index as u64,
                    end: *source_index as u64 + 1,
                },
                "destination",
                Anchor::ByteOffset(*offset as u64),
                Precondition::Sha256(destination.digest),
            )
        }).collect());

        let edit_plan = plan(&workspace, &request, ResourceBudget::default())
            .expect("generated insertions should plan");
        let actual = render(&edit_plan, &workspace, "destination");
        let mut expected = Vec::new();
        for offset in 0..=destination_bytes.len() {
            for (_, source_index) in insertions.iter().filter(|(candidate, _)| *candidate == offset) {
                expected.push(source_bytes[*source_index]);
            }
            if let Some(byte) = destination_bytes.get(offset) {
                expected.push(*byte);
            }
        }

        prop_assert_eq!(actual, expected);
    }

    #[test]
    fn deterministic_cbor_decoder_fuzz_regression_never_panics(
        bytes in prop::collection::vec(any::<u8>(), 0..1024),
    ) {
        let _ = strict_cbor_item_length(&bytes);
    }
}

fn reference_line_boundaries(bytes: &[u8]) -> Vec<u64> {
    let mut boundaries = Vec::new();
    let mut offset = 0;
    while offset < bytes.len() {
        match bytes[offset] {
            b'\r' if bytes.get(offset + 1) == Some(&b'\n') => {
                offset += 2;
                boundaries.push(offset as u64);
            }
            b'\r' | b'\n' => {
                offset += 1;
                boundaries.push(offset as u64);
            }
            _ => offset += 1,
        }
    }
    if !bytes.is_empty() && boundaries.last().copied() != Some(bytes.len() as u64) {
        boundaries.push(bytes.len() as u64);
    }
    boundaries
}

fn strict_cbor_item_length(bytes: &[u8]) -> Result<usize, ()> {
    let consumed = parse_cbor_item(bytes, 0)?;
    if consumed == bytes.len() {
        Ok(consumed)
    } else {
        Err(())
    }
}

fn parse_cbor_item(bytes: &[u8], depth: usize) -> Result<usize, ()> {
    if depth > 64 {
        return Err(());
    }
    let initial = *bytes.first().ok_or(())?;
    let major = initial >> 5;
    let additional = initial & 0x1f;
    if additional == 31 || matches!(major, 1 | 5 | 6) {
        return Err(());
    }
    if major == 7 {
        return if initial == 0xf6 { Ok(1) } else { Err(()) };
    }
    let (value, header) = parse_cbor_argument(bytes, additional)?;
    match major {
        0 => Ok(header),
        2 | 3 => {
            let length = usize::try_from(value).map_err(|_| ())?;
            let end = header.checked_add(length).ok_or(())?;
            let payload = bytes.get(header..end).ok_or(())?;
            if major == 3 && std::str::from_utf8(payload).is_err() {
                return Err(());
            }
            Ok(end)
        }
        4 => {
            let mut consumed = header;
            for _ in 0..value {
                let length = parse_cbor_item(bytes.get(consumed..).ok_or(())?, depth + 1)?;
                consumed = consumed.checked_add(length).ok_or(())?;
            }
            Ok(consumed)
        }
        _ => Err(()),
    }
}

fn parse_cbor_argument(bytes: &[u8], additional: u8) -> Result<(u64, usize), ()> {
    match additional {
        value @ 0..=23 => Ok((u64::from(value), 1)),
        24 => {
            let value = u64::from(*bytes.get(1).ok_or(())?);
            if value < 24 { Err(()) } else { Ok((value, 2)) }
        }
        25 => {
            let raw: [u8; 2] = bytes.get(1..3).ok_or(())?.try_into().map_err(|_| ())?;
            let value = u64::from(u16::from_be_bytes(raw));
            if value <= 0xff {
                Err(())
            } else {
                Ok((value, 3))
            }
        }
        26 => {
            let raw: [u8; 4] = bytes.get(1..5).ok_or(())?.try_into().map_err(|_| ())?;
            let value = u64::from(u32::from_be_bytes(raw));
            if value <= 0xffff {
                Err(())
            } else {
                Ok((value, 5))
            }
        }
        27 => {
            let raw: [u8; 8] = bytes.get(1..9).ok_or(())?.try_into().map_err(|_| ())?;
            let value = u64::from_be_bytes(raw);
            if value <= 0xffff_ffff {
                Err(())
            } else {
                Ok((value, 9))
            }
        }
        _ => Err(()),
    }
}
