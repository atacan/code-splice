# Fake LSP fixtures

This directory freezes the language-server traffic used to qualify semantic
selection. The fixtures are protocol data, not CodeSplice edit-protocol
goldens.

`transcripts/*.jsonl` contains one JSON object per line with a monotonic
`sequence`, a `direction`, and the exact JSON-RPC `message`. Tests serialize
each message with compact JSON and add `Content-Length` framing; whitespace in
the JSONL file itself is not part of the wire contract. The stable fixture URI
is `file:///fixture/workspace/source.rs`.

The `codesplice-fake-lsp` binary is built from the
`codesplice-test-support` package. A normal invocation is:

```console
codesplice-fake-lsp \
  --scenario success-with-configuration \
  --expected-document-uri file:///fixture/workspace/source.rs \
  --expected-language-id fixture-rust \
  --expected-document-text-file tests/fixtures/lsp/documents/source.rs
```

The fake server validates lifecycle order, a single matching root URI and
workspace folder, client capability declarations, document URI reuse, and any
configured exact `didOpen` expectations. It does not depend on
`codesplice-lsp`, so production protocol types can be tested against it without
a dependency cycle.

Named scenarios cover:

- successful sessions, with and without configuration;
- initialize errors, unknown IDs, invalid dual responses, early exits, and
  initialize/document-symbol/shutdown hangs;
- malformed framing, invalid JSON, stderr pressure, notification traffic,
  server-initiated requests, and same-process-group descendant cleanup;
- missing or invalid capabilities, legacy and options-based full/incremental
  synchronization, and UTF-8/UTF-16(default)/UTF-32/unsupported encodings; and
- hierarchical, flat, null, invalid-selection-range, unknown-kind, deep, and
  duplicate document-symbol results.

Keep successful-session changes intentional and review them alongside any
public selection-contract change.
