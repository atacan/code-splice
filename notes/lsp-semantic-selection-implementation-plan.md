# LSP-backed semantic selection implementation plan

Status: proposed; independent architecture review incorporated

Scope: read-only source selection through an installed Language Server Protocol (LSP) server

Editing protocol impact: none; protocol v1 `lines` and `bytes` selectors remain unchanged

## 1. Decision summary

Add a read-only `codesplice select` command that asks an installed language server for document symbols, resolves one symbol to a byte range in an immutable CodeSplice snapshot, and emits a source fragment that can be placed into an ordinary protocol-v1 `move` or `copy` request.

The language server identifies structure. CodeSplice remains responsible for:

- acquiring and hashing the exact source bytes;
- converting LSP positions to byte offsets;
- validating and bounding all server output;
- reporting ambiguity rather than guessing;
- previewing, hashing, and committing the eventual edit; and
- enforcing all existing path, precondition, transaction, and recovery guarantees.

This feature will use LSP only. It will not bundle grammars, load parser plugins, or provide a parser-based fallback.

## 2. Goals

- Select named functions, methods, classes, structs, interfaces, enums, and other standardized LSP symbol kinds.
- Select the smallest matching declaration that contains a user-provided source position.
- Work with language servers already installed on the user's machine.
- Start a private language-server process for CodeSplice instead of attaching to an editor-owned process.
- Give agents a deterministic, machine-readable result containing a protocol-v1 byte selector and source SHA-256 precondition.
- Keep selection read-only and independently versioned from the frozen edit protocol.
- Preserve CodeSplice's fail-closed behavior for stale files, malformed responses, resource exhaustion, ambiguity, and process failures.

## 3. Non-goals

- Natural-language interpretation inside the CLI. An agent may translate “this function” into a name, kind, or source position.
- Sharing or attaching to a language-server instance owned by an editor. LSP does not define portable discovery, multiplexing, or document ownership for that scenario.
- Asking a language server to edit files, apply workspace edits, format code, update imports, or perform refactors.
- Guaranteeing that every installed server implements document symbols consistently.
- Selecting non-UTF-8 source through LSP.
- Adding a semantic selector to protocol v1 or changing plan-hash version 1.
- Maintaining a CodeSplice language-server package registry in the first release.
- Keeping a language server alive across separate CLI invocations. A daemon may be considered later from measured startup data.

## 4. User-facing command

### 4.1 Query by name

```console
codesplice --workspace . select \
  --path crates/codesplice-protocol/src/lib.rs \
  --name parse_request \
  --kind function \
  --json
```

Names are exact and case-sensitive in version 1. `--kind` is optional, but omitting it can make a query ambiguous. A future version may add explicit match modes without changing the default.

### 4.2 Query by containing position

```console
codesplice --workspace . select \
  --path crates/codesplice-protocol/src/lib.rs \
  --at-byte 42711 \
  --kind function \
  --json
```

`--at-byte` is a zero-based insertion offset into the captured source and is the canonical exact-position interface. For human use, `--at-line` may be combined with `--at-column`; both are one-based, and the column counts Unicode scalar insertion positions rather than bytes or negotiated LSP code units. `--at-column` defaults to 1. CodeSplice first converts either input form to a snapshot byte offset, independently converts that offset to the encoding negotiated with the server, and then chooses the smallest symbol range containing it after applying the optional kind filter. The two position forms are mutually exclusive.

User-supplied byte, line, and scalar-column positions must be in range and are never clamped. Clamping described later applies only when normalizing server-returned LSP positions according to the LSP contract.

### 4.3 Server selection

The safe escape hatch is explicit and does not invoke a shell:

```console
codesplice --workspace . select \
  --path src/lib.rs \
  --name run \
  --server-program rust-analyzer \
  --server-arg=--some-server-option \
  --language-id rust \
  --json
```

`--server-id` selects a trusted user or built-in descriptor. `--server-program` is an executable name or explicit path passed to `std::process::Command` and requires `--language-id`; it cannot be combined with `--server-id`. Each `--server-arg` is one literal argument. Shell command strings, interpolation, and command substitution are not supported.

Resolution precedence is:

1. explicit `--server-id` or `--server-program`, arguments, and language ID;
2. trusted user-level CodeSplice configuration;
3. a small built-in table of conventional executable names and language IDs;
4. otherwise fail with `LSP_SERVER_NOT_CONFIGURED` and show an example configuration.

