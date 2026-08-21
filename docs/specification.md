# Editing specification

srcmv moves or copies bytes from one immutable initial workspace snapshot.
It does not parse source languages, format output, normalize line endings, update
imports, or create parent directories.

For each effectful exact-mode operation, the SHA-256 digest of the selected bytes
must equal the SHA-256 digest of the inserted bytes. A same-file move anchored at
its own start or end is reported as a no-op and inserts nothing.

## Coordinates

Line numbers are one-based. Line selection includes both endpoint lines and their
original LF, CRLF, or lone-CR terminators. A nonempty unterminated final line is a
line; a trailing terminator creates no phantom line; an empty file has no lines.

- `lines(start, end)` requires `1 <= start <= end <= line_count`.
- `bytes(start, end)` selects `[start, end)` and requires
  `0 <= start < end <= file_length`.
- `file_start` and `file_end` select insertion boundaries.
- `before_line(n)` uses the first byte of line `n`.
- `after_line(n)` uses the byte after the complete line terminator, or EOF for an
  unterminated last line.
- `byte_offset(offset)` accepts `0..=file_length` and may split any byte sequence.

For a new file, only `file_start`, `file_end`, and `byte_offset(0)` are valid.

## Composition

Every selector and anchor resolves against the initial snapshot. Moves add a
deletion and an insertion of the original selected slice; copies add only the
insertion. Effectful move deletions cannot overlap, and an insertion strictly
inside a deletion is invalid. Insertions at deletion boundaries are valid.

Events sort by initial offset, event class, then request index. At one offset,
deletions end first, insertions occur in request order, and deletions then begin.
At EOF, surviving original bytes precede EOF insertions. Output recipes reference
snapshot slices and never require a complete materialized output buffer.

Actual byte equality determines whether an existing output changes, even when
edit events occurred. Only changed outputs become transaction targets.

## Preconditions and conflicts

Every existing path carries a lowercase `sha256:` digest precondition. A new
destination carries `must_not_exist`; a source never may. Repeated references to
one normalized path must use one precondition.

Out-of-range coordinates, overlapping move deletions, insertions inside deletions,
physical aliases, incompatible preconditions, and mixed absent/existing use are
conflicts. Commit revalidates all inputs, absences, parent identities, target link
counts, and the workspace root before the first mutation.

## Plan-hash version 1

The plan digest is:

```text
SHA256(ASCII "SRCMV-PLAN-V1\0" || deterministic_cbor(plan_record))
```

Encoding follows RFC 8949 deterministic CBOR. All containers have definite
lengths. Maps, tags, floats, negative integers, and indefinite-length items are
forbidden. Unsigned integers use their shortest representation. SHA-256 values
are 32-byte CBOR byte strings, not hexadecimal text.

The positional record is:

```text
plan_record = [
  1, 1, workspace_identity, input_records, resolved_operations, output_records
]

workspace_identity = [device_u64, inode_u64]
input_record = [normalized_path, state]
state = [0, parent_device_u64, parent_inode_u64]
      | [1, parent_device_u64, parent_inode_u64,
           file_device_u64, file_inode_u64, length_u64, sha256_bytes]

resolved_operation = [
  operation_index_u64, kind_u64, source_path, selector_record,
  source_precondition_record, source_start_u64, source_end_u64,
  selected_sha256_bytes, destination_path, anchor_record,
  destination_precondition_record, destination_offset_u64, effect_u64
]

output_record = [
  normalized_path, original_sha256_or_null, resulting_sha256_bytes,
  resulting_length_u64, change_kind_u64, segments
]

segment = [1, snapshot_input_index_u64, start_u64, end_u64]
        | [2, operation_index_u64, snapshot_input_index_u64, start_u64,
             end_u64, payload_sha256_bytes]
```

Input and output records sort by normalized-path UTF-8 bytes. Operations remain
in request order. Segment snapshot indices refer to positions in the sorted input
record array.

### Discriminants

| Record | Value | Meaning |
|---|---:|---|
| input state | `0` | absent path |
| input state | `1` | existing file |
| operation kind | `1` | move |
| operation kind | `2` | copy |
| selector | `1` | `[1, first_line, last_line]` |
| selector | `2` | `[2, start_byte, end_byte_exclusive]` |
| anchor | `1` | `[1]` file start |
| anchor | `2` | `[2]` file end |
| anchor | `3` | `[3, line]` before line |
| anchor | `4` | `[4, line]` after line |
| anchor | `5` | `[5, offset]` byte offset |
| precondition | `1` | `[1, sha256_bytes]` existing digest |
| precondition | `2` | `[2]` must not exist |
| operation effect | `1` | changed event stream |
| operation effect | `2` | same-file move no-op |
| output change | `1` | unchanged existing |
| output change | `2` | modified existing |
| output change | `3` | created new |
| output change | `4` | emptied existing |
| segment | `1` | original snapshot slice |
| segment | `2` | operation payload slice |

The annotated version-1 golden bytes and domain-separated digest are stored under
`tests/golden/plan-hash-v1/`.
