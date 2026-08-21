#!/bin/sh
set -eu

repository_root=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
qualification_root=$(mktemp -d "${TMPDIR:-/tmp}/srcmv-lsp-qualification.XXXXXX")
trap 'rm -rf "$qualification_root"' EXIT HUP INT TERM

qualified=0

if command -v clangd >/dev/null 2>&1 && clangd --version >/dev/null 2>&1; then
    mkdir -p "$qualification_root/clangd"
    printf 'int add(int left, int right) {\n  return left + right;\n}\n' \
        >"$qualification_root/clangd/source.c"
    cargo run --quiet --manifest-path "$repository_root/Cargo.toml" \
        -p srcmv-lsp --example qualify_server --locked -- \
        "$(command -v clangd)" c "$qualification_root/clangd" source.c
    qualified=$((qualified + 1))
fi

if command -v rust-analyzer >/dev/null 2>&1 && rust-analyzer --version >/dev/null 2>&1; then
    mkdir -p "$qualification_root/rust-analyzer/src"
    printf '[package]\nname = "srcmv-lsp-qualification"\nversion = "0.0.0"\nedition = "2024"\n' \
        >"$qualification_root/rust-analyzer/Cargo.toml"
    printf 'pub fn add(left: u32, right: u32) -> u32 { left + right }\n' \
        >"$qualification_root/rust-analyzer/src/lib.rs"
    cargo run --quiet --manifest-path "$repository_root/Cargo.toml" \
        -p srcmv-lsp --example qualify_server --locked -- \
        "$(command -v rust-analyzer)" rust "$qualification_root/rust-analyzer" src/lib.rs
    qualified=$((qualified + 1))
fi

if [ "$qualified" -eq 0 ]; then
    printf '%s\n' 'no qualified real language server is installed; fake-server CI remains authoritative'
fi
