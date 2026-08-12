# Protocol contract

Protocol version 1 accepts JSON batches of `move` and `copy` operations. Each
operation contains a source path, selector, and existing-file digest precondition,
plus a destination path, anchor, and existing-file digest or `must_not_exist`
precondition. The request never contains the workspace root.

Objects reject unknown fields and duplicate keys. Integers are nonnegative and
must fit the domain type. Existing-file digests use exactly `sha256:` followed by
64 lowercase hexadecimal digits. The normative request and response JSON Schemas
are created in Phase 2 under `docs/schema/v1/`.

## Command surface

The `codesplice` binary will provide `inspect`, `apply`, `recover`, `capabilities`,
and `protocol-version`. `apply` is the only mutation interface. A commit must use
exactly one of an expected plan digest or the explicit human convenience that
accepts the current plan. Agents use the expected digest.

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
