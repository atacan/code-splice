# 03 — Use every destination anchor

Five copies from one immutable source demonstrate every anchor against existing
files. Coordinates always resolve against the initial workspace, even when a
batch has multiple operations.

| Destination | Anchor | Resolved boundary |
|---|---|---|
| `start.txt` | `file_start` | byte 0 |
| `end.txt` | `file_end` | original EOF |
| `before.txt` | `before_line(2)` | first byte of line 2 |
| `after.txt` | `after_line(1)` | byte after line 1's terminator |
| `offset.txt` | `byte_offset(1)` | between `<` and `>` |

```bash
examples/run.sh 03-all-anchors
```

Line anchors are one-based. `byte_offset` accepts any boundary from zero through
the initial file length and can deliberately split a text or binary sequence.
For an absent destination, only `file_start`, `file_end`, and `byte_offset(0)`
are valid.

