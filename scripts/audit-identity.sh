#!/usr/bin/env bash
# Identity audit for the srcmv rename (docs/renaming-plan.md Section 5.1).
#
# Usage: scripts/audit-identity.sh docs/renaming-allowlist.txt
#
# Fails when:
# - a tracked filename still contains the former identity;
# - a tracked file contains a former-identity literal outside the allowlist;
# - an allowlist entry matches nothing (stale entry); or
# - the allowlist file itself is malformed.
#
# Only tracked files are inspected, which inherently excludes `.git/**` and
# ungenerated untracked trees such as `target/**`.
set -euo pipefail

if test "$#" -ne 1; then
  printf 'usage: %s ALLOWLIST_FILE\n' "$0" >&2
  exit 2
fi

allowlist_file="$1"
test -s "$allowlist_file" || {
  printf 'audit-identity: allowlist file is missing or empty: %s\n' "$allowlist_file" >&2
  exit 2
}

# Former-identity detector: codesplice / code-splice / Codesplice / CODESPLICE.
readonly DETECT='code[-_]?splice'

status=0

while IFS=$'\t' read -r path pattern reason; do
  case "$path" in
    ''|'#'*) continue ;;
  esac
  if test -z "$pattern" || test -z "$reason"; then
    printf 'audit-identity: malformed allowlist entry (need PATH<TAB>PATTERN<TAB>REASON): %s\n' \
      "${path}	${pattern}	${reason}" >&2
    status=1
    continue
  fi
  if ! test -f "$path"; then
    printf 'audit-identity: allowlist entry names an absent path: %s\n' "$path" >&2
    status=1
    continue
  fi
  if ! grep -qE -- "$pattern" "$path"; then
    printf 'audit-identity: allowlist entry matches nothing: %s (%s)\n' "$path" "$reason" >&2
    status=1
  fi
done <"$allowlist_file"

matches="$(git grep -inI -E "$DETECT" -- . || true)"
if test -z "$matches"; then
  printf 'audit-identity: no former-identity matches found\n'
else
  while IFS= read -r line; do
    file="${line%%:*}"
    rest="${line#*:}"
    allowed=false
    while IFS=$'\t' read -r path pattern reason; do
      case "$path" in ''|'#'*) continue ;; esac
      test "$file" = "$path" || continue
      if printf '%s\n' "$rest" | grep -qE -- "$pattern"; then
        allowed=true
        break
      fi
    done <"$allowlist_file"
    if test "$allowed" = false; then
      printf 'audit-identity: unlisted former-identity match: %s\n' "$line" >&2
      status=1
    fi
  done <<<"$matches"
fi

renamed_paths="$(git ls-files | grep -iE "$DETECT" || true)"
if test -n "$renamed_paths"; then
  printf 'audit-identity: tracked paths still use the former identity:\n%s\n' "$renamed_paths" >&2
  status=1
fi

if test "$status" -eq 0; then
  printf 'audit-identity: OK; every former-identity match is allowlisted\n'
fi
exit "$status"
