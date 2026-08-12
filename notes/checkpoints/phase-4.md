# Phase 4 checkpoint

Status: PASS
Commit: not committed

## Delivered

- A filesystem-independent planner that resolves line/byte selectors and every
  destination anchor against one immutable workspace snapshot.
- The normative single event sweep, producing only original-snapshot and payload
  segment references with deletion-end, insertion, deletion-start, and EOF order.
- Conflict validation for coordinate bounds, incompatible preconditions, mixed
  path states, overlapping move deletions, and insertions strictly inside deletions.
- Streamed output length, SHA-256, and byte-equality classification without
  retaining materialized output buffers in `EditPlan`.
- Unchanged, modified-existing, created-new, and emptied-existing outputs, with
  changing hard-linked existing outputs rejected and byte-identical results
  excluded from transaction-target accounting.
- Lowerable per-output, aggregate-output, per-output-segment, aggregate-segment,
  changed-target, projected-response, and planning-memory limits.
- RFC 8949 deterministic positional CBOR and the domain-separated plan digest,
  with sorted input/output records and request-ordered resolved operations.
- Annotated golden CBOR/digest vectors covering every discriminant, absent input,
  non-UTF-8 content, same-file no-op, same-offset operations, and integer widths
  through `u64::MAX`.
- Required composition fixtures plus property tests for exact move/copy payload
  bytes, selected/inserted digest equality, and repeat-plan determinism.
- The complete selector, anchor, precondition, operation, output, input-state,
  effect, and segment discriminant table in `docs/specification.md`.

## Verification

- `cargo fmt --all --check` — pass
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — pass
- `cargo test --workspace --all-features` — pass (69 tests)
- `cargo build --workspace --all-features` — pass
- `cargo test -p codesplice-core planner` — pass (15 tests)
- `cargo test -p codesplice-core plan_hash` — pass (3 tests)

## Demonstrated behavior

- Same-file forward/backward moves, start/end no-ops, cross-file move/copy,
  whole-file moves, EOF insertion, and new-file request-order insertion all produce
  the specified byte recipes.
- Overlapping copies remain valid; overlapping move deletions and insertions
  strictly inside deletion ranges fail, while both deletion boundaries and
  adjacent deletions remain valid.
- A copy still reads immutable bytes removed by another move, and an effectful
  recipe that recreates original bytes remains represented as an unchanged output
  without becoming a transaction target.
- Arbitrary generated move/copy payloads retain exact selected bytes and SHA-256
  values in payload segments; planning the same snapshot and request twice yields
  an equal plan and digest.
- Exact resource limits pass and one-byte/count-lower limits reject through
  accounting fixtures, without allocating release-sized inputs.
- Golden CBOR bytes use only definite arrays, shortest unsigned integers,
  byte/text strings, and null; input/output record ordering is independent of
  source collection order.

## Decisions made within phase authority

- Deterministic CBOR is emitted by a small purpose-built encoder so forbidden CBOR
  constructs are unrepresentable and integer-width drift is directly golden-tested.
- Planning-memory accounting charges retained records, segments, and owned path
  bytes; projected response accounting uses conservative fixed structural charges
  documented in `docs/resource-limits.md` before Phase 6 enforces exact JSON size.
- `proptest` is a core dev dependency only; production planner dependencies remain
  limited to SHA-256 and the standard library.

## Deviations or concerns

- None.

## Next phase readiness

- Ready.
