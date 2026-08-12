# Editing specification

CodeSplice moves or copies bytes from one immutable initial workspace snapshot.
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

The exact version-1 deterministic CBOR plan-record schema remains authoritative
in Section 7.4 of the implementation plan until Phase 4 copies its finalized
discriminant table and golden vectors here.
