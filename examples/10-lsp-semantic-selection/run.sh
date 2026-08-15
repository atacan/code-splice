#!/usr/bin/env bash
set -euo pipefail

example_dir=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
repo_dir=$(CDPATH= cd -- "$example_dir/../.." && pwd)

usage() {
  printf '%s\n' \
    "usage: examples/10-lsp-semantic-selection/run.sh <rust|python|typescript|swift>" \
    "" \
    "Runs in examples/.work/10-lsp-semantic-selection/<language>." \
    "It requires the language server documented in README.md."
}

language=${1:-}
case "$language" in
  rust)
    source_path=src/lib.rs
    selection_files=(trait.json struct.json inherent-impl.json trait-impl.json)
    destination_paths=(src/greets.rs src/person.rs src/person_inherent.rs src/person_greets.rs)
    prerequisite=rust-analyzer
    ;;
  python)
    source_path=src/example.py
    selection_files=(protocol.json person.json greeting-adapter.json uppercase-greeting-adapter.json)
    destination_paths=(src/named.py src/person.py src/greeting_adapter.py src/uppercase_greeting_adapter.py)
    prerequisite=pylsp
    ;;
  typescript)
    source_path=src/example.ts
    selection_files=(interface.json class.json namespace.json formatter.json)
    destination_paths=(src/named.ts src/person.ts src/person-namespace.ts src/format-greeting.ts)
    prerequisite=typescript-language-server
    ;;
  swift)
    swift_lsp=${CODESPLICE_SWIFT_LSP:-sourcekit-lsp}
    source_path=Sources/SemanticDemo/SemanticDemo.swift
    # Position queries avoid relying on SourceKit-LSP's display names or its
    # varying protocol/extension kind mapping. Each line is a declaration's
    # opening `public` keyword in the checked-in fixture.
    selection_files=(
      protocol.json
      struct.json
      struct-extension.json
      protocol-extension.json
    )
    destination_paths=(
      Sources/SemanticDemo/DisplayNamed.swift
      Sources/SemanticDemo/Account.swift
      Sources/SemanticDemo/Account+Greeting.swift
      Sources/SemanticDemo/DisplayNamed+Formatting.swift
    )
    prerequisite=$swift_lsp
    ;;
  -h|--help|"") usage; exit 0 ;;
  *) usage >&2; exit 2 ;;
esac

if ! command -v "$prerequisite" >/dev/null 2>&1; then
  printf 'required language server is not on PATH: %s\n' "$prerequisite" >&2
  exit 1
fi

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

work_root="$repo_dir/examples/.work/10-lsp-semantic-selection"
case_root="$work_root/$language"
case "$case_root" in
  "$work_root"/rust|"$work_root"/python|"$work_root"/typescript|"$work_root"/swift) ;;
  *) printf 'refusing unsafe work path: %s\n' "$case_root" >&2; exit 1 ;;
esac
if [[ -L $work_root || -L $case_root ]]; then
  printf 'refusing symbolic-link work path\n' >&2
  exit 1
fi
mkdir -p "$work_root"
rm -rf -- "$case_root"
mkdir -p "$case_root/reports"

workspace="$case_root/workspace"
reports="$case_root/reports"
cp -R "$example_dir/before/$language" "$workspace"

# Inspect before mutation, including every intentionally absent destination.
inspect_paths=(--path "$source_path")
for destination_path in "${destination_paths[@]}"; do
  inspect_paths+=(--path "$destination_path")
done
"$codesplice_bin" --workspace "$workspace" inspect "${inspect_paths[@]}" --json > "$reports/inspect.json"

# Selection is read-only. The commands are intentionally explicit so each
# example shows exactly when a stable name/kind or a position query is used.
case "$language" in
  rust)
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --name Greets --kind interface --json > "$reports/trait.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --name Person --kind struct --json > "$reports/struct.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --at-line 10 --at-column 1 --json > "$reports/inherent-impl.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --at-line 15 --at-column 1 --json > "$reports/trait-impl.json"
    ;;
  python)
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --name Named --kind class --json > "$reports/protocol.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --name Person --kind class --json > "$reports/person.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --name GreetingAdapter --kind class --json > "$reports/greeting-adapter.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --name UppercaseGreetingAdapter --kind class --json \
      > "$reports/uppercase-greeting-adapter.json"
    ;;
  typescript)
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --name Named --kind interface --json > "$reports/interface.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --name Person --kind class --json > "$reports/class.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --at-line 10 --at-column 1 --json > "$reports/namespace.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --name formatGreeting --kind function --json > "$reports/formatter.json"
    ;;
  swift)
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --at-line 4 --at-column 1 --server-program "$swift_lsp" --language-id swift --json \
      > "$reports/protocol.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --at-line 7 --at-column 1 --server-program "$swift_lsp" --language-id swift --json \
      > "$reports/struct.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --at-line 13 --at-column 1 --server-program "$swift_lsp" --language-id swift --json \
      > "$reports/struct-extension.json"
    "$codesplice_bin" --workspace "$workspace" select --path "$source_path" \
      --at-line 18 --at-column 1 --server-program "$swift_lsp" --language-id swift --json \
      > "$reports/protocol-extension.json"
    ;;
esac

# This copies every `matches[0].request_source` without changing it into
# protocol-v1. Each language composes four independently discovered
# declarations into one guarded multi-operation move.
compose_args=()
for index in "${!selection_files[@]}"; do
  compose_args+=("$reports/${selection_files[$index]}" "${destination_paths[$index]}")
done
python3 "$example_dir/compose-request.py" "${compose_args[@]}" > "$reports/request.json"
"$codesplice_bin" --workspace "$workspace" apply --request "$reports/request.json" \
  --preview --json > "$reports/preview.json"
plan=$(sed -n 's/.*"plan_sha256":"\([^"]*\)".*/\1/p' "$reports/preview.json")
if [[ ! $plan =~ ^sha256:[0-9a-f]{64}$ ]]; then
  printf 'preview did not return a valid plan digest\n' >&2
  exit 1
fi
"$codesplice_bin" --workspace "$workspace" apply --request "$reports/request.json" \
  --commit --expect-plan "$plan" --json > "$reports/commit.json"

# Language servers are independently trusted programs, not sandboxed by
# CodeSplice. These known build/index directories are server/tool artifacts;
# all checked-in fixture files, including the selected source and destination,
# remain under byte-for-byte comparison.
diff -r -x .build -x .codesplice -x .swiftpm -x target -- \
  "$example_dir/expected/$language" "$workspace"
printf 'PASS semantic selection (%s)\nworkspace: %s\nreports: %s\n' \
  "$language" "$workspace" "$reports"
