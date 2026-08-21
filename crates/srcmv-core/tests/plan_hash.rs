//! Deterministic CBOR plan-record golden tests.

use std::sync::Arc;

use srcmv_core::{
    AbsentPathSnapshot, Anchor, ByteRange, EditPlan, FileIdentity, FileSnapshot, LineIndex,
    OperationEffect, OperationKind, OutputChange, OutputSegment, PlanDigest, PlannedOutput,
    PlanningUsage, Precondition, ResolvedOperation, Selector, Sha256Digest, SnapshotFileId,
    WorkspaceRelativePath, WorkspaceSnapshot, encode_plan_record, plan_digest,
};
use sha2::{Digest, Sha256};

const GOLDEN_HEX: &str =
    include_str!("../../../tests/golden/plan-hash-v1/all-discriminants.cbor.hex");
const GOLDEN_DIGEST: &str =
    include_str!("../../../tests/golden/plan-hash-v1/all-discriminants.sha256");

fn path(value: &str) -> WorkspaceRelativePath {
    WorkspaceRelativePath {
        value: value.to_owned(),
    }
}

fn digest(bytes: &[u8]) -> Sha256Digest {
    Sha256Digest(Sha256::digest(bytes).into())
}

fn fixture() -> (WorkspaceSnapshot, EditPlan) {
    let bytes: Arc<[u8]> = Arc::from([0xff, 0x00, 0x80]);
    let file_digest = digest(&bytes);
    let file = FileSnapshot {
        id: SnapshotFileId(7),
        path: path("z.bin"),
        parent_identity: FileIdentity {
            device: u64::MAX,
            inode: 24,
        },
        parent_identities: Arc::new([]),
        identity: FileIdentity {
            device: 256,
            inode: u32::MAX as u64 + 1,
        },
        link_count: 1,
        bytes: Arc::clone(&bytes),
        digest: file_digest,
        line_index: LineIndex::from_bytes_with_limits(&bytes, u64::MAX, u64::MAX)
            .expect("golden line index should build"),
    };
    let absent = AbsentPathSnapshot {
        path: path("a.new"),
        parent_identity: FileIdentity {
            device: 23,
            inode: 255,
        },
        parent_identities: Arc::new([]),
        basename: "a.new".to_owned(),
    };
    let snapshot = WorkspaceSnapshot {
        workspace_identity: FileIdentity {
            device: 65_535,
            inode: u64::MAX,
        },
        files: Arc::from([file]),
        absent_paths: Arc::from([absent]),
    };

    let anchors = [
        Anchor::FileStart,
        Anchor::FileEnd,
        Anchor::BeforeLine(1),
        Anchor::AfterLine(1),
        Anchor::ByteOffset(u64::MAX),
    ];
    let operations = anchors
        .into_iter()
        .enumerate()
        .map(|(index, anchor)| ResolvedOperation {
            operation_index: if index == 4 { u64::MAX } else { index as u64 },
            kind: if index % 2 == 0 {
                OperationKind::Move
            } else {
                OperationKind::Copy
            },
            source_path: path("z.bin"),
            selector: if index == 0 {
                Selector::Lines { start: 1, end: 1 }
            } else {
                Selector::Bytes {
                    start: index as u64,
                    end: if index == 4 {
                        u64::MAX
                    } else {
                        index as u64 + 1
                    },
                }
            },
            source_precondition: Precondition::Sha256(file_digest),
            source_range: ByteRange {
                start: index as u64,
                end: if index == 4 {
                    u64::MAX
                } else {
                    index as u64 + 1
                },
            },
            selected_digest: Sha256Digest([index as u8; 32]),
            destination_path: path(if index == 3 { "z.bin" } else { "a.new" }),
            anchor,
            destination_precondition: if index == 0 {
                Precondition::MustNotExist
            } else {
                Precondition::Sha256(file_digest)
            },
            destination_offset: if index == 4 {
                u64::MAX
            } else if index == 1 {
                0
            } else {
                index as u64
            },
            effect: if index == 3 {
                OperationEffect::NoOp
            } else {
                OperationEffect::Changed
            },
        })
        .collect::<Vec<_>>();

    let changes = [
        OutputChange::Unchanged,
        OutputChange::ModifiedExisting,
        OutputChange::CreatedNew,
        OutputChange::EmptiedExisting,
    ];
    let outputs = changes
        .into_iter()
        .enumerate()
        .map(|(index, change)| PlannedOutput {
            path: path(&format!("output-{index}")),
            original_digest: (index != 2).then_some(file_digest),
            change,
            resulting_length: if index == 3 { 0 } else { index as u64 + 1 },
            resulting_digest: Sha256Digest([0xa0 + index as u8; 32]),
            segments: if index == 0 {
                Arc::from([
                    OutputSegment::OriginalSlice {
                        snapshot_file_id: SnapshotFileId(7),
                        range: ByteRange { start: 0, end: 1 },
                    },
                    OutputSegment::PayloadSlice {
                        operation_index: u64::MAX,
                        snapshot_file_id: SnapshotFileId(7),
                        range: ByteRange {
                            start: 1,
                            end: u64::MAX,
                        },
                        payload_digest: Sha256Digest([0xee; 32]),
                    },
                ]) as Arc<[OutputSegment]>
            } else {
                Arc::new([])
            },
        })
        .collect::<Vec<_>>();

    let plan = EditPlan {
        operations: operations.into(),
        outputs: outputs.into(),
        usage: PlanningUsage::default(),
        digest: PlanDigest(Sha256Digest([0; 32])),
    };
    (snapshot, plan)
}

#[test]
fn plan_hash_golden_cbor_covers_all_discriminants_and_integer_widths() {
    let (snapshot, plan) = fixture();
    let actual = encode_plan_record(&snapshot, &plan).expect("golden plan should encode");
    let expected = decode_hex(GOLDEN_HEX);

    assert_eq!(actual, expected, "actual CBOR hex: {}", encode_hex(&actual));
}

#[test]
fn plan_hash_golden_digest_is_domain_separated_and_stable() {
    let (snapshot, plan) = fixture();
    let actual = plan_digest(&snapshot, &plan).expect("golden plan should hash");

    assert_eq!(actual.0.to_prefixed_hex(), GOLDEN_DIGEST.trim());
}

#[test]
fn plan_hash_sorts_input_and_output_records_by_utf8_bytes() {
    let (snapshot, plan) = fixture();
    let mut reversed_snapshot = snapshot.clone();
    reversed_snapshot.files = snapshot.files.iter().cloned().rev().collect();
    reversed_snapshot.absent_paths = snapshot.absent_paths.iter().cloned().rev().collect();
    let mut reversed_plan = plan.clone();
    reversed_plan.outputs = plan.outputs.iter().cloned().rev().collect();

    let canonical = plan_digest(&snapshot, &plan).expect("canonical fixture should hash");
    let reversed =
        plan_digest(&reversed_snapshot, &reversed_plan).expect("reordered fixture should hash");

    assert_eq!(canonical, reversed);
}

fn decode_hex(input: &str) -> Vec<u8> {
    let compact = input
        .lines()
        .filter_map(|line| line.split('#').next())
        .flat_map(str::split_whitespace)
        .collect::<String>();
    assert!(compact.len().is_multiple_of(2), "golden hex must be paired");
    compact
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let pair = std::str::from_utf8(pair).expect("golden hex should be UTF-8");
            u8::from_str_radix(pair, 16).expect("golden should contain hexadecimal bytes")
        })
        .collect()
}

fn encode_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}