Workspace-local server configuration is not loaded automatically. If added later, it must require an explicit trust option because it can cause arbitrary executable launch.

### 4.4 Ambiguity behavior

The default command requires exactly one match:

- zero matches: `SELECTION_NOT_FOUND`;
- more than one match: `SELECTION_AMBIGUOUS`, with a bounded candidate list;
- exactly one match: success.

An explicit `--all` option returns all bounded matches successfully for discovery. Candidate ordering is deterministic: resolved start byte, end byte, symbol kind, symbol path, then name.

## 5. Selection protocol v1

Selection is a separate, read-only protocol surface. Do not modify the frozen protocol-v1 request schema, response schema, capability response, error registry, or plan-hash encoding.

Add:

```text
docs/schema/selection-v1/response.schema.json
docs/schema/selection-v1/error.schema.json
docs/schema/selection-v1/README.md
tests/golden/selection-v1/
```

Example successful response:

```json
{
  "selection_protocol_version": 1,
  "workspace_identity_hash": "sha256:...",
  "source": {
    "path": "crates/codesplice-protocol/src/lib.rs",
    "sha256": "sha256:...",
    "byte_length": 81234
  },
  "query": {
    "kind": "name",
    "name": "parse_request",
    "symbol_kind": "function"
  },
  "server": {
    "configuration_id": "rust",
    "reported_name": "rust-analyzer",
    "reported_version": "...",
    "position_encoding": "utf-8"
  },
  "matches": [
    {
      "name": "parse_request",
      "symbol_kind": "function",
      "symbol_path": ["parse_request"],
      "detail": null,
      "lsp_range": {
        "start": {"line": 1364, "character": 0},
        "end": {"line": 1417, "character": 1}
      },
      "extent": "declaration_lines",
      "selector": {"kind": "bytes", "start": 42100, "end": 43793},
      "selected_payload_sha256": "sha256:...",
      "selected_byte_length": 1693,
      "request_source": {
        "path": "crates/codesplice-protocol/src/lib.rs",
        "selector": {"kind": "bytes", "start": 42100, "end": 43793},
        "precondition": {"kind": "sha256", "value": "sha256:..."}
      }
    }
  ],
  "warnings": []
}
```

`lsp_range` preserves the raw zero-based LSP line/character coordinates for audit. The byte selector is the authoritative CodeSplice coordinate.

The emitted `matches[0].request_source` is directly composable, without field transformation, into the `source` member of an ordinary edit request:

```json
{
  "path": "crates/codesplice-protocol/src/lib.rs",
  "selector": {"kind": "bytes", "start": 42100, "end": 43793},
  "precondition": {"kind": "sha256", "value": "sha256:..."}
}
```

The top-level source and per-match selector remain useful for review, while `request_source` is the copy-ready protocol-v1 fragment. Golden composition tests must insert it unchanged into a complete edit request and successfully parse and preview that request. Do not place server command lines, initialization options, stderr, absolute workspace paths, or file contents in the JSON response.

The `warnings` array reuses the existing frozen `WarningDto` only for warnings already defined by CodeSplice, initially `OBSERVATION_MAY_BE_STALE` from snapshot acquisition. Selection v1 introduces no new warning codes. Any future selection-specific warning requires an explicit selection-contract revision rather than modifying the frozen edit warning registry.

## 6. Declaration extent

LSP `DocumentSymbol.range` excludes leading and trailing whitespace. Using it literally will often omit indentation and the terminating newline, which is inconvenient for exact code movement.

Selection v1 therefore defines two language-independent extents:

- `symbol`: use the converted `DocumentSymbol.range` exactly;
- `declaration_lines`: expand the range to complete logical lines when doing so consumes only whitespace outside the LSP range.

`declaration_lines` is the default. The expansion algorithm is:

1. Inspect bytes from the start of the physical line to the LSP start offset. Expand left only when all bytes are spaces or tabs.
2. Inspect bytes from the LSP end offset to the next logical line terminator. Expand right through that terminator only when the intervening bytes are spaces or tabs.
3. If the LSP range already ends at the start of a logical line because it consumed the preceding terminator, do not consume that following line or a following blank line.
4. Preserve the original LF, CRLF, lone-CR, or absent final terminator exactly.
5. If either side contains non-whitespace bytes, keep that side at the LSP boundary.
6. Require the resulting range to be nonempty and within the immutable snapshot.

