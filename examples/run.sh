#!/usr/bin/env bash
set -euo pipefail

examples_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH= cd -- "$examples_dir/.." && pwd)

usage() {
  printf '%s\n' \
    "usage: examples/run.sh <example>" \
    "" \
    "examples:" \
    "  00-discover" \
    "  01-move-lines" \
    "  02-copy-bytes-new-file" \
    "  03-all-anchors" \
    "  04-same-file-reorder" \
    "  05-same-file-no-op" \
    "  06-multi-target-split" \
    "  07-exact-bytes" \
    "  08-safe-failures" \
    "  09-recovery"
}

scenario=${1:-}
case "$scenario" in
  00-discover|01-move-lines|02-copy-bytes-new-file|03-all-anchors|04-same-file-reorder|05-same-file-no-op|06-multi-target-split|07-exact-bytes|08-safe-failures|09-recovery) ;;
  -h|--help|"") usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac

if [[ -n ${CODESPLICE_BIN:-} ]]; then
  codesplice_bin=$CODESPLICE_BIN
elif command -v codesplice >/dev/null 2>&1; then
  codesplice_bin=$(command -v codesplice)
else
  cargo build --manifest-path "$repo_dir/Cargo.toml" -p codesplice-cli
  codesplice_bin="$repo_dir/target/debug/codesplice"
fi
if [[ ! -x $codesplice_bin ]]; then
  printf 'codesplice executable is not executable: %s\n' "$codesplice_bin" >&2
  exit 1
fi

work_root="$examples_dir/.work"
case_root="$work_root/$scenario"
case "$case_root" in
  "$examples_dir"/.work/00-discover|"$examples_dir"/.work/01-move-lines|"$examples_dir"/.work/02-copy-bytes-new-file|"$examples_dir"/.work/03-all-anchors|"$examples_dir"/.work/04-same-file-reorder|"$examples_dir"/.work/05-same-file-no-op|"$examples_dir"/.work/06-multi-target-split|"$examples_dir"/.work/07-exact-bytes|"$examples_dir"/.work/08-safe-failures|"$examples_dir"/.work/09-recovery) ;;
  *) printf 'refusing unsafe work path: %s\n' "$case_root" >&2; exit 1 ;;
esac
if [[ -L $work_root ]]; then
  printf 'refusing symbolic-link work root: %s\n' "$work_root" >&2
  exit 1
fi
mkdir -p "$work_root"
if [[ -L $case_root ]]; then
  printf 'refusing to replace a symbolic-link work path: %s\n' "$case_root" >&2
  exit 1
fi
rm -rf -- "$case_root"
mkdir -p "$case_root/reports"

example_dir="$examples_dir/$scenario"
reports="$case_root/reports"

materialize() {
  python3 "$examples_dir/materialize.py" "$1" "$2"
}

extract_plan() {
  sed -n 's/.*"plan_sha256":"\([^"]*\)".*/\1/p' "$1"
}

assert_code() {
  local expected_exit=$1
  local expected_code=$2
  local report=$3
  local actual_exit=$4
  if [[ $actual_exit -ne $expected_exit ]]; then
    printf 'expected exit %s, got %s for %s\n' "$expected_exit" "$actual_exit" "$report" >&2
    return 1
  fi
  grep -Fq "\"code\":\"$expected_code\"" "$report"
}

preview_and_commit() {
  local request=$1
  shift
  "$codesplice_bin" --workspace "$workspace" inspect "$@" --json > "$reports/inspect.json"
  "$codesplice_bin" --workspace "$workspace" apply --request "$request" --preview > "$reports/preview.txt"
  "$codesplice_bin" --workspace "$workspace" apply --request "$request" --preview --json > "$reports/preview.json"
  "$codesplice_bin" --workspace "$workspace" apply --request "$request" --preview --json --no-diff > "$reports/preview-no-diff.json"
  local plan
  plan=$(extract_plan "$reports/preview.json")
  if [[ ! $plan =~ ^sha256:[0-9a-f]{64}$ ]]; then
    printf 'preview did not return a valid plan digest\n' >&2
    return 1
  fi
  "$codesplice_bin" --workspace "$workspace" apply --request "$request" --commit --expect-plan "$plan" --json > "$reports/commit.json"
}

compare_expected() {
  materialize "$example_dir/expected" "$case_root/expected"
  diff -r -x .codesplice -- "$case_root/expected" "$workspace"
}

