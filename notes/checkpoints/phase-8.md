# Phase 8 checkpoint

Status: PASS
Commit: not committed

## Delivered

- One shared transaction engine for one through 100 changed targets, with the
  Phase 7 CLI admission limit removed while retaining the single-target
  filesystem regression boundary as a delegate to the same implementation.
- Manifest targets sorted by normalized UTF-8 path bytes with contiguous persisted
  `target_index`, generated per-target candidate/backup names, and full indexed
  state snapshots after each candidate, backup, install, and restoration.
- Preparation and verification of every candidate before input revalidation and
  `Committing` publication, followed by deterministic forward target commit and
  final verification of every planned length, digest, identity, and permission mode.
- Conservative recovery that classifies the entire target set before mutation,
  completes forward, rolls back in reverse order, resumes filesystem lag at every
  target stage, and returns `TRANSACTION_RECOVERY_REQUIRED` when automatic rollback
  cannot finish.
- Target-index-specific debug failpoints in addition to the existing generic and
  record-publication failpoints; release builds still cannot activate failpoints.
- Protocol-v1 commit visibility reporting as `recoverable_not_atomic` and recovery
  visibility reporting as `all_original`, `mixed_old_new_possible`, or
  `all_planned`, with matching schema and public documentation updates.
- Phase 8 filesystem, commit, automatic-rollback, completion, rollback, conflict,
  target-index crash, and every-state-record-boundary crash tests. Existing
  single-target commit and crash-recovery behavior remains covered by the same engine.

## Verification

- `cargo fmt --all --check` — pass
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — pass
- `cargo test --workspace --all-features` — pass (136 tests)
- `cargo build --workspace --all-features` — pass
- `cargo test -p codesplice-fs multi_target` — pass (1 matching test)
- `cargo test --test multi_target_commit` — pass (4 tests)
- `cargo test --test multi_target_crash_recovery` — pass (6 tests)

## Demonstrated behavior

- A three-target move/copy transaction persisted targets as `new`, `source`, and
  `target` with indices 0, 1, and 2, then committed every planned digest and
  reported both inserted payload digests and non-atomic visibility.
- Failure after all candidates verified but before mutation left every original
  target and absence state unchanged. Failure at the final target rolled back all
  earlier installed or backed-up targets.
- A second injected rollback failure returned `TRANSACTION_RECOVERY_REQUIRED`;
  explicit recovery in a fresh process resumed reverse rollback to all-original.
- Interrupting after the first install reported `mixed_old_new_possible`; fresh
  rollback restored all original digests and absence, while an equivalent fresh
  completion produced every planned digest.
- A third-party modification of a later target caused whole-transaction recovery
  classification to return `RECOVERY_CONFLICT` before changing any other target.
- Indexed crashes covered candidate, backup, install, and rollback steps at every
  applicable target index, plus both sides of state-record sequences 0 through 10;
  every fixture resolved to all-old, all-new, or the explicit conflict above.

## Decisions made within phase authority

- Phase 7's `commit_single_target` filesystem API remains as a compatibility and
  regression boundary, but delegates to the new unrestricted `commit` method; the
  CLI and all multi-target execution use only the shared engine.
- Preparing state snapshots record each ready candidate before the final candidate
  transition publishes `Prepared`, keeping crash recovery bounded and explicit.
- Debug failpoint configuration accepts a comma-separated set so one test can
  inject both a late commit failure and an incomplete automatic rollback.

## Deviations or concerns

- None. Local runtime evidence is macOS arm64 on APFS; Linux x86_64/ext4 remains
  represented by the existing CI matrix, and full platform qualification remains
  Phase 9.

## Next phase readiness

- Ready.