The response reports the original LSP range, selected extent, and final byte selector so preview remains auditable. `--extent symbol` exposes the unexpanded behavior.

No language-specific comment, decorator, or attribute inference is performed. CodeSplice relies on the server's symbol range for those constructs.

## 7. Workspace integration

### 7.1 Snapshot acquisition

Add a read-only filesystem API that captures existing paths without requiring a caller-supplied digest, for example:

```rust
pub fn acquire_existing_snapshot(
    &self,
    paths: &[String],
    limits: SnapshotLimits,
) -> Result<WorkspaceSnapshot, FsError>;
```

It must reuse the existing normalized path walk, stable-read retry, alias detection, file-type checks, line index, identity retention, and resource accounting. It must fail when a requested path is absent. This avoids the inefficient `inspect`-then-conditioned-snapshot double read.

The CLI acquires the diagnostic lock, scans recovery state, captures the source snapshot, and then releases the lock before starting an external process. The captured bytes, digest, identities, and observation warnings remain immutable inputs to selection. A slow or indexing language server therefore cannot make unrelated CodeSplice commits fail with `TRANSACTION_BUSY`; staleness after capture is handled by the emitted SHA-256 precondition. Add a concurrency test in which a slow fake server does not block a CodeSplice commit after snapshot capture.

### 7.2 Exact server input

Initialize the server with the canonical configured project root as both `rootUri` and the single `workspaceFolders` entry. After initialization, send `textDocument/didOpen` with:

- a canonical `file:` URI for the captured source path;
- the configured LSP language ID;
- a fixed document version for the session; and
- the exact UTF-8 snapshot text.

Language features must be requested for the synchronized document. Do not rely only on the language server rereading the disk path. The source SHA in the response always refers to the CodeSplice snapshot, regardless of later workspace changes.

Before sending `didOpen`, require usable synchronization capability. Accept legacy `TextDocumentSyncKind::Full` and `Incremental`. For `TextDocumentSyncOptions`, require `openClose: true`; reject `None`, omitted synchronization, or `openClose: false` with `LSP_DOCUMENT_SYNC_UNAVAILABLE`. Tests must prove the server receives the captured text when the disk file changes after capture.

### 7.3 Existing edit workflow

`select` does not create an edit plan or a commit token. The caller still performs:

```text
select -> construct protocol-v1 request -> preview -> commit --expect-plan
```

If the source changes after selection, the ordinary SHA-256 precondition rejects the later preview or commit. No LSP server runs during `apply`.

## 8. Rust crate and module design

### 8.1 New `codesplice-lsp` crate

Add a workspace library crate with `#![forbid(unsafe_code)]` and `#![deny(missing_docs)]`.

Suggested modules:

```text
crates/codesplice-lsp/src/
  lib.rs
  config.rs       # validated server descriptors and language matching
  error.rs        # typed LspError and SelectionError
  jsonrpc.rs      # bounded Content-Length framing and message IDs
  process.rs      # child launch, pipe ownership, stderr drain, cleanup
  client.rs       # initialize/open/request/close/shutdown lifecycle
  position.rs     # LSP position <-> snapshot byte conversion
  symbols.rs      # tolerant LSP DTOs and hierarchy flattening
  resolve.rs      # query filtering, ambiguity, extent, deterministic ordering
```

Keep this crate independent of `codesplice-fs`. It accepts borrowed snapshot bytes and a canonical URI, and returns typed matches containing `codesplice_core::ByteRange`. Filesystem access and protocol rendering remain CLI concerns.

Use a concrete LSP client implementation internally. Do not introduce trait objects merely for hypothetical alternate parsers or transports. A narrow test-only transport trait is acceptable if it materially simplifies deterministic unit tests; prefer generics at that seam.

### 8.2 `codesplice-protocol` additions

Add `SelectionResponse` and a concrete `SelectionErrorDto`, strict serialization tests, schemas, and golden vectors. These types are independent of `ProtocolVersionResponse`, `CapabilitiesResponse::v0_1_0`, edit `ErrorDto`, edit `ErrorCode`, and protocol-v1 request parsing. Do not generalize the frozen edit error types solely to reuse them here.

Selection-specific errors may preserve the existing numeric exit categories while using a separate code enum. Initial codes:

