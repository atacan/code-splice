# Transaction recovery

Use recovery only for an interrupted or unfinished CodeSplice transaction. Do not edit, move, or delete `.codesplice` records or artifacts manually.

## Inspect before acting

List known transactions:

```bash
codesplice --workspace /absolute/path/to/repo recover --list --json
```

Inspect a specific transaction:

```bash
codesplice --workspace /absolute/path/to/repo recover TRANSACTION_ID --status --json
```

Check its `classification`, allowed `actions`, and `visibility`:

- `all_original`: every target is proven at its original state.
- `mixed_old_new_possible`: commit or rollback may have exposed a mixture.
- `all_planned`: every target is proven at its planned state.

Classification can be `orphan_record`, `manifest_only`, `active`, or `cleanup_only`. Use only an action listed by the response.

## Require explicit direction

Completion and rollback both mutate the workspace. If the user did not explicitly choose the desired final state, report status and ask whether to finish the planned change or restore the originals. Do not infer the direction from convenience.

Complete the original plan:

```bash
codesplice --workspace /absolute/path/to/repo recover TRANSACTION_ID --complete --json
```

Restore the original target states:

```bash
codesplice --workspace /absolute/path/to/repo recover TRANSACTION_ID --rollback --json
```

CodeSplice classifies all targets before changing any target. Completion proceeds in normalized path order; rollback proceeds in reverse order. Unexpected or ambiguous artifacts fail with `RECOVERY_CONFLICT` and remain unchanged.

## Verify recovery

Require exit status 0, inspect the returned visibility, and run status or list again. Then verify workspace content and run relevant repository checks.

Do not continue normal CodeSplice mutations while an unfinished active transaction remains. For `TRANSACTION_RECORD_CORRUPT`, `CONTROL_DIRECTORY_INVALID`, or a recovery conflict, stop and surface the structured report; do not guess, repair permissions silently, or remove evidence.
