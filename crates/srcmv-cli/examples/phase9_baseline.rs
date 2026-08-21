//! Reproducible Phase 9 wall-clock baseline generator.

use std::hint::black_box;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use sha2::{Digest, Sha256};
use srcmv_core::{
    AbsentPathSnapshot, Anchor, BatchSpecification, ByteRange, Destination, EditPlan, FileIdentity,
    FileSnapshot, LineIndex, Operation, OperationSpecification, OutputChange, OutputSegment,
    PlanDigest, PlannedOutput, PlanningUsage, Precondition, ResourceBudget, Selector, Sha256Digest,
    SnapshotFileId, SourceSelection, WorkspaceRelativePath, WorkspaceSnapshot, encode_plan_record,
    plan,
};
use srcmv_fs::Workspace;

#[derive(Serialize)]
struct Baseline {
    format_version: u64,
    measured_at_utc: String,
    host: Host,
    methodology: Methodology,
    measurements: Vec<Measurement>,
}

#[derive(Serialize)]
struct Host {
    operating_system: &'static str,
    architecture: &'static str,
    filesystem: String,
    rustc: String,
    uname: String,
}

#[derive(Serialize)]
struct Methodology {
    profile: &'static str,
    clock: &'static str,
    statistic: &'static str,
    warmup_runs: u64,
    timing_scope: &'static str,
}

#[derive(Serialize)]
struct Measurement {
    scenario: &'static str,
    iterations: usize,
    median_milliseconds: f64,
    minimum_milliseconds: f64,
    maximum_milliseconds: f64,
}

fn main() {
    let repository = repository_root();
    let workspace = Workspace::open(&repository).expect("repository workspace should open");
    let filesystem = workspace
        .qualified_filesystem()
        .expect("baseline must run on a qualified filesystem");

    let one_mib = copy_fixture(1024 * 1024, 1);
    let hundred_mib = copy_fixture(100 * 1024 * 1024, 1);
    let one_target = copy_fixture(64, 1);
    let ten_targets = copy_fixture(64, 10);
    let hundred_targets = copy_fixture(64, 100);
    let one_segment = segment_fixture(1);
    let thousand_segments = segment_fixture(1_000);
    let hundred_thousand_segments = segment_fixture(100_000);

    let measurements = vec![
        measure("plan_1_mib", 7, || plan_fixture(&one_mib)),
        measure("plan_100_mib", 3, || plan_fixture(&hundred_mib)),
        measure("plan_1_target", 25, || plan_fixture(&one_target)),
        measure("plan_10_targets", 15, || plan_fixture(&ten_targets)),
        measure("plan_100_targets", 7, || plan_fixture(&hundred_targets)),
        measure("encode_1_segment", 100, || encode_fixture(&one_segment)),
        measure("encode_1000_segments", 25, || {
            encode_fixture(&thousand_segments)
        }),
        measure("encode_100000_segments", 5, || {
            encode_fixture(&hundred_thousand_segments)
        }),
        measure("reject_changed_target_limit", 100, || {
            reject_limit_fixture(&one_target)
        }),
    ];

    let baseline = Baseline {
        format_version: 1,
        measured_at_utc: command("date", &["-u", "+%Y-%m-%dT%H:%M:%SZ"]),
        host: Host {
            operating_system: std::env::consts::OS,
            architecture: std::env::consts::ARCH,
            filesystem: filesystem.as_str().to_owned(),
            rustc: command("rustc", &["--version"]),
            uname: command("uname", &["-a"]),
        },
        methodology: Methodology {
            profile: "cargo run --release",
            clock: "std::time::Instant monotonic wall clock",
            statistic: "median with minimum and maximum",
            warmup_runs: 1,
            timing_scope: "in-memory plan/encode/rejection call; fixture construction excluded",
        },
        measurements,
    };
    println!(
        "{}",
        serde_json::to_string_pretty(&baseline).expect("baseline should serialize")
    );
}

struct CopyFixture {
    snapshot: WorkspaceSnapshot,
    batch: BatchSpecification,
}

