# Phase 2 checkpoint

Status: PASS
Commit: not committed

## Delivered

- Normative Draft 2020-12 request and response schemas for protocol version 1.
- Strict request decoding with duplicate-key, unknown-field, enum, integer,
  digest, selector, anchor, source-precondition, size, depth, operation-count,
  distinct-path-count, and path-byte validation.
- Filesystem-independent DTO-to-domain conversion for all move/copy, selector,
  anchor, and precondition variants.
- Complete `inspect`, `apply`, `recover`, `capabilities`, and `protocol-version`
  command grammar, including exact commit-intent policy enforcement.
- Fully implemented JSON capability and protocol-version queries; all execution
  routes validate input and return a registered development-only `INTERNAL_ERROR`
  without inspecting or mutating the workspace.
- Centralized v1 error/warning registries, exit mapping, retryability, structured
  context, one-value JSON stdout, absolute-path redaction, and terminal-safe human
  error escaping.
- Golden request, response, registry, and command results plus protocol and CLI
  boundary tests.

## Verification

- `cargo fmt --all --check` — pass
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — pass
- `cargo test --workspace --all-features` — pass (30 tests)
- `cargo build --workspace --all-features` — pass
- `cargo test -p codesplice-protocol` — pass (15 tests)
- `cargo test -p codesplice-cli --test protocol_cli` — pass (9 tests)

## Demonstrated behavior

- Golden decoding covers move and copy, line and byte selectors, all five anchors,
  both destination preconditions, both source/destination digest preconditions,
  and the maximum accepted `u64` coordinate.
- Malformed syntax, duplicate and unknown fields, invalid enums and numbers,
  missing/source-absence preconditions, bad digest spelling, empty operations,
  release request-size/depth excess, and lowered boundary limits fail with stable
  registered codes.
- Every registered error maps to its documented category and exit status, and the
  schema registries exactly match the code registries.
- Every valid command form reaches either a complete target-independent response
  or its named development-only execution stub. Commit rejects zero or two plan
  policies, JSON mode emits exactly one value plus LF on stdout, and human errors
  use stderr with control and bidi characters escaped.
- Protocol conversion depends only on `codesplice-core` and performs no filesystem
  access. CLI tests confirm request-file/stdin ingestion does not reach workspace
  inspection or mutation.

## Decisions made within phase authority

- Protocol integers convert to `u64`; schema/runtime checks preserve the exact
  nonnegative range through `18446744073709551615`.
- The Phase 2 capability response reports request parsing as available and later
  workspace/preview/commit/recovery features as unavailable, avoiding simulated
  support while keeping the complete command grammar testable.
- Error retryability and the concrete v1 query/report field names are frozen by
  the checked-in registries, response schema, and golden vectors.

## Deviations or concerns

- None.

## Next phase readiness

- Ready.
