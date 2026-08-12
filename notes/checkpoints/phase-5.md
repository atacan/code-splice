# Phase 5 checkpoint

Status: PASS
Commit: not committed

## Delivered

- Creation and validation of the user-owned `.codesplice` control tree, private
  transaction directories, and a real non-group/other-writable lock file, with
  directory syncing and retained identity revalidation.
- Nonblocking exclusive mutation locks and shared diagnostic locks, including
  deterministic cross-process contention reporting through `TRANSACTION_BUSY`.
- Random 128-bit lowercase transaction IDs, exclusive active-directory creation,
  checks against both completed suffixes, and an eight-attempt collision bound.
- Strict transaction-v1 manifest and full-state schemas, typed payloads, golden
  JSON examples, and golden complete record-envelope bytes.
- Checksummed manifest/state envelopes with big-endian version and length fields,
  exclusive mode-`0600` temporaries, full write/flush/file sync, no-replace
  publication, transaction-directory sync, and immutable published names.
- Full state snapshots with contiguous sequences, manifest/prior checksum links,
  pure transition validation, per-target semantic validation against original
  existence, and rejection of gaps, forks, invalid combinations, bad checksums,
  truncation, oversized records, unknown fields, and invalid temporaries.
- Lowerable record, target, state-count, cumulative-state-byte, directory-scan,
  recovery-read, and projected transaction-disk limits with boundary tests.
- Bounded active/completed scans, active-transaction admission blocking,
  suffix-classified cleanup-only deletion, and preservation of every unknown entry.
- Read-only `recover --list` and `recover ID --status`, plus target-independent
  rollback cleanup for canonical `orphan_record` and `manifest_only` directories.
- A pure filesystem-observation recovery classifier with exhaustive existing/new
  target tables for Preparing, Prepared, Committing, Committed, RollingBack, and
  RolledBack states.
- Test-only subprocess failpoints around record publication; normal library builds
  contain an inert implementation and cannot activate the environment trigger.

## Verification

- `cargo fmt --all --check` — pass
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — pass
- `cargo test --workspace --all-features` — pass (100 tests)
- `cargo build --workspace --all-features` — pass
- `cargo test -p codesplice-fs journal` — pass (22 matching tests)
- `cargo test -p codesplice-fs recovery_classifier` — pass (4 tests)
- `cargo test -p codesplice-cli --test recovery_status_cli` — pass (5 tests)

## Demonstrated behavior

- A missing control tree yields an empty recovery list without creating anything;
  a partial, replaced, permissive, symlinked, or malformed control tree fails closed.
- Exclusive/shared lock contention is nonblocking and deterministic across a fresh
  CLI process, while a quiescent valid tree can be scanned under the shared lock.
- Published manifest and state records round-trip exact golden envelope bytes and
  one valid contiguous chain folds to the expected recovery actions.
- Torn, truncated, trailing, checksum-invalid, oversized, gapped, forked, and
  semantically impossible records are rejected without adopting unpublished state.
- Every documented state-machine edge and every candidate/commit/rollback tag
  combination is checked; every recovery observation tuple is either the exact
  documented action set or `RECOVERY_CONFLICT`.
- Orphan and manifest-only rollback removes only validated transaction-owned
  control entries; a user-target fixture remains byte-for-byte unchanged.
- A new-transaction gate refuses any active directory, removes an empty validated
  cleanup-only suffix directory, and leaves unknown entries untouched on error.

## Decisions made within phase authority

- Manifest/state JSON field names and integer widths are frozen by the checked-in
  transaction-v1 schemas and golden envelopes. Record checksums link the exact
  stored envelope prefix, so JSON member order is intentionally non-semantic.
- Safe Rust wrappers from `rustix` provide advisory locks, effective-user identity,
  and no-replace record publication on the Phase 5 Linux/macOS boundary;
  `getrandom` supplies transaction IDs.
- The recovery response's existing `cleanup_only` classification with an empty
  action list represents successful control-only rollback after its directory has
  been removed.

## Deviations or concerns

- None.

## Next phase readiness

- Ready.
