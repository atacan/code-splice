# Semantic-selection release note draft

This release adds `codesplice select`, a read-only bridge from an installed
Language Server Protocol server to CodeSplice's exact byte selectors. Callers
can select an exact, case-sensitive symbol name or the smallest declaration
containing a byte, line, and Unicode-scalar-column position. Optional symbol-kind
filters, deterministic ambiguity errors, `--all`, and `symbol` or
`declaration_lines` extents make the result explicit.

Language servers and language grammars are not bundled. Automatic discovery
provides convenience descriptors for installed Rust, Go, Python, C, C++,
JavaScript, and TypeScript servers, while trusted user TOML descriptors and the
shell-free `--server-program` interface cover other languages and server setups.
Automatic `PATH` discovery ignores relative and empty entries and refuses
workspace-local executables; explicit programs and descriptors opting into
`allow_workspace_program` are deliberate trust decisions.

Selection protocol version 1 is independent of the frozen edit protocol. Its
JSON response records the immutable UTF-8 source snapshot, normalized query,
negotiated UTF-8/UTF-16/UTF-32 position encoding, raw LSP ranges, authoritative
half-open byte selectors, and selected-payload digests. Every match includes a
`request_source` object that can be copied unchanged into a protocol-v1 `move`
or `copy` request. The existing inspect, preview, and
`commit --expect-plan` workflow is unchanged, and the embedded source digest
rejects a source file changed after selection.

The language server is a trusted external process running with the invoking
user's privileges, not a sandboxed plugin. CodeSplice captures one immutable
selected-file snapshot and releases its diagnostic lock before launching the
server, so an indexing server does not block unrelated commits. The server may
observe other project files later, producing a documented mixed-time project
view. CodeSplice refuses server-requested edits and sends no LSP mutation
command, but it cannot prevent a malicious executable from accessing the
filesystem directly.

Semantic selection is bounded across source size, framing, JSON, queues, symbol
count/depth/storage, position-conversion work, matches, configuration, output,
and lifecycle deadlines. Malformed server data, unsupported flat symbol lists,
invalid encodings or ranges, capability gaps, floods, exits, and timeouts fail
closed with a separate selection-v1 error registry.

The supported release rows remain Linux x86_64 on local ext4 and macOS arm64 on
local APFS. `scripts/qualify-lsp.sh` provides a non-gating smoke test for
installed `clangd` and `rust-analyzer`; bounded fake-server tests remain the
authoritative compatibility and failure qualification. This feature does not
change `capabilities --json`, protocol version 1, plan-hash version 1,
transaction-record version 1, or the frozen edit error and warning registries.
