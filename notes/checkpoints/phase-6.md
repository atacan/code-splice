# Phase 6 checkpoint

Status: PASS
Commit: not committed

## Delivered

- End-to-end `apply --preview` orchestration from strict protocol parsing through
  immutable snapshot acquisition, pure planning, plan hashing, report conversion,
  and JSON or human rendering.
- Protocol-v1 preview records for resolved byte coordinates, operation effects,
  selected payload digests, output before/after lengths and digests,
  `plan_hash_version`, `plan_sha256`, and the opaque workspace identity hash.
- Linear-memory text diffs with explicit `LF`, `CRLF`, lone-`CR`, and unterminated
  line labels; common exact prefix/suffix reduction; 8 MiB per-side detailed input
  bounds; 10,000,000 counted work units; and exact 4 MiB human/JSON diff bounds.
- Binary summaries with lengths, digests, and bounded base64 head/tail samples,
  plus `DIFF_TRUNCATED` summary fallback when detailed text exceeds a bound.
- Digest-neutral `--no-diff`, a 16 MiB exact serialized JSON preview limit, one
  JSON value plus LF on stdout, and terminal-safe human paths and diff content.
- Phase 5 diagnostic-lock integration retained through control scan, snapshot,
  diff construction, and report serialization. Missing control state remains
  noncreating with `OBSERVATION_MAY_BE_STALE`; exclusive contention is busy; a
  quiescent active transaction requires recovery before preview or inspect.
- Phase 6 capability reporting and public documentation for preview availability,
  diff behavior, resource limits, and the still-disabled commit boundary.

## Verification

- `cargo fmt --all --check` — pass
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — pass
- `cargo test --workspace --all-features` — pass (110 tests)
- `cargo build --workspace --all-features` — pass
- `cargo test -p codesplice-cli --test preview_cli` — pass (5 tests)
- `cargo test --test preview_read_only` — pass (4 tests)

## Demonstrated behavior

- One batch combined an effectful cross-file move, a copy into a new destination,
  and a same-file start-anchored no-op. The report exposed all three resolved byte
  ranges and destination offsets, selected payload hashes, three output hashes,
  plan-hash version 1, the plan digest, and the workspace identity hash.
- The demonstration verified the two existing files remained byte-for-byte
  unchanged, the planned new file remained absent, and no `.codesplice` tree was
  created by preview.
- A pre-existing valid control tree produced a warning-free preview while leaving
  every entry identity, mode, length, and modification timestamp unchanged.
- Binary bytes produced bounded base64 samples; mixed CRLF/CR/LF input retained
  each terminator label; a large text change emitted `DIFF_TRUNCATED`; and
  `--no-diff` preserved the plan digest, operations, and outputs.
- A concurrently held exclusive CodeSplice lock returned `TRANSACTION_BUSY`, while
  a quiescent unfinished transaction returned `TRANSACTION_RECOVERY_REQUIRED` for
  both preview and inspect without planning the partial workspace.

## Decisions made within phase authority

- Text reporting uses a bounded common-prefix/common-suffix line comparison rather
  than a quadratic edit matrix. It reports the exact changed middle and charges
  input scanning plus rendered bytes against the documented work budget.
- The existing schema's single diff record uses `kind: text` with a structured
  reason summary when text detail is bounded away, and `kind: binary` with per-
  output digest/sample summaries when any changed output is binary.

## Deviations or concerns

- None.

## Next phase readiness

- Ready.
