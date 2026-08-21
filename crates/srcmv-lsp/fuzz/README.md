# srcmv LSP fuzzing

This standalone package keeps nightly-only `cargo-fuzz` tooling outside the
stable workspace build. Every target calls the production byte-oriented APIs;
they do not contain a second framing or JSON-RPC parser.

The dependency is pinned to `libfuzzer-sys = 0.4.13`. That release has no
declared MSRV and depends on `arbitrary 1` plus build dependency `cc >=1.0.83`.
It builds LLVM libFuzzer C++ support, so it remains development-only and is not
linked into srcmv binaries.

## Targets

- `framing-header` passes raw input through `FrameDecoder` with both whole-input
  and deliberately fragmented delivery, then sends every completed body to
  `decode_body`.
- `jsonrpc-envelope` sends a raw, already-framed body to `decode_body` for JSON
  parsing and envelope classification.
- `position-conversion` builds the production `LineIndex` and exercises exact
  byte, LSP-position, LSP-range, and user-coordinate conversions for UTF-8,
  UTF-16, and UTF-32.
- `declaration-line-expansion` derives valid UTF-8 byte ranges and checks the
  production declaration-line extent expansion invariants.
- `hierarchical-symbol-normalization` parses raw document-symbol JSON and sends
  it through production wire-shape validation, position conversion, hierarchy
  flattening, and resource accounting.

The corpora are checked in under `corpus/<target>/`. Add every minimized defect
to the appropriate corpus so normal integration tests can also cover it. The
`artifacts/<target>/` directories are reserved for cargo-fuzz crash artifacts;
their generated contents are ignored.

## Running

Install cargo-fuzz and use a nightly toolchain:

```console
cargo install cargo-fuzz --locked
cargo +nightly fuzz run framing-header -- -max_len=20971520
cargo +nightly fuzz run jsonrpc-envelope -- -max_len=16777216
cargo +nightly fuzz run position-conversion -- -max_len=1048576
cargo +nightly fuzz run declaration-line-expansion -- -max_len=1048576
cargo +nightly fuzz run hierarchical-symbol-normalization -- -max_len=1048576
```

`cargo-fuzz` is intentionally non-gating. A stable compile-only check of the
harness package is still useful when the installed compiler supports it:

```console
cargo check --manifest-path crates/srcmv-lsp/fuzz/Cargo.toml --locked
cargo clippy --manifest-path crates/srcmv-lsp/fuzz/Cargo.toml --all-targets --locked -- -D warnings
```

After a failure, minimize it and copy the result into the checked-in corpus:

```console
cargo +nightly fuzz cmin framing-header
cargo +nightly fuzz tmin framing-header artifacts/framing-header/<artifact>
```
