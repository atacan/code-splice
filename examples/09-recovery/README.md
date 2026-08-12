# 09 — Inspect and resolve recovery state

Recovery is normally empty. Listing it is read-only and does not create a
`.codesplice/` control tree:

```bash
codesplice --workspace examples/.work/09-recovery/workspace recover --list
codesplice --workspace examples/.work/09-recovery/workspace recover --list --json
examples/run.sh 09-recovery
```

If a changing commit is interrupted, first list and inspect the validated
transaction; use the opaque ID exactly as reported:

```bash
codesplice --workspace /path/to/workspace recover --list --json
codesplice --workspace /path/to/workspace recover TRANSACTION_ID --status --json
```

Only perform an action listed by status, after deciding whether the entire
transaction should reach its planned or original state:

```bash
codesplice --workspace /path/to/workspace recover TRANSACTION_ID --complete --json
codesplice --workspace /path/to/workspace recover TRANSACTION_ID --rollback --json
```

This ordinary example intentionally does not manufacture an interrupted journal.
Doing that deterministically requires internal, test-only failpoints, which are
not a supported CLI feature. The qualified interruption/complete/rollback
demonstration lives in `crates/codesplice-cli/tests/codex_pilot.rs` and is run by
`scripts/run-codex-pilot.sh` on a qualified host. Do not copy its failpoint
environment variables into production workflows.

Multi-target commit and rollback are recoverable, not atomically visible. Status
may report `mixed_old_new_possible`; corruption or unexpected files fail closed
rather than guessing.

