#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repository_root"

operating_system="$(uname -s)"
architecture="$(uname -m)"

case "$operating_system/$architecture" in
  Linux/x86_64)
    filesystem="$(findmnt -n -o FSTYPE --target "$repository_root")"
    test "$filesystem" = ext4
    rust_target=x86_64-unknown-linux-gnu
    ;;
  Darwin/arm64)
    device="$(df "$repository_root" | tail -n 1 | awk '{print $1}')"
    filesystem="$(mount | awk -v device="$device" '$1 == device { value=$4; sub(/^\(/, "", value); sub(/,$/, "", value); print value }')"
    test "$filesystem" = apfs
    rust_target=aarch64-apple-darwin
    ;;
  *)
    printf 'unsupported qualification row: %s/%s\n' "$operating_system" "$architecture" >&2
    exit 1
    ;;
esac

printf 'qualifying %s/%s on %s\n' "$operating_system" "$architecture" "$filesystem"

qualification_tmp="${CODESPLICE_QUALIFICATION_TMPDIR:-$repository_root/target/phase9-tmp}"
mkdir -p "$qualification_tmp"
export TMPDIR="$qualification_tmp"

cargo test --workspace --all-features --tests
scripts/run-fuzz-regressions.sh
cargo test -p srcmv-fs --test platform_qualification
cargo test -p srcmv-cli --test single_target_crash_recovery
cargo test -p srcmv-cli --test multi_target_crash_recovery
scripts/audit-unsafe.sh

RUSTC_BOOTSTRAP=1 RUSTFLAGS='-Zsanitizer=address' PROPTEST_CASES=512 \
  cargo test --workspace --all-features fuzz_regression --target "$rust_target"

# Rebuild without sanitizer flags before enabling allocator diagnostics. Keeping
# the build itself outside those variables avoids perturbing compiler subprocesses.
cargo test --workspace --all-features fuzz_regression --no-run

case "$operating_system" in
  Linux)
    MALLOC_CHECK_=3 MALLOC_PERTURB_=165 PROPTEST_CASES=512 \
      cargo test --workspace --all-features fuzz_regression
    ;;
  Darwin)
    MallocNanoZone=0 MallocScribble=1 MallocPreScribble=1 MallocGuardEdges=1 \
      PROPTEST_CASES=512 cargo test --workspace --all-features fuzz_regression
    ;;
esac