if [[ $scenario == 00-discover ]]; then
  "$codesplice_bin" --version > "$reports/version.txt"
  "$codesplice_bin" capabilities --json > "$reports/capabilities.json"
  "$codesplice_bin" protocol-version --json > "$reports/protocol-version.json"
elif [[ $scenario == 08-safe-failures ]]; then
  materialize "$example_dir/before" "$case_root/workspace"
  workspace="$case_root/workspace"

  set +e
  "$codesplice_bin" --workspace "$workspace" apply --request "$example_dir/stale-precondition.json" --preview --json > "$reports/stale-precondition.json"
  status=$?
  set -e
  assert_code 3 PRECONDITION_FAILED "$reports/stale-precondition.json" "$status"

  "$codesplice_bin" --workspace "$workspace" apply --request "$example_dir/plan-mismatch.json" --preview --json > "$reports/preview.json"
  set +e
  "$codesplice_bin" --workspace "$workspace" apply --request "$example_dir/plan-mismatch.json" --commit --expect-plan "sha256:0000000000000000000000000000000000000000000000000000000000000000" --json > "$reports/plan-mismatch.json"
  status=$?
  set -e
  assert_code 3 EXPECTED_PLAN_MISMATCH "$reports/plan-mismatch.json" "$status"

  set +e
  "$codesplice_bin" --workspace "$workspace" apply --request "$example_dir/path-escape.json" --preview --json > "$reports/path-escape.json"
  status=$?
  set -e
  assert_code 2 INVALID_REQUEST "$reports/path-escape.json" "$status"

  ln -s source.txt "$workspace/link.txt"
  set +e
  "$codesplice_bin" --workspace "$workspace" inspect --path link.txt --json > "$reports/symlink.json"
  status=$?
  set -e
  assert_code 4 SYMLINK_NOT_ALLOWED "$reports/symlink.json" "$status"
  rm -- "$workspace/link.txt"
  compare_expected
elif [[ $scenario == 09-recovery ]]; then
  materialize "$example_dir/before" "$case_root/workspace"
  workspace="$case_root/workspace"
  "$codesplice_bin" --workspace "$workspace" recover --list > "$reports/list.txt"
  "$codesplice_bin" --workspace "$workspace" recover --list --json > "$reports/list.json"
  grep -Fq '"transactions":[]' "$reports/list.json"
  if [[ -e $workspace/.codesplice ]]; then
    printf 'read-only recovery list unexpectedly created a control tree\n' >&2
    exit 1
  fi
else
  materialize "$example_dir/before" "$case_root/workspace"
  workspace="$case_root/workspace"
  request="$example_dir/request.json"
  case "$scenario" in
    01-move-lines)
      preview_and_commit "$request" --path src/source.rs --path src/destination.rs
      grep -Fq '"diff":{"kind":"omitted"' "$reports/preview-no-diff.json"
      ;;
    02-copy-bytes-new-file)
      "$codesplice_bin" --workspace "$workspace" inspect --path src/source.txt --path src/copied.txt --json > "$reports/inspect.json"
      "$codesplice_bin" --workspace "$workspace" apply --request - --preview --json < "$request" > "$reports/preview.json"
      plan=$(extract_plan "$reports/preview.json")
      "$codesplice_bin" --workspace "$workspace" apply --request - --commit --expect-plan "$plan" --json < "$request" > "$reports/commit.json"
      ;;
    03-all-anchors) preview_and_commit "$request" --path src/source.txt --path src/start.txt --path src/end.txt --path src/before.txt --path src/after.txt --path src/offset.txt ;;
    04-same-file-reorder) preview_and_commit "$request" --path src/order.txt ;;
    05-same-file-no-op)
      preview_and_commit "$request" --path src/order.txt
      grep -Fq '"transaction_id":null' "$reports/commit.json"
      plan=$(extract_plan "$reports/preview.json")
      "$codesplice_bin" --workspace "$workspace" apply --request "$request" --commit --expect-plan "$plan" > "$reports/commit.txt"
      if [[ -e $workspace/.codesplice ]]; then
        printf 'same-file no-op unexpectedly created a control tree\n' >&2
        exit 1
      fi
      ;;
    06-multi-target-split) preview_and_commit "$request" --path src/source.rs --path src/one.rs --path src/two.rs ;;
    07-exact-bytes) preview_and_commit "$request" --path src/mixed.txt --path src/mixed-destination.txt --path src/source.bin --path src/destination.bin ;;
  esac
  compare_expected
fi

printf 'PASS %s\nworkspace: %s\nreports: %s\n' "$scenario" "${workspace:-none}" "$reports"
