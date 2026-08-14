# 10 — Semantic selection with installed language servers

This example selects declarations from an immutable workspace snapshot, copies
each response's `request_source` object unchanged into a normal protocol-v1
request, then uses the usual inspect, preview, digest, and guarded-commit
workflow. The Swift case composes four selections into one move request. It
runs only in an ignored disposable workspace below
`examples/.work/`; source fixtures are never modified.

Run one language from the repository root:

```bash
examples/10-lsp-semantic-selection/run.sh rust
examples/10-lsp-semantic-selection/run.sh python
examples/10-lsp-semantic-selection/run.sh typescript
examples/10-lsp-semantic-selection/run.sh swift
```

The script writes `inspect.json`, `selection.json`, generated `request.json`,
`preview.json`, and `commit.json` under its reported `reports/` directory, then
compares the resulting workspace with the checked-in expected tree. It is not
included in the offline example suite: language servers are user-installed,
and CodeSplice does not claim that all four are installed or compatible on a
given machine.

## Server prerequisites and selection commands

Rust, Python, and TypeScript exercise trusted built-in discovery. Their
programs must be installed in an absolute, trusted `PATH` entry; discovery
rejects workspace-local candidates and does not run a shell. Swift uses the
explicit-program escape hatch because SourceKit-LSP is not a CodeSplice built-in
descriptor. That explicit command is a user trust decision.

| Language | Install or expose on `PATH` | Exact selection invocation |
|---|---|---|
| Rust | `rust-analyzer` | `codesplice --workspace WORKSPACE select --path src/lib.rs --name select_greeting --kind function --json` |
| Python | `pylsp` (python-lsp-server) | `codesplice --workspace WORKSPACE select --path src/example.py --name select_greeting --kind function --json` |
| TypeScript | `typescript-language-server` (and its TypeScript runtime) | `codesplice --workspace WORKSPACE select --path src/example.ts --name selectGreeting --kind function --json` |
| Swift | A `sourcekit-lsp` build that starts its stdio server when launched directly | Four position queries at lines 4, 7, 13, and 18; see `run.sh` |

The Swift fixture moves a protocol, its conforming struct, an extension of the
struct, and an extension of the protocol into four new files. It intentionally
uses position queries without `--kind`: SourceKit-LSP display names and the
symbol-kind mapping for protocols/extensions vary by release, while each
declaration-opening position is unambiguous here. `--at-line` is one-based and
`--at-column` counts Unicode scalar insertion columns.
SourceKit-LSP packaging varies between toolchains; an executable that exposes
only diagnostic/debug subcommands is not a compatible stdio server. Point the
runner at another trusted build with
`CODESPLICE_SWIFT_LSP=/absolute/path/to/sourcekit-lsp`.

You can inspect the static, server-independent contract before installing a
server:

```bash
codesplice selection-capabilities --json
```

## Composition and mixed-time safety

The helper makes protocol-v1 move operations; it assigns every
`matches[0].request_source` directly from its selection report, without changing
the path, byte selector, or SHA-256 precondition. The selected bytes are then
*moved* to deliberately absent destinations:

```json
{
  "protocol_version": 1,
  "operations": [{
    "kind": "move",
    "source": "matches[0].request_source (copied as an object)",
    "destination": {
      "path": "src/extracted.rs",
      "anchor": {"kind": "file_start"},
      "precondition": {"kind": "must_not_exist"}
    }
  }]
}
```

The source's selection-time SHA-256 is what matters when `apply` runs later:
if a person or tool changes the source after selection, preview or commit fails
its precondition instead of copying from a mixed-time snapshot. Selection itself
is read-only with respect to CodeSplice's workspace edits and transaction state;
the language server remains a separately trusted, installed program. Review
`preview.json`, extract its `plan_sha256`, and commit only with
`--expect-plan`, as the runner does. The Swift run has four operations in the
same request, so the preview and commit cover the whole reorganization.

CodeSplice does not write while it selects, but the server is not sandboxed and
may create its own build or index state. The runner excludes only known
server/tool artifact directories (`.build`, `.swiftpm`, and `target`) plus
CodeSplice's own `.codesplice` control tree from its final tree comparison; it
still compares every checked-in fixture file byte-for-byte, including the
source after the moves and every new destination.

For a manual run, replace `WORKSPACE` with a disposable copy of one `before/`
fixture, run the table's selection command, and compose the response:

```bash
python3 examples/10-lsp-semantic-selection/compose-request.py \
  selection.json src/extracted.rs > request.json
codesplice --workspace WORKSPACE inspect --path src/lib.rs --path src/extracted.rs --json
codesplice --workspace WORKSPACE apply --request request.json --preview --json > preview.json
PLAN_SHA=$(sed -n 's/.*"plan_sha256":"\([^"]*\)".*/\1/p' preview.json)
codesplice --workspace WORKSPACE apply --request request.json --commit --expect-plan "$PLAN_SHA" --json
```

The `src/` paths in the final snippet are the Rust case; use the table and the
corresponding destination path from `run.sh` for the other three languages.
For Swift, pass four `SELECTION_JSON DESTINATION_PATH` pairs to
`compose-request.py`; the runner shows the exact order.

## Offline fixture validation

No LSP needs to be installed to check the script and composition helper:

```bash
bash -n examples/10-lsp-semantic-selection/run.sh
python3 -m py_compile examples/10-lsp-semantic-selection/compose-request.py
python3 examples/10-lsp-semantic-selection/verify-fixtures.py
```
