# Semantic selection protocol version 1

`response.schema.json` and `error.schema.json` are the normative JSON Schema
Draft 2020-12 contracts for the standalone, read-only semantic-selection
surface. They do not modify CodeSplice edit protocol version 1, plan-hash
version 1, capability output, or the frozen edit error and warning registries.

Successful responses carry `selection_protocol_version: 1`. Selection errors
that occur after CLI parsing carry the same version and use the independent
selection error registry. Top-level command-line grammar failures occur before
selection dispatch and retain the global `INVALID_CLI` response.

## Success contract

The response describes the immutable source snapshot sent to the language
server, the normalized query, the negotiated server identity and position
encoding, and zero or more deterministic matches. A successful `--all` query
may contain no matches; without `--all`, zero and multiple matches are errors.

Name queries serialize as `{"kind":"name",...}`. Position queries always
serialize their validated, zero-based snapshot byte offset as
`{"kind":"position","byte_offset":...}`, including when the user supplied a
one-based line and Unicode-scalar column. `symbol_kind` is always present and is
`null` when no kind filter was requested.

`lsp_range` and `lsp_selection_range` preserve the raw, zero-based LSP
coordinates returned by the server. `selector` is the authoritative validated
CodeSplice byte range after applying the requested extent. `symbol_path`
contains the complete breadcrumb, including the selected symbol as its final
element.

`request_source` normatively references the frozen edit protocol v1 `source`
definition. A consumer may insert it unchanged as an operation's `source` in a
protocol-v1 edit request. Offline schema validators must register
`../v1/request.schema.json` under its declared `https://codesplice.dev` schema
ID; validation must not depend on a network fetch. Runtime validation
additionally guarantees all of the following relationships, which JSON Schema
cannot express:

- `request_source.path` equals top-level `source.path`;
- `request_source.selector` equals the match's `selector`;
- `request_source.precondition.value` equals top-level `source.sha256`;
- `selected_byte_length` equals `selector.end - selector.start`;
- `selected_payload_sha256` hashes exactly the selected snapshot bytes;
- each selector is nonempty, ordered, and contained by `source.byte_length`;
- `lsp_selection_range` is well formed and contained by `lsp_range`.

The server object never exposes a process command, arguments, environment,
initialization options, stderr, or an absolute workspace path. Its nullable
fields distinguish an explicit server program with no configuration ID and an
LSP server that omitted `serverInfo`.

The warning array reuses the existing `WarningDto` object shape but permits
only the already-registered `OBSERVATION_MAY_BE_STALE` warning. Selection v1
does not extend the frozen edit warning registry.

## Error contract

Selection error responses have strict top-level fields: code, category,
retryability, message, and bounded structured context. The schema enforces the
category and retryability associated with every code. Process exit status is a
CLI property and is not serialized in each error response. The complete mapping
is frozen both as the schema's `x-codesplice-error-registry` annotation and as a
golden registry.

| Code | Category | Exit | Retryable |
|---|---|---:|---|
| `INVALID_SELECTION_QUERY` | request | 2 | false |
| `LSP_SERVER_NOT_CONFIGURED` | support | 4 | false |
| `SELECTION_NOT_FOUND` | conflict | 3 | false |
| `SELECTION_AMBIGUOUS` | conflict | 3 | false |
| `UNSUPPORTED_TEXT_ENCODING` | support | 4 | false |
| `LSP_CAPABILITY_UNAVAILABLE` | support | 4 | false |
| `LSP_FLAT_SYMBOLS_UNSUPPORTED` | support | 4 | false |
| `LSP_DOCUMENT_SYNC_UNAVAILABLE` | support | 4 | false |
| `LSP_RESOURCE_LIMIT_EXCEEDED` | support | 4 | false |
| `LSP_TIMEOUT` | support | 4 | true |
| `LSP_START_FAILED` | support | 4 | true |
| `LSP_EXITED` | support | 4 | true |
| `LSP_PROTOCOL_ERROR` | support | 4 | false |
| `LSP_REQUEST_FAILED` | support | 4 | true |
| `SELECTION_INTERNAL_ERROR` | internal | 8 | false |

`SELECTION_AMBIGUOUS` has a strict context containing the total candidate
count and up to 50 deterministic candidate summaries. Other error contexts are
code-specific implementation data and must remain bounded and non-sensitive;
they must not include source text, complete stderr, environment variables,
initialization options, or absolute paths.

## Limits and validation

Schema `maxLength` counts Unicode characters, not encoded bytes. Runtime code
must enforce the corresponding UTF-8 byte limits before allocation and must
also enforce the global 16 MiB serialized-response limit, JSON nesting depth,
checked arithmetic, and duplicate-key rejection. The response-specific release
caps are 1000 successful matches, 50 ambiguity candidate summaries, 256
symbol-breadcrumb elements, and 16 observation
warnings. Lower configured limits are permitted, but implementations must not
emit a response that exceeds these versioned maxima.

The golden vectors in `tests/golden/selection-v1` are hand-authored contract
examples. `composition-selection.json`, `composition-request-source.json`, and
`composition-edit-request.json` intentionally contain the same source fragment;
tests must extract and compare it structurally, parse the edit request with the
unchanged protocol-v1 parser, and preview it against a matching fixture
workspace.
