#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repository_root"

for crate_root in crates/*/src/lib.rs; do
  if command -v rg >/dev/null 2>&1; then
    rg -q '^#!\[forbid\(unsafe_code\)\]$' "$crate_root"
  else
    grep -q -E '^#!\[forbid\(unsafe_code\)\]$' "$crate_root"
  fi
done

if command -v rg >/dev/null 2>&1; then
  direct_unsafe="$(rg -n --glob '*.rs' '(^|[^[:alnum:]_])unsafe[[:space:]]*(\{|fn|impl|trait)' crates || true)"
else
  direct_unsafe="$(grep -R -n -E --include='*.rs' '(^|[^[:alnum:]_])unsafe[[:space:]]*(\{|fn|impl|trait)' crates || true)"
fi
if [[ -n "$direct_unsafe" ]]; then
  printf '%s\n' "$direct_unsafe" >&2
  printf '%s\n' 'direct unsafe code exists outside the reviewed safe no-replace wrapper' >&2
  exit 1
fi

RUSTFLAGS='-Funsafe-code' cargo check --workspace --all-targets --all-features
