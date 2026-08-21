# Unsafe-code audit

Phase 9 found no direct unsafe Rust in any workspace crate. Every crate root uses
`#![forbid(unsafe_code)]`, and `scripts/audit-unsafe.sh` scans for unsafe blocks,
functions, implementations, and traits before compiling all targets with
`RUSTFLAGS=-Funsafe-code`.

The no-replace platform boundary is implemented through rustix's safe
`renameat_with(..., RenameFlags::NOREPLACE)` wrapper. srcmv contains no FFI
or weaker check-then-rename fallback. The wrapper maps collision, cross-device,
and unavailable-primitive errors to fail-closed srcmv errors. Native
collision regression tests verify that both source and destination entries remain
unchanged.

Third-party dependency internals are outside the direct-source audit; their safe
APIs do not relax the workspace's `unsafe_code` prohibition.
