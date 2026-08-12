# 05 — Recognize a same-file no-op

A move anchored at its selected range's own start or end is a defined no-op.
Here line 2 is moved `before_line(2)`, its own start.

```bash
examples/run.sh 05-same-file-no-op
```

Preview reports the operation effect as `no_op`. Commit still requires the
previewed plan digest, but returns a null transaction ID, changes no bytes, and
does not create `.codesplice/`.