- request/configuration: `INVALID_SELECTION_QUERY`, `LSP_SERVER_NOT_CONFIGURED`;
- conflict: `SELECTION_NOT_FOUND`, `SELECTION_AMBIGUOUS`;
- support: `UNSUPPORTED_TEXT_ENCODING`, `LSP_CAPABILITY_UNAVAILABLE`, `LSP_FLAT_SYMBOLS_UNSUPPORTED`, `LSP_DOCUMENT_SYNC_UNAVAILABLE`;
- resource/support: `LSP_RESOURCE_LIMIT_EXCEEDED`, `LSP_TIMEOUT`;
- external process/protocol: `LSP_START_FAILED`, `LSP_EXITED`, `LSP_PROTOCOL_ERROR`, `LSP_REQUEST_FAILED`;
- internal: `SELECTION_INTERNAL_ERROR`.

Do not add these variants to the frozen edit `ErrorCode::ALL` registry.

Freeze this mapping in the selection error schema and golden vectors:

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

Top-level Clap grammar failures remain the existing global `INVALID_CLI` behavior because they occur before command dispatch. `INVALID_SELECTION_QUERY` is reserved for semantic query validation after successful CLI parsing.

### 8.3 CLI additions

Add `Command::Select(SelectArgs)` and route it to a dedicated module:

```text
crates/codesplice-cli/src/select.rs
```

The module owns orchestration only:

1. validate mutually exclusive query modes;
2. open the workspace and acquire the diagnostic context;
3. capture one existing source file;
4. retain the captured observation warnings and release the diagnostic lock;
5. load and resolve the server configuration;
6. call `codesplice-lsp`;
7. digest each final selected range;
8. build the selection-v1 response, enforce response limits, and render JSON or human output.

Refactor the current top-level error rendering only as far as required to support an independently versioned selection error envelope. Avoid coupling selection failures to edit protocol DTOs.

## 9. Generic LSP client behavior

### 9.1 Process model

Use `std::process::Command` with piped stdin, stdout, and stderr. The initial implementation does not require an async runtime:

- one reader thread parses bounded stdout frames and sends typed events over a small `std::sync::mpsc::sync_channel` whose capacity and cumulative queued bytes are bounded;
- one writer thread owns child stdin and consumes a bounded outbound queue, so a server that stops reading cannot trap the orchestration thread inside `write_all` past its deadline;
- one stderr thread continuously drains into a bounded diagnostic tail so a noisy server cannot block;
- every I/O thread reports completion over a bounded status channel, allowing orchestration to enforce deadlines without a blocking join;
- the orchestration thread owns request IDs, queues frames, handles server-initiated requests, checks deadlines, observes child exit, and never attempts to close stdin while the writer thread owns it;
- on supported Unix platforms, the direct server starts in a dedicated process group; and
- an explicit fallible lifecycle method terminates or waits for the process before joining all I/O threads, while `Drop` provides only bounded best-effort cleanup.

On normal completion, send `shutdown`, wait for its response, queue `exit`, and drop the outbound sender. Until the shutdown deadline, observe writer completion and direct-child exit without performing a blocking join. If the deadline expires, terminate the dedicated process group. On any failure, drop all channel senders and the inbound receiver so blocked queue operations wake, terminate the process group immediately, and skip the graceful wait. In every path, wait for and reap the direct child before joining the writer, stdout-reader, and stderr-reader threads; reaping closes the child-side pipes and unblocks pending I/O. The process-group boundary covers ordinary wrapper-spawned helpers; CodeSplice cannot guarantee cleanup of a malicious or independently daemonized descendant, which remains inside the trusted-language-server boundary. Test a fake server that spawns a child and ignores shutdown.

### 9.2 Lifecycle sequence

The minimum successful sequence is:

```text
spawn
  -> initialize request
  <- InitializeResult
  -> initialized notification
  -> workspace/didChangeConfiguration notification (when configured)
  -> textDocument/didOpen notification
  -> textDocument/documentSymbol request
  <- DocumentSymbol[] | null
  -> textDocument/didClose notification
  -> shutdown request
  <- null
  -> exit notification
  -> reap
```

Advertise hierarchical document-symbol support and position encodings in this preference order:

```text
utf-8, utf-16, utf-32
```

If the server does not report a selected encoding, use the LSP-required `utf-16` default.

Advertise `dynamicRegistration: false`, `workDoneProgress: false`, and `workspace.applyEdit: false`. Selection v1 uses only statically initialized capabilities; it does not maintain a dynamic-registration state machine.

### 9.3 Server-initiated messages

While waiting for a response, the client must continue consuming notifications and respond to server requests. Implement at least:

