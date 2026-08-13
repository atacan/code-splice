# Protocol contract

Protocol version 1 accepts JSON batches of `move` and `copy` operations. Each
operation contains a source path, selector, and existing-file digest precondition,
plus a destination path, anchor, and existing-file digest or `must_not_exist`
precondition. The request never contains the workspace root.

Objects reject unknown fields and duplicate keys. Integers are nonnegative and
must fit `u64`. Existing-file digests use exactly `sha256:` followed by 64
lowercase hexadecimal digits. The normative Draft 2020-12 request and response
schemas are `docs/schema/v1/request.schema.json` and
`docs/schema/v1/response.schema.json`.

## Command surface

The `codesplice` binary provides the complete grammar for `inspect`, `apply`,
`recover`, `capabilities`, and `protocol-version`. `apply` is the only mutation
interface. A commit must use
exactly one of an expected plan digest or the explicit human convenience that
accepts the current plan. Agents use the expected digest.

In `v0.1.0`, `inspect`, preview, multi-target commit, recovery list
and status, and explicit transaction-wide completion/rollback are implemented.
Read-only commands create nothing and retain the existing shared control lock
through their scan, workspace observation, and report when it exists. Commit uses
two planning passes, requires explicit plan intent, prepares every candidate before
mutation, and commits changed targets in normalized UTF-8 path order.

Preview reports resolved byte coordinates, selected payload digests, output
before/after lengths and digests, plan-hash version 1, the plan digest, and an
opaque workspace identity hash. `--no-diff` changes only the diff field. Text
diffs label `LF`, `CRLF`, lone `CR`, and unterminated (`NONE`) lines; binary data
uses digest, length, and bounded base64 head/tail samples.

Beginning with CodeSplice `v0.2.0`, preview also accepts opt-in `--summary`.
Without that flag, JSON and human preview output are unchanged. `--summary`
retains the bounded diff and adds review-summary-v1 metadata at
`diff.summary.review`; `--summary --no-diff` is the concise review mode and uses
`diff.kind = "omitted"` while retaining the complete review metadata. The
independent nested schema is `docs/schema/review-summary-v1/schema.json`.

Review operation indices join `resolved_operations` in request order, and output
indices join `outputs` in normalized UTF-8 path order. Logical lines use the same
LF, CRLF, lone-CR, and final-unterminated-line semantics as inspection. Selected
ranges are interpreted as standalone byte sequences. Each output lists only
effectful payload insertion groups in final segment traversal order; a reported
same-file `no_op` has no fabricated insertion event. Existing binary and
truncation summary keys remain siblings of `review`.

Commit reports include a transaction ID (or `null` for a no-op), terminal state,
changed paths, preserved existing-target permission modes, and an inserted payload
digest for every effectful operation (`null` for a reported same-file no-op). The
`visibility` field states `recoverable_not_atomic`: unrelated readers can observe
mixed old/new targets during commit or rollback. Recovery list/status reports use
`mixed_old_new_possible` for those in-progress states, and `all_original` or
`all_planned` when the journal proves a uniform view.

JSON mode writes exactly one UTF-8 JSON value followed by LF to stdout. Human
diagnostics use stderr and visibly escape terminal control and bidi characters.

## Errors and warnings

Version 1 reserves the error identifiers and exit categories below.

| Exit | Category | Codes |
|---:|---|---|
| 2 | Request | `INVALID_CLI`, `INVALID_JSON`, `UNSUPPORTED_PROTOCOL_VERSION`, `INVALID_REQUEST`, `INVALID_DIGEST` |
| 3 | Conflict | `PRECONDITION_FAILED`, `FILE_CHANGED`, `FILE_ALIAS`, `EXPECTED_PLAN_REQUIRED`, `EXPECTED_PLAN_MISMATCH`, `PLAN_CHANGED_DURING_COMMIT`, `EDIT_CONFLICT`, `RECOVERY_CONFLICT` |
| 4 | Limit/support | `RESOURCE_LIMIT_EXCEEDED`, `UNSUPPORTED_PLATFORM`, `UNSUPPORTED_FILESYSTEM`, `UNSUPPORTED_FILE_TYPE`, `SYMLINK_NOT_ALLOWED`, `HARD_LINK_NOT_SUPPORTED`, `CROSS_DEVICE_TRANSACTION`, `NO_REPLACE_UNAVAILABLE` |
| 5 | Transaction | `TRANSACTION_BUSY`, `TRANSACTION_RECOVERY_REQUIRED`, `TRANSACTION_NOT_FOUND`, `RECOVERY_ACTION_NOT_ALLOWED` |
| 6 | Corruption | `CONTROL_DIRECTORY_INVALID`, `TRANSACTION_RECORD_CORRUPT` |
| 8 | Internal | `IO_ERROR`, `INTERNAL_ERROR` |

Warnings are `OBSERVATION_MAY_BE_STALE`, `METADATA_NOT_PRESERVED`, and
`DIFF_TRUNCATED`. Every error report includes its code, category, retryability,
message, and structured context.

Absolute request-file paths are redacted from structured I/O errors. Human error
messages visibly escape terminal controls and Unicode bidirectional-formatting
characters.

Every `TRANSACTION_BUSY` response has `retryable: true` and this exact context:

```json
{
  "lock_state": "contended",
  "recovery_required": "unknown",
  "safe_next_action": "wait_then_retry"
}
```

The error means that the command's nonblocking lock attempt encountered an
incompatible workspace lock. It does not identify the holder, prove that a
mutation is active, or establish whether recovery is required. `retryable` means
that an external state change may allow a later invocation to succeed; it does
not direct clients to poll or retry in a tight loop. Wait before retrying and
never bypass, remove, or break the lock.

Retry behavior depends on the command. A retried `inspect` or preview obtains a
fresh observation. An unchanged commit may be retried with the same
`--expect-plan`; the command replans and rejects a changed plan before mutation.
After a precondition or plan mismatch, inspect and preview again. A human using
`--accept-current-plan` previews again before retrying because that option can
accept a plan different from the earlier preview. Before an unrelated normal
mutation after contention, `recover --list --json` is the authoritative
point-in-time workspace status operation.

`recover --list --json` and `recover ID --status --json` are read-only and hold a
shared lock through validation, scanning, and report construction. An empty list
means that no recorded transaction needing recovery was observed. Any
`orphan_record`, `manifest_only`, or `active` entry requires recovery before a
new mutation. `cleanup_only` by itself is terminal cleanup state, not unfinished
recovery. If an exclusive holder prevents a safe scan, these status operations
return `TRANSACTION_BUSY` instead of observing a journal being published.

## Version freeze

The `v0.1.0` tag closes protocol version 1. The request and response schemas,
error and warning registries above, plan-hash version 1, and transaction-record
version 1 are the frozen release contract. A breaking wire-format change requires
a new protocol version.
