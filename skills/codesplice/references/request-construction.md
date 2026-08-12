# Protocol-v1 request construction

## Request envelope

Use one or more operations in request order:

```json
{
  "protocol_version": 1,
  "operations": [
    {
      "kind": "move",
      "source": {
        "path": "src/source.rs",
        "selector": { "kind": "lines", "start": 4, "end": 7 },
        "precondition": {
          "kind": "sha256",
          "value": "sha256:0000000000000000000000000000000000000000000000000000000000000000"
        }
      },
      "destination": {
        "path": "src/destination.rs",
        "anchor": { "kind": "before_line", "line": 12 },
        "precondition": {
          "kind": "sha256",
          "value": "sha256:1111111111111111111111111111111111111111111111111111111111111111"
        }
      }
    }
  ]
}
```

Replace both example digests with current values returned by `inspect`. A digest is exactly `sha256:` plus 64 lowercase hexadecimal digits. Objects reject unknown and duplicate fields.

Use `"kind": "copy"` to retain the selected source bytes. Use `"kind": "move"` to delete the selected source bytes and insert that same initial-snapshot slice at the destination.

## Select source bytes

| Selector | JSON | Meaning |
|---|---|---|
| Lines | `{"kind":"lines","start":S,"end":E}` | One-based inclusive lines, including their original complete terminators |
| Bytes | `{"kind":"bytes","start":S,"end":E}` | Zero-based half-open byte interval `[S,E)` |

Require `1 <= start <= end <= line_count` for lines. Require `0 <= start < end <= byte_length` for bytes.

A nonempty unterminated final line counts as a line. A trailing terminator does not create a phantom line. An empty file has no lines. Line selection preserves LF, CRLF, lone CR, and missing final terminators exactly.

Use a byte selector for non-UTF-8 content or when a boundary must split an arbitrary byte sequence.

## Choose the destination boundary

| Anchor | JSON | Meaning |
|---|---|---|
| File start | `{"kind":"file_start"}` | Offset zero |
| File end | `{"kind":"file_end"}` | After all existing bytes |
| Before line | `{"kind":"before_line","line":N}` | First byte of line `N` |
| After line | `{"kind":"after_line","line":N}` | After that line's terminator, or EOF for an unterminated final line |
| Byte offset | `{"kind":"byte_offset","offset":N}` | Exact offset from `0` through file length |

For a new file, only `file_start`, `file_end`, and `byte_offset(0)` are valid. The parent directory must already exist.

## Express destination state

For an existing destination, use its inspected digest:

```json
{"kind":"sha256","value":"sha256:CURRENT_64_LOWERCASE_HEX_DIGEST"}
```

For an absent destination, use:

```json
{"kind":"must_not_exist"}
```

A source must always exist and therefore never uses `must_not_exist`. Every repeated reference to the same normalized path must carry the same precondition.

## Compose multiple edits

All selectors and anchors resolve against one immutable initial workspace snapshot, not against earlier operations in the array. Insertions at one offset occur in request order. Effectful move deletions cannot overlap, and an insertion strictly inside a deletion is rejected.

Batch operations when they must share one plan and recover as one transaction. Preview reports all changed outputs; only byte-different outputs become transaction targets.

A same-file move anchored at its selected range's own start or end is a defined no-op. Preview it and verify `effect: "no_op"`; do not assume other same-file rearrangements are no-ops.
