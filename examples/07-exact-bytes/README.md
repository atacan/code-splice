# 07 — Preserve mixed terminators and binary bytes

One batch performs two exact moves:

- lines 1–2 contain CRLF followed by a lone CR; they are appended without
  normalization to a CRLF destination;
- byte range `[1, 4)` contains `ff 00 fe`; it is appended to an ASCII binary
  prefix while leaving `41 42` in the source.

```bash
examples/run.sh 07-exact-bytes
```

The `.hex` fixtures are text encodings for review and Git portability. The runner
materializes them without the suffix before invoking srcmv, then decodes the
expected tree and performs a byte-for-byte comparison. Human/JSON preview labels
text terminators and summarizes binary differences without interpreting payloads
as UTF-8.

srcmv guarantees file-content bytes. It does not parse code, format output,
normalize line endings, or preserve metadata outside the released metadata
contract.

