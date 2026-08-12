# Phase 1 checkpoint

Status: PASS
Commit: not committed

## Delivered

- Five-crate Rust workspace with the planned dependency direction and an inert
  `codesplice` binary.
- Rust 1.97.1 pin, rustfmt and Clippy policy, Apache-2.0 license, and Linux x86_64
  plus macOS arm64 CI jobs.
- Documented core domain skeleton and typed core, filesystem, protocol, and CLI
  placeholder errors with unsafe code forbidden.
- Authoritative public contract docs plus deferred protocol and transaction schema
  directories.
- Fixture, golden-vector, and scenario roots, shared path helpers, and an
  architecture dependency test.
- Planning gate verified at commit `4455bdd`; the plan marks it complete and no
  separate Phase 0 checkpoint is required by the phase process.

## Verification

- `cargo metadata --no-deps` — pass
- `cargo fmt --all --check` — pass
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — pass
- `cargo test --workspace --all-features` — pass (4 tests)
- `cargo build --workspace --all-features` — pass

## Demonstrated behavior

- Metadata contains exactly the five planned crates: core has no dependencies;
  filesystem and protocol depend only on core; CLI depends on all three; test
  support is isolated.
- The dependency-direction integration test and common test-root unit tests pass.
- Production crates compile with unsafe code forbidden and contain no editing,
  parsing, filesystem acquisition, planning, locking, or mutation behavior.
- Documentation contains no `TBD`, `TODO`, or `FIXME` product decisions.

## Decisions made within phase authority

- Pinned Rust 1.97.1 with edition 2024 and used only standard-library error types
  in Phase 1, leaving third-party dependency choices to the phases that need them.
- Schema directories contain explanatory readmes; schema files remain deferred to
  their explicitly authorized Phase 2 and Phase 5 checkpoints.

## Deviations or concerns

- None.

## Next phase readiness

- Ready.
