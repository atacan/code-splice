# CodeSplice

CodeSplice is a Rust command-line tool for moving or copying exact byte ranges
already present in workspace files. The `v0.1.0` pilot targets Linux x86_64 on
local ext4 and macOS arm64 on local APFS.

The `v0.1.0` pilot exposes strict protocol-v1 request validation, read-only
workspace inspection, immutable planning, bounded preview diffs, diagnostic
locking, and complete human or JSON reports. Commit and target-mutating
completion/rollback use one persistent engine for up to 100 changed targets.
Multi-target visibility is recoverable rather than atomic, and recovery reports
identify when mixed old/new bytes may be visible.

Protocol version 1, plan-hash version 1, and transaction-record version 1 are
frozen at the `v0.1.0` tag. Release archives are produced only for
`x86_64-unknown-linux-gnu` and `aarch64-apple-darwin`.

## Workspace

- `codesplice-core`: immutable domain model and pure planning concepts.
- `codesplice-fs`: workspace, snapshot, transaction, and recovery boundary.
- `codesplice-protocol`: JSON protocol and report boundary.
- `codesplice-cli`: command parsing, orchestration, and rendering boundary.
- `codesplice-test-support`: fixtures and helpers available only to tests.

The implementation authority is
[`notes/implementation_plan.md`](notes/implementation_plan.md). Short public
contracts live under [`docs/`](docs/).

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --all-features
```

CodeSplice is licensed under Apache-2.0.
