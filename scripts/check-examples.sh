#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cargo build --manifest-path "$repo_dir/Cargo.toml" -p codesplice-cli
export CODESPLICE_BIN="$repo_dir/target/debug/codesplice"

for scenario in \
  00-discover \
  01-move-lines \
  02-copy-bytes-new-file \
  03-all-anchors \
  04-same-file-reorder \
  05-same-file-no-op \
  06-multi-target-split \
  07-exact-bytes \
  08-safe-failures \
  09-recovery
do
  "$repo_dir/examples/run.sh" "$scenario"
done

printf 'All CodeSplice examples passed.\n'

