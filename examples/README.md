# CodeSplice examples

These executable examples teach the released `v0.1.0` CLI with small, complete
workspaces. Every mutation starts from `before/`, uses the checked-in
`request.json`, and compares the result byte-for-byte with `expected/`. The
runner never edits these fixtures: it works only below ignored `examples/.work/`.

Run one example from the repository root:

```bash
examples/run.sh 01-move-lines
```

Set `CODESPLICE_BIN=/absolute/path/to/codesplice` to exercise a particular
binary. Otherwise the runner uses an installed `codesplice`, or builds the local
debug binary if none is installed. Each run prints the scratch workspace and a
`reports/` directory containing the actual inspect, preview, and commit reports.
Plan digests, workspace identity hashes, transaction IDs, and some warnings are
machine-specific, so reports are generated rather than misleadingly frozen.

Run the anti-rot check:

```bash
scripts/check-examples.sh
```

Changing commits are supported only on the two qualified `v0.1.0` rows: macOS
arm64 with local APFS and Linux x86_64 with local ext4. The examples do not
expand that support or the trusted-user, content-only threat model.

## Feature matrix

| Feature | Example |
|---|---|
| Version, capabilities, protocol/plan versions | [`00-discover`](00-discover/) |
| `inspect`, including an absent destination | [`01-move-lines`](01-move-lines/), [`02-copy-bytes-new-file`](02-copy-bytes-new-file/) |
| Human diff, JSON preview, `--no-diff` syntax | [`01-move-lines`](01-move-lines/) |
| Commit with previewed `--expect-plan` | Every changing example |
| Request file and `--request -` standard input | [`01-move-lines`](01-move-lines/), [`02-copy-bytes-new-file`](02-copy-bytes-new-file/) |
| `move` and one-based inclusive `lines` selector | [`01-move-lines`](01-move-lines/) |
| `copy` and half-open `bytes` selector | [`02-copy-bytes-new-file`](02-copy-bytes-new-file/) |
| Existing SHA-256 and absent `must_not_exist` preconditions | [`01-move-lines`](01-move-lines/), [`02-copy-bytes-new-file`](02-copy-bytes-new-file/) |
| All anchors: `file_start`, `file_end`, `before_line`, `after_line`, `byte_offset` | [`03-all-anchors`](03-all-anchors/) |
| Same-file reorder and immutable coordinates | [`04-same-file-reorder`](04-same-file-reorder/) |
| Same-file move no-op and no transaction | [`05-same-file-no-op`](05-same-file-no-op/) |
| Multi-operation, three-target split | [`06-multi-target-split`](06-multi-target-split/) |
| CRLF, lone CR, LF, NUL, and non-UTF-8 byte exactness | [`07-exact-bytes`](07-exact-bytes/) |
| Stale digest, plan mismatch, path escape, and symlink rejection | [`08-safe-failures`](08-safe-failures/) |
| `recover --list`, status/action guidance, qualified interruption boundary | [`09-recovery`](09-recovery/) |
| Read-only LSP selection, `request_source` composition, and guarded commit | [`10-lsp-semantic-selection`](10-lsp-semantic-selection/) |

`--accept-current-plan` is a human convenience in the grammar, but these
examples model the safer preview-and-bind workflow used by coding agents. They
therefore always commit with `--expect-plan`.

The semantic-selection example has separate user-installed language-server
prerequisites, so it is intentionally not part of `scripts/check-examples.sh`.
Its README provides deterministic offline fixture validation and runnable Rust,
Python, TypeScript, and Swift cases.

Hexadecimal fixture files are used only where Git-friendly text cannot represent
mixed terminators or non-UTF-8 bytes reliably. For example,
`before/src/source.bin.hex` materializes as `src/source.bin`; the golden expected
tree uses the same convention.
