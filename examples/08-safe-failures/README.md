# 08 — Fail closed without changing the workspace

These independent checks demonstrate representative rejection boundaries:

| Case | Expected exit | Stable code |
|---|---:|---|
| stale source digest | 3 | `PRECONDITION_FAILED` |
| deliberately wrong `--expect-plan` | 3 | `EXPECTED_PLAN_MISMATCH` |
| `..` path escape | 2 | `INVALID_REQUEST` |
| inspection of a symlink | 4 | `SYMLINK_NOT_ALLOWED` |

```bash
examples/run.sh 08-safe-failures
```

The runner asserts each JSON error and compares the workspace with `expected/`.
The plan-mismatch case first performs a valid preview, then supplies an all-zero
plan digest to commit. A real client should instead pass the exact digest from
preview; on any mismatch, inspect and preview again.

CodeSplice is a trusted-user tool, not a hostile-filesystem security boundary.
These fail-closed checks do not claim protection against a malicious same-user
process racing workspace namespace changes.

