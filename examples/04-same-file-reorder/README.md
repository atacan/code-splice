# 04 — Reorder within one file

This moves line 3 to `file_start` in the same file. The selector and anchor both
resolve against the immutable initial bytes, so edits do not shift later
coordinates while the batch is planned.

```bash
examples/run.sh 04-same-file-reorder
```

The result is `gamma`, `alpha`, `beta`; no copied text appears in the request.