fn copy_fixture(byte_count: usize, target_count: usize) -> CopyFixture {
    let bytes = vec![b'x'; byte_count];
    let digest = Sha256Digest(Sha256::digest(&bytes).into());
    let source = FileSnapshot {
        id: SnapshotFileId(0),
        path: path("source"),
        parent_identity: identity(2),
        parent_identities: Arc::new([]),
        identity: identity(3),
        link_count: 1,
        line_index: LineIndex::from_bytes_with_limits(&bytes, u64::MAX, u64::MAX)
            .expect("benchmark line index should build"),
        bytes: bytes.into(),
        digest,
    };
    let absent_paths = (0..target_count)
        .map(|index| AbsentPathSnapshot {
            path: path(&format!("target-{index:03}")),
            parent_identity: identity(2),
            parent_identities: Arc::new([]),
            basename: format!("target-{index:03}"),
        })
        .collect::<Vec<_>>();
    let operations = absent_paths
        .iter()
        .map(|target| {
            Operation::Copy(OperationSpecification {
                source: SourceSelection {
                    path: source.path.clone(),
                    selector: Selector::Bytes {
                        start: 0,
                        end: u64::try_from(byte_count).expect("fixture length should fit"),
                    },
                    precondition: Precondition::Sha256(digest),
                },
                destination: Destination {
                    path: target.path.clone(),
                    anchor: Anchor::FileStart,
                    precondition: Precondition::MustNotExist,
                },
            })
        })
        .collect::<Vec<_>>();
    CopyFixture {
        snapshot: WorkspaceSnapshot {
            workspace_identity: identity(1),
            files: vec![source].into(),
            absent_paths: absent_paths.into(),
        },
        batch: BatchSpecification {
            operations: operations.into(),
        },
    }
}

fn plan_fixture(fixture: &CopyFixture) {
    let result = plan(&fixture.snapshot, &fixture.batch, ResourceBudget::default())
        .expect("baseline fixture should plan");
    black_box(result);
}

fn reject_limit_fixture(fixture: &CopyFixture) {
    let budget = ResourceBudget {
        changed_targets: 0,
        ..ResourceBudget::default()
    };
    let error = plan(&fixture.snapshot, &fixture.batch, budget)
        .expect_err("baseline fixture should exceed the lowered target limit");
    black_box(error);
}

struct SegmentFixture {
    snapshot: WorkspaceSnapshot,
    plan: EditPlan,
}

fn segment_fixture(segment_count: usize) -> SegmentFixture {
    let digest = Sha256Digest(Sha256::digest(b"x").into());
    let source = FileSnapshot {
        id: SnapshotFileId(0),
        path: path("source"),
        parent_identity: identity(2),
        parent_identities: Arc::new([]),
        identity: identity(3),
        link_count: 1,
        bytes: Arc::from(&b"x"[..]),
        digest,
        line_index: LineIndex::from_bytes_with_limits(b"x", 1, 8)
            .expect("one-byte fixture should index"),
    };
    let segments = (0..segment_count)
        .map(|_| OutputSegment::OriginalSlice {
            snapshot_file_id: SnapshotFileId(0),
            range: ByteRange { start: 0, end: 1 },
        })
        .collect::<Vec<_>>();
    SegmentFixture {
        snapshot: WorkspaceSnapshot {
            workspace_identity: identity(1),
            files: vec![source].into(),
            absent_paths: Arc::new([]),
        },
        plan: EditPlan {
            operations: Arc::new([]),
            outputs: vec![PlannedOutput {
                path: path("source"),
                original_digest: Some(digest),
                change: OutputChange::ModifiedExisting,
                resulting_length: u64::try_from(segment_count).expect("segment count should fit"),
                resulting_digest: digest,
                segments: segments.into(),
            }]
            .into(),
            usage: PlanningUsage::default(),
            digest: PlanDigest(digest),
        },
    }
}

fn encode_fixture(fixture: &SegmentFixture) {
    let encoded = encode_plan_record(&fixture.snapshot, &fixture.plan)
        .expect("segment fixture should encode");
    black_box(encoded);
}

fn measure(name: &'static str, iterations: usize, mut run: impl FnMut()) -> Measurement {
    run();
    let mut samples = Vec::with_capacity(iterations);
    for _ in 0..iterations {
        let started = Instant::now();
        run();
        samples.push(started.elapsed().as_secs_f64() * 1_000.0);
    }
    samples.sort_by(f64::total_cmp);
    Measurement {
        scenario: name,
        iterations,
        median_milliseconds: samples[samples.len() / 2],
        minimum_milliseconds: samples[0],
        maximum_milliseconds: samples[samples.len() - 1],
    }
}

const fn identity(inode: u64) -> FileIdentity {
    FileIdentity { device: 1, inode }
}

fn path(value: &str) -> WorkspaceRelativePath {
    WorkspaceRelativePath {
        value: value.to_owned(),
    }
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("CLI crate should be nested beneath the repository")
        .to_path_buf()
}

fn command(program: &str, arguments: &[&str]) -> String {
    let output = Command::new(program)
        .args(arguments)
        .output()
        .unwrap_or_else(|error| panic!("failed to run {program}: {error}"));
    assert!(output.status.success(), "{program} exited unsuccessfully");
    String::from_utf8(output.stdout)
        .expect("command output should be UTF-8")
        .trim()
        .to_owned()
}
