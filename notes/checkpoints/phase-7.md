# Phase 7 checkpoint

Status: PASS
Commit: not committed

## Delivered

- Two-pass single-target commit orchestration with expected-plan checks before and
  after lock acquisition, `PLAN_CHANGED_DURING_COMMIT` identity-change detection,
  a one-changed-target admission guard, and a noncreating no-op fast path.
- One transaction engine for new and existing targets: manifest and `Preparing`
  publication, streamed/synced/verified candidates, final input and parent
  revalidation, `Prepared`/`Committing`/`Committed` state transitions, and bounded
  terminal cleanup.
- No-replace backup, install, restore, and terminal-directory renames through
  rustix's Linux `renameat2(RENAME_NOREPLACE)` / macOS `renamex_np(RENAME_EXCL)`
  abstraction, with collision, cross-device, primitive-unavailable, and qualified
  APFS/ext4 filesystem failures mapped to registered protocol errors.
- Existing-target permission preservation from the verified backup and exact
  `0666 & !startup_umask` modes for new files, including `000`, `022`, `027`, and
  `077` fixtures.
- Conservative single-target completion and rollback from every journal state,
  including filesystem lag during commit, rollback, active-to-completed rename,
  and cleanup. Candidate identity remains authoritative even for equal bytes.
- Debug-build-only subprocess crash failpoints before and after every record
  publication, candidate stage, target rename, verification, rollback step, and
  cleanup step; release builds cannot activate them.
- Protocol-v1 commit reports with terminal state, changed files, preserved modes,
  exact inserted-payload digests, metadata warnings, and `null` transaction/digest
  fields for the corresponding no-op cases. Capabilities now truthfully report
  Phase 7 commit and recovery support.
- Phase 7 filesystem-layer, CLI, crash-recovery, plan-change, collision, source-
  revalidation, mode, and exact-report integration tests plus updated public docs.

## Verification

- `cargo fmt --all --check` — pass
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — pass
- `cargo test --workspace --all-features` — pass (126 tests)
- `cargo build --workspace --all-features` — pass
- `cargo test -p codesplice-fs single_target` — pass (3 matching tests)
- `cargo test --test single_target_commit` — pass (6 tests)
- `cargo test --test single_target_crash_recovery` — pass (7 tests)

## Demonstrated behavior

- An effectful same-file move committed exact planned bytes; a copy into an
  existing target preserved a permission change made after preview; and copies
  into new targets produced the exact startup-umask-derived modes.
- An expected-plan mismatch created no control tree, while an equal-byte physical
  input replacement between planning passes returned
  `PLAN_CHANGED_DURING_COMMIT` with no transaction directory.
- A plan-level same-file no-op returned `transaction_id: null`, no changed files,
  and `inserted_payload_sha256: null` without creating `.codesplice`.
- Crash after backup completed to planned bytes in a fresh process; crash after
  install rolled back to original bytes in a fresh process. Every commit failpoint
  resolved to all-old or all-new, and every rollback failpoint resumed to all-old.
- Completion from `Prepared` rehashed a copy-only source and refused its change
  before target mutation. An external new-destination collision was not
  overwritten, and replacing a candidate with equal bytes but a different inode
  produced an explicit recovery conflict.
- Cross-file move planning was rejected by the temporary one-target guard before
  transaction creation, leaving Phase 8's multi-target work untouched.

## Decisions made within phase authority

- The existing rustix no-replace abstraction is the reviewed platform shim; no
  unsafe code or check-then-rename fallback was added to project crates.
- Commit responses represent preserved existing-target modes as a path-to-mode
  object and expose inserted payload evidence on each resolved operation.
- `RollingBack` classification accepts only the additional identity-and-digest-
  verified lag combinations produced between the Phase 7 rollback steps.

## Deviations or concerns

- None. Local runtime evidence is macOS arm64 on APFS; the existing CI matrix
  contains the Linux x86_64 job, and full ext4/APFS qualification remains Phase 9.

## Next phase readiness

- Ready.