- `workspace/workspaceFolders`: return the single configured project root;
- `workspace/configuration`: return exactly one bounded result per request item by resolving its optional `section` against configured settings;
- `window/showMessageRequest`: return `null`;
- `workspace/applyEdit`: return `{ "applied": false, "failureReason": "CodeSplice selection is read-only" }`;
- unknown requests: return JSON-RPC `MethodNotFound`.

Bound and ignore ordinary logging, diagnostics, telemetry, and progress notifications unless needed for a useful error tail. A language server must never be allowed to turn selection into an edit.

### 9.4 Capability handling

After initialization:

- require a hierarchical `DocumentSymbol[]` response because its `range` is defined to enclose the declaration;
- reject legacy flat `SymbolInformation[]` with `LSP_FLAT_SYMBOLS_UNSUPPORTED`, because its reveal range need not describe an AST node or full declaration;
- fail with `LSP_CAPABILITY_UNAVAILABLE` when document symbols are not supported;
- do not silently switch to another structural-selection request or technology.

Selection v1 requires static `documentSymbolProvider` and usable text synchronization; this keeps response semantics uniform and the exact `didOpen` snapshot authoritative.

### 9.5 JSON-RPC validation

Treat stdout as an untrusted framed stream:

- require exactly one unsigned decimal `Content-Length` header, rejecting a sign, overflow, duplicate, or missing length;
- require ASCII header bytes and reject unsupported `Content-Type` charsets while accepting the specified `utf-8` spelling and legacy `utf8` compatibility spelling;
- reject JSON-RPC batches, messages with an invalid `jsonrpc` version, and responses containing both or neither of `result` and `error`;
- accept integer or string request IDs from the server, but bound their encoded size;
- reject duplicate or unknown completed client-request IDs; and
- bound unknown method names and parameters before returning `MethodNotFound`.

Every framing or validation failure enters the same explicit cleanup path and joins all I/O threads.

## 10. Position and range conversion

Position conversion is a correctness boundary and must be implemented over the immutable snapshot bytes.

Requirements:

- LSP lines and characters are zero-based. CLI line/scalar-column positions are one-based, while `--at-byte` is zero-based.
- Support negotiated UTF-8, UTF-16, and UTF-32 code-unit counts.
- Treat LF, CRLF, lone CR, and an unterminated final line consistently with `LineIndex`.
- Reject nonexistent line numbers. As required by LSP, silently normalize a character offset beyond a valid line's content length to the content end; never count CR or LF bytes as line content.
- Reject positions that split a UTF-8 code point or UTF-16 surrogate pair.
- Reject reversed, empty, or out-of-file symbol ranges after normalization.
- Validate that every `DocumentSymbol.selectionRange` is well formed and contained by its `range`, even though selection uses the enclosing `range`.
- Convert every candidate before filtering by containing position so malformed server output cannot hide behind a query filter.
- Charge conversion work and candidate storage against explicit limits.

Reuse or extend `codesplice_core::LineIndex` for physical line boundaries. Keep Unicode code-unit conversion in `codesplice-lsp::position`, where it can be exhaustively tested without filesystem access.

## 11. Symbol resolution

Flatten hierarchical document symbols iteratively while retaining a `Vec<String>` full breadcrumb that includes all containing symbols and the selected symbol itself, for example `["OuterType", "InnerType", "method"]`. Map the standardized LSP `SymbolKind` integers to stable lowercase names. Preserve unknown numeric kinds as `unknown` plus the numeric value in internal diagnostics; never panic on future server values.

Name query:

1. retain candidates whose `name` exactly equals the query;
2. apply the optional standardized kind filter;
3. deduplicate identical `(range, kind, symbol_path, name)` records;
4. sort deterministically;
5. enforce unique-or-`--all` behavior.

Position query:

1. validate `--at-byte` directly or convert the CLI line/scalar-column position to a snapshot byte offset;
2. retain symbol ranges containing that position using `start <= position < end`; when the queried position is EOF, also permit `position == end == file_length` for a nonempty symbol ending at EOF;
3. apply the optional kind filter;
4. sort by increasing byte length, then deterministic candidate ordering;
5. select the smallest range, failing as ambiguous if equally small distinct symbols remain.

Do not infer a qualified-name separator. Return `symbol_path` as an array because separators are language-specific.

## 12. Configuration and discovery

Define a versioned user configuration document, separate from the edit protocol. A representative shape is:

