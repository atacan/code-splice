#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repository_root"

if test "$#" -ne 3; then
  printf 'usage: %s TARGET-TRIPLE BINARY OUTPUT-DIRECTORY\n' "$0" >&2
  exit 2
fi

target="$1"
binary="$2"
output_directory="$3"
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"

case "$target" in
  x86_64-unknown-linux-gnu|aarch64-apple-darwin) ;;
  *)
    printf 'unsupported release package target: %s\n' "$target" >&2
    exit 4
    ;;
esac

test "$version" = 0.1.0
test -x "$binary"
mkdir -p "$output_directory"
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
package="codesplice-v${version}-${target}"
mkdir "$staging/$package"
cp "$binary" "$staging/$package/codesplice"
cp LICENSE README.md "$staging/$package/"
cp docs/protocol.md docs/agent-integration.md docs/platform-support.md \
  "$staging/$package/"
tar -C "$staging" -czf "$output_directory/$package.tar.gz" "$package"

case "$(uname -s)" in
  Darwin) shasum -a 256 "$output_directory/$package.tar.gz" ;;
  Linux) sha256sum "$output_directory/$package.tar.gz" ;;
esac
