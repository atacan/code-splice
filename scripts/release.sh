#!/usr/bin/env bash
set -euo pipefail

repository_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repository_root"
tag_pushed=0

usage() {
  printf 'usage: %s verify|publish MAJOR.MINOR.PATCH\n' "$0" >&2
}

fail() {
  printf 'release: %s\n' "$1" >&2
  if test "$tag_pushed" -eq 1; then
    printf 'release: the tag is immutable on origin; inspect the failed Release run and recover with:\n' >&2
    printf '  gh workflow run Release --ref main -f tag=%s\n' "$tag" >&2
  fi
  exit 1
}

require_command() {
  command -v "$1" >/dev/null 2>&1 || fail "required command is unavailable: $1"
}

if test "$#" -ne 2; then
  usage
  exit 2
fi

mode="$1"
version="$2"
case "$mode" in
  verify|publish) ;;
  *)
    usage
    exit 2
    ;;
esac

if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  fail "version must be a stable MAJOR.MINOR.PATCH value without a v prefix"
fi

tag="v${version}"
notes="docs/release-${tag}.md"
workspace_version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' Cargo.toml | head -n 1)"

test "$workspace_version" = "$version" \
  || fail "Cargo.toml version is ${workspace_version}, expected ${version}"
test -s "$notes" || fail "release notes are missing or empty: ${notes}"

require_command cargo
require_command git

git diff --check
cargo metadata --locked --no-deps --format-version 1 >/dev/null
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets --all-features \
  -- -D warnings -D clippy::perf
scripts/qualify-platform.sh
scripts/check-examples.sh
cargo build --locked --workspace --all-features

if test "$mode" = verify; then
  printf 'release: v%s passed local release verification\n' "$version"
  exit 0
fi

require_command gh
gh auth status >/dev/null

test -z "$(git status --porcelain)" \
  || fail "publish requires a clean working tree"
test "$(git branch --show-current)" = main \
  || fail "publish requires the main branch"

git fetch origin main --tags
head_sha="$(git rev-parse HEAD)"
remote_sha="$(git rev-parse origin/main)"
test "$head_sha" = "$remote_sha" \
  || fail "local main is not synchronized with origin/main"

ci_run="$(
  gh run list --workflow CI --commit "$head_sha" --event push --limit 1 \
    --json headSha,status,conclusion,url \
    --jq '.[0] | [.headSha, .status, .conclusion, .url] | @tsv' \
    2>/dev/null || true
)"
test -n "$ci_run" || fail "no CI push run found for ${head_sha}"
IFS=$'\t' read -r ci_sha ci_status ci_conclusion ci_url <<<"$ci_run"
test "$ci_sha" = "$head_sha" || fail "CI run does not match ${head_sha}"
test "$ci_status/$ci_conclusion" = completed/success \
  || fail "CI is not successful for ${head_sha}: ${ci_status}/${ci_conclusion} ${ci_url}"

if git show-ref --verify --quiet "refs/tags/${tag}"; then
  fail "local tag already exists: ${tag}"
fi
if git ls-remote --exit-code --tags origin "refs/tags/${tag}" >/dev/null 2>&1; then
  fail "remote tag already exists: ${tag}"
fi
if gh release view "$tag" >/dev/null 2>&1; then
  fail "GitHub release already exists: ${tag}"
fi

# Recheck the mutable preconditions immediately before creating the immutable
# release tag. A later failure never deletes or moves a tag that reached origin.
test -z "$(git status --porcelain)" || fail "working tree changed during verification"
git fetch origin main --tags
test "$(git rev-parse HEAD)" = "$(git rev-parse origin/main)" \
  || fail "origin/main changed during verification; rerun from the new main"

publish_failed() {
  status=$?
  if test "$tag_pushed" -eq 1; then
    printf 'release: %s is immutable on origin; inspect the failed Release run and recover with:\n' "$tag" >&2
    printf '  gh workflow run Release --ref main -f tag=%s\n' "$tag" >&2
  fi
  exit "$status"
}
trap publish_failed ERR

git tag -a "$tag" -F "$notes"
if ! git push origin "refs/tags/${tag}"; then
  git tag --delete "$tag" >/dev/null
  fail "failed to push release tag ${tag}; removed the unpushed local tag"
fi
tag_pushed=1

release_run=""
for _ in $(seq 1 60); do
  release_run="$(
    gh run list --workflow Release --commit "$head_sha" --event push --limit 5 \
      --json databaseId,headSha \
      --jq 'map(select(.headSha == "'"$head_sha"'")) | first | .databaseId // empty' \
      2>/dev/null || true
  )"
  test -n "$release_run" && break
  sleep 2
done
test -n "$release_run" || fail "Release workflow did not start for ${tag}"
gh run watch "$release_run" --exit-status

release_state="$(
  gh release view "$tag" --json isDraft,isPrerelease,tagName \
    --jq '[.tagName, .isDraft, .isPrerelease] | @tsv'
)"
test "$release_state" = "${tag}"$'\tfalse\tfalse' \
  || fail "published release state is unexpected: ${release_state}"

actual_assets="$(
  gh release view "$tag" --json assets --jq '.assets[].name' | LC_ALL=C sort
)"
expected_assets="$(
  printf '%s\n' \
    SHA256SUMS \
    "codesplice-v${version}-aarch64-apple-darwin.tar.gz" \
    "codesplice-v${version}-x86_64-unknown-linux-gnu.tar.gz" \
    | LC_ALL=C sort
)"
test "$actual_assets" = "$expected_assets" \
  || fail "published asset set does not match the release contract"

release_url="$(gh release view "$tag" --json url --jq .url)"
printf 'release: published and verified %s at %s\n' "$tag" "$release_url"
