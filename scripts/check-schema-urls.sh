#!/usr/bin/env bash
# Dereferences every absolute schema $id/$ref committed under docs/schema and
# compares each response byte-for-byte with its tracked file
# (docs/renaming-plan.md Section 1.4). Run after the repository rename; any
# drift between the published namespace and this tree fails the check.
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repository_root"

base='https://raw.githubusercontent.com/atacan/srcmv/main/docs/schema/'

urls="$(
  grep -rhoE "\"${base}[^\"]+\"" docs/schema --include='*.json' \
    | sort -u | tr -d '"'
)"
if test -z "$urls"; then
  printf 'check-schema-urls: no absolute schema URLs found under docs/schema\n' >&2
  exit 1
fi

status=0
while IFS= read -r url; do
  tracked="docs/schema/${url#"${base}"}"
  if ! test -f "$tracked"; then
    printf 'check-schema-urls: URL has no tracked file: %s\n' "$url" >&2
    status=1
    continue
  fi
  if ! curl --fail --silent --location --proto '=https' --tlsv1.2 "$url" \
    | cmp -s - "$tracked"; then
    printf 'check-schema-urls: published bytes differ from %s\n' "$tracked" >&2
    status=1
  else
    printf 'check-schema-urls: OK %s\n' "$url"
  fi
done <<<"$urls"

if test "$status" -eq 0; then
  printf 'check-schema-urls: all canonical schema URLs match tracked files\n'
fi
exit "$status"
