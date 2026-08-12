# CodeSplice

CodeSplice is a Rust command-line tool for moving or copying exact byte ranges
already present in workspace files. The `v0.1.0` pilot targets Linux x86_64 on
local ext4 and macOS arm64 on local APFS.

The implementation is organized as a phased build. Through Phase 3, the binary
exposes the complete command grammar, strict protocol-v1 request validation,
read-only workspace inspection, immutable snapshot acquisition, and
capability/version queries. Planning and execution remain explicitly unavailable.

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
