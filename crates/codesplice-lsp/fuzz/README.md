# CodeSplice LSP fuzzing

This standalone package keeps nightly-only `cargo-fuzz` tooling outside the
stable workspace build. Both targets call the production byte-oriented APIs;
they do not contain a second framing or JSON-RPC parser.

The dependency is pinned to `libfuzzer-sys = 0.4.13`. That release has no
declared MSRV and depends on `arbitrary 1` plus build dependency `cc >=1.0.83`.
It builds LLVM libFuzzer C++ support, so it remains development-only and is not
linked into CodeSplice binaries.

## Targets

- `framing-header` passes raw input through `FrameDecoder` with both whole-input
  and deliberately fragmented delivery, then sends every completed body to
  `decode_body`.
- `jsonrpc-envelope` sends a raw, already-framed body to `decode_body` for JSON
  parsing and envelope classification.

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
```

`cargo-fuzz` is intentionally non-gating. A stable compile-only check of the
harness package is still useful when the installed compiler supports it:

```console
cargo check --manifest-path crates/codesplice-lsp/fuzz/Cargo.toml --locked
```

After a failure, minimize it and copy the result into the checked-in corpus:

```console
cargo +nightly fuzz cmin framing-header
cargo +nightly fuzz tmin framing-header artifacts/framing-header/<artifact>
```
