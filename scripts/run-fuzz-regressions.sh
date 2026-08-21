#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repository_root"

: "${PROPTEST_CASES:=4096}"
export PROPTEST_CASES

cargo test --workspace --all-features fuzz_regression
cargo test -p srcmv-core --test planner line_index_fuzz_property
cargo test -p srcmv-core --test planner event_composition_fuzz_property
