# Phase 3 checkpoint

Status: PASS
Commit: not committed

## Delivered

- Canonical workspace resolution retaining the macOS/Linux POSIX root identity.
- Strict UTF-8 workspace-relative path parsing, ASCII-case-insensitive reserved-tree
  rejection, no-symlink parent walks, regular-file enforcement, full root-to-parent
  identity capture, and physical alias rejection.
- Immutable core snapshots backed by shared byte storage, with content digests,
  file and parent identities, link counts, and validated absent destinations.
- One-handle-per-attempt snapshot acquisition with metadata before and after the
  bounded streaming read/hash, parent-entry confirmation, and at most three total
  attempts for demonstrably unstable reads.
- Compact one-boundary-per-line indexing for LF, CRLF, lone CR, unterminated,
  empty, non-UTF-8, mixed-terminator, and long-line content.
- Lowerable enforcement for path, identity, individual/aggregate snapshot byte,
  aggregate line-count, and line-index-memory limits.
- Working protocol-v1 `inspect --json` reports for regular and absent paths,
  opaque physical-identity hashes, and `OBSERVATION_MAY_BE_STALE` until Phase 5
  introduces lock coordination.
- Updated Phase 3 capability reporting, protocol documentation, golden results,
  and read-only CLI/filesystem coverage.

## Verification

- `cargo fmt --all --check` — pass
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — pass
- `cargo test --workspace --all-features` — pass (51 tests)
- `cargo build --workspace --all-features` — pass
- `cargo test -p codesplice-core line_index` — pass (5 tests)
- `cargo test -p codesplice-fs snapshot` — pass (10 tests)
- `cargo test -p codesplice-cli --test inspect_cli` — pass (6 tests)

## Demonstrated behavior

- Line indexing preserves byte coordinates across empty, unterminated, LF, CRLF,
  lone-CR, mixed, non-UTF-8, and 128-KiB-line fixtures without phantom lines.
- Snapshot tests retain a multiply linked source and its link count, reject two
  distinct hard-link spellings as `FILE_ALIAS`, reject stable stale digests without
  retry, retry a changed file and a replaced parent entry, and return `FILE_CHANGED`
  after exactly three continuously unstable attempts.
- Path tests reject absolute, empty-component, `.`, `..`, reserved, missing-parent,
  parent/final symlink, directory, and physical-alias cases with registered errors.
- Boundary tests reject identity, individual file, aggregate byte, aggregate line,
  and aggregate index-memory excess through lowerable limits.
- CLI tests report exact SHA-256, byte length, line count, file type, opaque identity,
  and valid absence in one JSON value plus LF; tree observations confirm inspection
  and snapshot acquisition do not create or modify workspace entries.
- The checkpoint ran locally on Darwin arm64; the existing `CI` matrix retains the
  Linux x86_64 and macOS arm64 jobs for the same common checks.

## Decisions made within phase authority

- `LineIndex` stores each logical line's exclusive byte end, including its original
  terminator, using one `u64` per line.
- Opaque identity values are domain-separated SHA-256 hashes of the POSIX device and
  inode pair; raw physical identities remain internal to snapshot acquisition.
- Exact duplicate inspection paths preserve request order and reuse one observation;
  immutable planning snapshot files remain sorted by normalized UTF-8 path.

## Deviations or concerns

- None.

## Next phase readiness

- Ready.