```toml
version = 1

[[servers]]
id = "rust"
extensions = ["rs"]
language_id = "rust"
program = "rust-analyzer"
args = []
initialization_options = {}
settings = {}
project_root = "."
allow_workspace_program = false
startup_timeout_ms = 10000
request_timeout_ms = 30000
```

Validation rules:

- server IDs and extensions are nonempty and unique after normalization;
- `program` is one executable value, never a shell expression;
- argument count, argument bytes, JSON depth, and JSON bytes are bounded;
- the complete configuration file has a byte and nesting-depth limit;
- timeouts have conservative lower and upper limits;
- duplicate extension matches require explicit `--server-id` selection;
- `project_root` is a normalized workspace-relative directory whose canonical target must remain inside the CodeSplice workspace;
- `allow_workspace_program` defaults to false and is permitted only in a trusted user descriptor;
- environment replacement is not supported initially; a small allowlisted environment overlay may be added later;
- no secrets or configuration contents are echoed in normal responses.

Use `$CODESPLICE_CONFIG` when explicitly set; otherwise use the platform user-configuration location (`$XDG_CONFIG_HOME/codesplice/config.toml` or its standard fallback on Linux, and `~/Library/Application Support/CodeSplice/config.toml` on macOS). Configuration loading must not create directories or files. After `initialized`, send `workspace/didChangeConfiguration` when settings are configured. For `workspace/configuration`, resolve each requested section independently.

Executable discovery rules are:

- built-in and automatically selected descriptors search only absolute `PATH` entries;
- ignore empty and relative `PATH` entries during automatic discovery;
- resolve the executable before changing the child's working directory;
- require the resolved target to be a regular executable file;
- reject any automatically resolved executable whose canonical path is inside the CodeSplice workspace; built-in descriptors can never override this; and
- allow relative or workspace-local executables only through explicit `--server-program` or a trusted user descriptor with `allow_workspace_program = true`.

Add PATH-poisoning tests, including `.`, an empty `PATH` component, an absolute workspace `bin` entry, and a workspace executable shadowing a system server. Automatic discovery must skip or reject every workspace-local candidate.

A read-only diagnostic command may be designed after selection v1 ships; it is not part of the implementation phases in this plan. A likely shape is:

```console
codesplice lsp doctor --path src/lib.rs --json
```

It would report which descriptor matched, whether the executable resolved, its reported server name/version after initialization, negotiated encoding, and document-symbol capability. It must not open or inspect unrelated source files.

## 13. Resource limits and security

Add lowerable limits with below/at/above boundary tests:

- maximum LSP header bytes;
- maximum LSP source bytes, initially recommended as 8 MiB rather than the filesystem-wide 256 MiB maximum;
- separate maximum inbound and outbound JSON-RPC message bytes, initially recommended as 16 MiB inbound and 64 MiB outbound;
- maximum JSON nesting depth;
- maximum pending request count;
- maximum server-initiated requests per selection;
- maximum notifications per selection;
- maximum inbound and outbound channel capacity and cumulative queued event bytes;
- maximum stderr bytes retained;
- maximum document symbols before and after flattening;
- maximum symbol nesting depth;
- maximum name, detail, and symbol-path bytes;
- maximum candidates included in an ambiguity error;
- startup, initialize, document-symbol, shutdown, and total wall-clock deadlines.

Preflight the exactly serialized `didOpen` message against the outbound frame limit before queuing any bytes. Defaults must fit inside the existing 16 MiB serialized-response limit, and the dedicated source limit must be lower than the 256 MiB general snapshot-file limit. All cumulative accounting uses checked arithmetic before allocation. Add below/at/above tests for source bytes, JSON escaping expansion, inbound and outbound frames, queued bytes, and channel saturation.

Security rules:

- never launch through a shell;
- do not automatically trust workspace-local executable configuration;
- drop or reject inherited environment variables only if a documented compatibility study supports doing so; otherwise inherit the environment and document the trusted-local-server assumption;
- set the child working directory to the canonical configured project root, which is constrained inside the CodeSplice workspace;
- construct canonical project-root and source `file:` URIs with a proven URI implementation rather than hand-written percent encoding; test spaces, `#`, `%`, and non-ASCII path components;
- reject `workspace/applyEdit` and any other server request that would mutate user files;
- redact absolute paths and bounded stderr from structured errors;
- visibly escape control and bidirectional characters in human diagnostics;
- reap the child on every exit path;
- document that an installed language server is trusted code and may independently create caches, indexes, or compiler artifacts.

