# Semantic source selection

Use `select` only to discover an exact source range. It is read-only and does
not change the protocol-v1 inspect, preview, and guarded-commit workflow.

First confirm the selection surface and an appropriate trusted language server:

```bash
codesplice selection-capabilities --json
codesplice --workspace /absolute/repo select \
  --path src/lib.rs --name parse_request --kind function --json \
  > selection.json
```

Use exactly one query: exact case-sensitive unqualified `--name`; zero-based
UTF-8-boundary `--at-byte`; or one-based `--at-line` with optional one-based
Unicode-scalar `--at-column` (default 1). Add `--kind` to disambiguate. Prefer a
precise position over choosing the first ambiguous result. Use `--all` only when
the task explicitly needs every match; it may return none.

The default `--extent declaration_lines` suits moving a standalone declaration.
Use `--extent symbol` for the language server's exact symbol range.

Require one intended match, then copy `matches[0].request_source` unchanged into
the operation's `source`:

```bash
jq -n --slurpfile selection selection.json --arg sha "$DESTINATION_SHA" '{
  protocol_version: 1,
  operations: [{
    kind: "move",
    source: $selection[0].matches[0].request_source,
    destination: {
      path: "src/destination.rs",
      anchor: {kind: "file_end"},
      precondition: {kind: "sha256", value: $sha}
    }
  }]
}' > request.json
```

Do not recalculate its byte selector or source digest. Inspect the destination,
compose the request structurally, then follow [workflow.md](workflow.md). If the
source changes, rerun selection instead of weakening its precondition.

Automatic discovery supports installed trusted server descriptors; language
servers are not bundled or sandboxed. Treat `--server-program` and user-enabled
workspace programs as explicit trust decisions. See the repository's
`docs/agent-integration.md` for complete composition and server configuration,
and `examples/10-lsp-semantic-selection/` for Rust, Python, TypeScript, and Swift.
