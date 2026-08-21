# Required agent workflow

Use this workflow for every srcmv mutation. Commands assume an installed `srcmv` binary and an absolute workspace path.

## 1. Confirm the runtime surface

```bash
command -v srcmv
srcmv capabilities --json
srcmv protocol-version --json
```

Protocol v1 supports `move` and `copy`, `lines` and `bytes` selectors, and `file_start`, `file_end`, `before_line`, `after_line`, and `byte_offset` anchors.

If the binary is unavailable, do not silently replace this workflow with ordinary file editing. In the srcmv source checkout, `cargo run --locked --release -p srcmv-cli -- ...` can run the repository version when building it is in scope. Otherwise report the missing installation.

## 2. Inspect one immutable starting view

Inspect all referenced paths together, including absent destinations:

```bash
srcmv --workspace /absolute/path/to/repo inspect \
  --path src/source.rs \
  --path src/destination.rs \
  --json
```

Record each path's `exists`, `file_type`, `sha256`, `byte_length`, and `line_count`. Use the returned lowercase SHA-256 value as every existing-file precondition. Use `must_not_exist` only for a destination reported absent.

Read the actual source and destination content to choose coordinates. Do not place selected payload bytes in the JSON request.

## 3. Construct and retain the request

Create a protocol-v1 JSON file with workspace-relative paths. See [request-construction.md](request-construction.md) for the exact shapes.

Keep this exact request unchanged between preview and commit. The request does not contain the workspace root.

## 4. Preview

```bash
srcmv --workspace /absolute/path/to/repo apply \
  --request /absolute/path/to/request.json \
  --preview \
  --summary \
  --no-diff \
  --json
```

Require exit status 0. Review:

- Every `resolved_operations` source interval, destination offset, effect, and selected payload digest.
- Every output path, change kind, before/after length, and before/after digest.
- The complete diff, or the binary summary when the content is non-text.
- All warnings.
- `plan_hash_version` and `plan_sha256`.

Beginning with v0.2.0, `--summary --no-diff` is the concise review mode. Review
the complete `diff.summary.review` operation metrics, output line counts, and
insertion groups. Omit `--no-diff` when detailed bounded diff evidence is useful.
Omitting `--summary` preserves the earlier preview shape. Neither option changes
the plan digest.

Stop if the resolved coordinates, output set, or diff do not exactly match the requested edit. Correct the request, then preview again.

## 5. Commit only the reviewed plan

Pass the unchanged request and exact `plan_sha256` returned by preview:

```bash
srcmv --workspace /absolute/path/to/repo apply \
  --request /absolute/path/to/request.json \
  --commit \
  --expect-plan sha256:PREVIEWED_64_LOWERCASE_HEX_DIGEST \
  --json
```

Never pass `--accept-current-plan` in an agent workflow.

If the command returns `PRECONDITION_FAILED`, `FILE_CHANGED`, `EXPECTED_PLAN_MISMATCH`, or `PLAN_CHANGED_DURING_COMMIT`, do not weaken the guard. Re-inspect the workspace, rebuild preconditions or coordinates if still appropriate, and preview a new plan for review.

If the command returns `TRANSACTION_BUSY`, wait before retrying. Never poll in a
tight loop, bypass or remove the lock, or infer a holder or transaction state
from contention. You may retry the exact request with the same `--expect-plan`:
the command replans and the expected-plan check still prevents an unreviewed plan
from committing. If the retry instead reports a precondition or plan mismatch,
return to inspect and preview. A retried inspect or preview inherently obtains a
fresh observation.

Before starting an unrelated normal mutation after contention, run:

```bash
srcmv --workspace /absolute/path/to/repo recover --list --json
```

This is the authoritative point-in-time workspace status check. An empty list
means no recorded transaction needing recovery was observed;
`orphan_record`, `manifest_only`, or `active` requires recovery before mutation;
`cleanup_only` alone does not.

## 6. Verify and finish

On a changing commit, require:

- `transaction_state: "committed"`
- `recoverability_status: "complete"`
- The expected `files_changed`
- An inserted payload digest for each effectful operation

`visibility: "recoverable_not_atomic"` means unrelated readers may have observed a mixed state during execution; it does not mean recovery is pending.

Inspect or read the resulting files, run the repository's relevant checks, and handle imports or formatting as explicitly separate edits. Do not alter srcmv's exact moved/copied bytes before verifying them.

For a reported no-op, require `transaction_state: "no_op"`, `transaction_id: null`, and no changed files.

If the response instead requires recovery, stop normal mutation and follow [recovery.md](recovery.md).