## 14. Error handling

`codesplice-lsp` exposes typed library errors using `Result<T, LspError>` and `Result<T, SelectionError>`. Do not use `anyhow` or erased boxed errors in library APIs. Production paths must not use `unwrap`, `expect`, or panic for malformed external input.

Errors should retain structured, non-sensitive context such as:

- server configuration ID;
- lifecycle phase;
- JSON-RPC method and request ID;
- elapsed time and configured limit;
- reported capability state;
- received versus supported position encoding;
- candidate count and bounded candidate summaries.

Do not retain source text, complete server stderr, initialization options, environment variables, or absolute paths in error DTOs.

## 15. Implementation phases

### Phase 0: contract and fixtures

- [ ] Approve CLI grammar, response/error schemas, extent semantics, and initial limits.
- [ ] Add selection-v1 schemas and hand-authored golden examples.
- [ ] Add a fake language-server fixture executable to `codesplice-test-support`.
- [ ] Freeze representative JSON-RPC transcripts for initialization, configuration, document synchronization, document symbols, and shutdown.

Exit criterion: the external behavior is reviewable before production implementation begins.

### Phase 1: framed transport and process lifecycle

- [ ] Add `codesplice-lsp` and workspace dependencies.
- [ ] Implement separately bounded inbound/outbound `Content-Length` framing, a bounded event channel, a writer thread, and exact outbound-message preflight.
- [ ] Implement request IDs, response correlation, server-request dispatch, stderr draining, deadlines, dedicated process groups, explicit shutdown, thread joins, and child reaping.
- [ ] Test malformed headers, oversized frames, invalid JSON, unknown response IDs, channel saturation, blocked stdin/stdout/stderr, early exit, timeout, descendant cleanup, and forced cleanup.

Exit criterion: the fake server can complete and fail every lifecycle path without leaking a process or thread.

### Phase 2: initialization and document synchronization

- [ ] Implement initialize/initialized/shutdown/exit.
- [ ] Advertise hierarchical symbols, all supported position encodings, no dynamic registration or progress, and no apply-edit capability.
- [ ] Implement workspace/configuration and workspaceFolders responses.
- [ ] Validate text-synchronization capability and implement didOpen/didClose with exact snapshot text.
- [ ] Send bounded workspace/didChangeConfiguration when settings are configured.
- [ ] Reject workspace/applyEdit.

Exit criterion: CodeSplice can negotiate capabilities and open a captured document against the fake server.

### Phase 3: position conversion and symbol resolution

- [ ] Implement UTF-8/UTF-16/UTF-32 conversion.
- [ ] Support hierarchical document-symbol responses and reject flat `SymbolInformation[]` without emitting a selector.
- [ ] Implement standardized kind mapping, name queries, position queries, deterministic ordering, deduplication, and ambiguity.
- [ ] Implement `symbol` and `declaration_lines` extents.
- [ ] Validate `selectionRange` containment and add property tests for Unicode, CR/LF combinations, range normalization, containment, and conversion round trips.

Exit criterion: every accepted match yields a validated, nonempty `ByteRange` over the original snapshot.

### Phase 4: filesystem and CLI integration

- [ ] Add single-read unconditioned existing-file snapshot acquisition.
- [ ] Add `Command::Select`, argument validation, configuration loading, and server discovery.
- [ ] Release the diagnostic context after snapshot capture and prove a slow server does not block a later CodeSplice commit.
- [ ] Compute source and selected-payload digests.
- [ ] Render selection-v1 JSON and human output with exact response limits.

Exit criterion: a fake-server integration test emits a source fragment accepted unchanged by protocol-v1 request parsing and preview.

### Phase 5: hardening and compatibility

- [ ] Add below/at/above tests for every new resource limit.
- [ ] Add server-request flood, notification flood, deep symbol tree, flat-response rejection, duplicate symbol, malformed range, invalid selectionRange, and non-UTF-8 tests.
- [ ] Add source-size, JSON-expansion, frame-size, queue-byte, channel-saturation, and PATH-poisoning tests.
- [ ] Verify no mutation command is sent or accepted during selection.
- [ ] Add optional, non-gating qualification tests for a small set of real installed servers; CI correctness must continue to rely on fake servers.
- [ ] Measure startup and document-symbol latency before considering batching or a daemon.

