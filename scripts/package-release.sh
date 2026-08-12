#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repository_root"
umask 022

if test "$#" -ne 3; then
  printf 'usage: %s TARGET-TRIPLE BINARY OUTPUT-DIRECTORY\n' "$0" >&2
  exit 2
fi

target="$1"
binary="$2"
output_directory="$3"
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+([+-][0-9A-Za-z.-]+)?$ ]]; then
  printf 'invalid workspace release version: %s\n' "$version" >&2
  exit 3
fi

case "$target" in
  x86_64-unknown-linux-gnu|aarch64-apple-darwin) ;;
  *)
    printf 'unsupported release package target: %s\n' "$target" >&2
    exit 4
    ;;
esac

test -x "$binary"
test "$("$binary" --version)" = "codesplice $version"
mkdir -p "$output_directory"
staging="$(mktemp -d)"
trap 'rm -rf "$staging"' EXIT
package="codesplice-v${version}-${target}"
mkdir "$staging/$package"
cp "$binary" "$staging/$package/codesplice"
cp LICENSE README.md "$staging/$package/"
cp docs/protocol.md docs/agent-integration.md docs/platform-support.md \
  "$staging/$package/"
chmod 0755 "$staging/$package/codesplice"
chmod 0644 "$staging/$package/"*.md "$staging/$package/LICENSE"

# Normalize payload timestamps to the release commit and keep the gzip header
# free of a build timestamp. This makes repeated packaging of the same binary
# stable on one native host; the release workflow records the digest of the
# exact archive it publishes.
source_date_epoch="${SOURCE_DATE_EPOCH:-$(git log -1 --format=%ct)}"
if ! [[ "$source_date_epoch" =~ ^[0-9]+$ ]]; then
  printf 'invalid SOURCE_DATE_EPOCH: %s\n' "$source_date_epoch" >&2
  exit 6
fi

case "$(uname -s)" in
  Darwin) archive_timestamp="$(date -u -r "$source_date_epoch" +%Y%m%d%H%M.%S)" ;;
  Linux) archive_timestamp="$(date -u -d "@$source_date_epoch" +%Y%m%d%H%M.%S)" ;;
  *)
    printf 'unsupported packaging host: %s\n' "$(uname -s)" >&2
    exit 5
    ;;
esac
find "$staging/$package" -exec touch -t "$archive_timestamp" {} +

# COPYFILE_DISABLE suppresses macOS AppleDouble metadata.
COPYFILE_DISABLE=1 tar -C "$staging" -cf - "$package" \
  | gzip -n >"$output_directory/$package.tar.gz"

case "$(uname -s)" in
  Darwin)
    digest="$(shasum -a 256 "$output_directory/$package.tar.gz" | awk '{print $1}')"
    ;;
  Linux)
    digest="$(sha256sum "$output_directory/$package.tar.gz" | awk '{print $1}')"
    ;;
  *)
    printf 'unsupported packaging host: %s\n' "$(uname -s)" >&2
    exit 5
    ;;
esac

printf '%s  %s\n' "$digest" "$package.tar.gz"
