# 01 — Move lines to an existing file

This moves lines 3 through 5 (one-based, both endpoints included) from
`src/source.rs` to the end of `src/destination.rs`. Line selection includes each
selected line's original terminator. Both paths exist, so each endpoint is bound
to the SHA-256 digest of its initial bytes.

```bash
codesplice --workspace examples/.work/01-move-lines/workspace inspect \
  --path src/source.rs --path src/destination.rs --json

codesplice --workspace examples/.work/01-move-lines/workspace apply \
  --request examples/01-move-lines/request.json --preview

codesplice --workspace examples/.work/01-move-lines/workspace apply \
  --request examples/01-move-lines/request.json --preview --json

codesplice --workspace examples/.work/01-move-lines/workspace apply \
  --request examples/01-move-lines/request.json --preview --json --no-diff

codesplice --workspace examples/.work/01-move-lines/workspace apply \
  --request examples/01-move-lines/request.json --commit \
  --expect-plan sha256:PLAN_FROM_PREVIEW --json
```

Let the runner create the scratch workspace, substitute the actual previewed
plan, commit, and compare the full resulting tree:

```bash
examples/run.sh 01-move-lines
```

`--no-diff` omits only diff detail; it does not change the resolved edits or plan
digest. A changing commit creates a recoverable transaction and may replace
metadata outside the documented content/permission contract.