Exit criterion: all failure modes are typed, bounded, deterministic, and leave the workspace untouched by CodeSplice.

### Phase 6: documentation and release

- [ ] Document configuration, discovery, trust, range semantics, ambiguity, and composition with edit requests.
- [ ] Update `README.md`, `docs/protocol.md`, `docs/resource-limits.md`, `docs/security.md`, and agent integration guidance.
- [ ] Add runnable examples using a fake or explicitly supplied server.
- [ ] Report selection support through a new independently versioned discovery surface; do not change the frozen v0.1.0 capabilities JSON.
- [ ] Add release notes identifying supported platforms and the trusted-language-server boundary.

Exit criterion: a user can configure an installed server, select a symbol, preview the resulting move, and commit it using only documented commands.

## 16. Test matrix

| Area | Required cases |
|---|---|
| Framing | split headers/bodies, duplicate or missing `Content-Length`, sign/overflow, non-ASCII headers, supported/unsupported charset, oversized inbound/outbound frame, EOF mid-frame, batch rejection |
| JSON-RPC | integer/string server request IDs, both/neither result and error, duplicate/unknown completed ID, bounded unknown method/params |
| Backpressure | exact didOpen preflight, JSON escaping expansion, saturated inbound/outbound channels, blocked child stdin, bounded queued bytes |
| Lifecycle | successful shutdown, initialize failure, early exit, timeout, ignored notification, server request during client request, process-group termination, thread joins after framing failure |
| Capabilities | static hierarchical symbols, no symbol support, flat-only response, unknown capabilities, full/incremental/no text synchronization |
| Symbol responses | hierarchy, rejected flat list, null, empty, duplicate, deep nesting, unknown kind, excessive candidates, invalid selectionRange containment |
| Queries | exact name, kind filter, containing position, nested symbols, zero/one/many matches, deterministic order |
| Unicode positions | ASCII, multibyte UTF-8, supplementary characters in UTF-16, UTF-32, invalid mid-code-unit positions |
| Lines | LF, CRLF, lone CR, mixed terminators, unterminated final line, empty file |
| Extent | indented declaration, trailing spaces, final newline, range already ending at next-line column zero, inline declaration, non-whitespace prefix/suffix |
| URI/discovery | spaces, `#`, `%`, non-ASCII paths, relative/empty PATH entries, workspace executable shadowing |
| Safety | non-UTF-8 input, stale source after selection, slow selection concurrent with commit, rejected applyEdit, redaction, stderr flood, descendant cleanup |
| Composition | emitted `request_source` inserted unchanged into a protocol-v1 request and successfully parsed and previewed |

## 17. Acceptance criteria

The first release is complete when all of the following are true:

1. No grammar or parser is bundled or required by CodeSplice.
2. A configured installed LSP server can resolve a named or containing declaration.
3. The result contains a validated protocol-v1 byte selector and source SHA-256.
4. Unicode and line-ending conversions are exact and exhaustively tested.
5. Ambiguity, unsupported capabilities, malformed responses, timeouts, and server exits fail closed.
6. Selection never accepts a server-requested edit and never mutates through CodeSplice.
7. Protocol v1, plan-hash version 1, transaction records, and frozen capability output are byte-for-byte unchanged.
8. The direct server and ordinary same-process-group descendants terminate, all I/O threads join, and the direct child is reaped on success and failure.
9. Existing format, Clippy, test, build, golden-vector, and platform qualification suites remain green.

## 18. Open decisions to settle in Phase 0

- Exact default startup and request deadlines after measuring representative servers. Recommended starting bounds: 10 seconds for initialization, 30 seconds for document symbols, and 5 seconds for graceful shutdown.
- Whether the first release should ship built-in executable-name descriptors or require explicit/user configuration. Recommended: built-in descriptors only where invocation is stable and covered by qualification tests.
- Confirm the recommended 8 MiB source, 16 MiB inbound-frame, 64 MiB outbound-frame, and small fixed channel limits against measured serialization and server behavior.

## 19. Primary references

- [LSP 3.18 specification](https://microsoft.github.io/language-server-protocol/specifications/lsp/3.18/specification/)
- [Document Symbols Request](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/language/documentSymbol.md)
- [Initialize Request and capability negotiation](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/general/initialize.md)
- [Position encoding](https://github.com/microsoft/language-server-protocol/blob/gh-pages/_specifications/lsp/3.18/types/position.md)
