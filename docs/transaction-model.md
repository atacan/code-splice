# Transaction and recovery model

CodeSplice uses one transaction engine for one or many changed targets. Multi-file
visibility is recoverable, not atomic: unrelated readers can observe a mixture
during commit or rollback.

Mutating commands hold a nonblocking exclusive advisory workspace lock. Read-only
commands create nothing and use a shared lock only when a valid lock already
exists. A changing commit refuses an unfinished active transaction. Lock
contention proves only that an incompatible lock is held: an exclusive request
can be blocked by a shared reader, and a shared request can be blocked by an
exclusive holder. CodeSplice therefore does not infer holder kind, transaction
identity, phase, or recovery need from contention. The persistent lock file is a
rendezvous object, not evidence of a stale lock; the OS releases the advisory
lock when its holder exits.

Transaction records live below `.codesplice/transactions/<id>/`. A manifest is
published before candidate creation. Append-only state records contain full
snapshots linked by checksums and progress through:

```text
Preparing -> Prepared -> Committing -> Committed
     |           |            |
     +-----------+------------+-> RollingBack -> RolledBack
```

Records are written and synced before publication and never claim a filesystem
action before it occurs. Every candidate is ready and verified before the first
target changes. Existing targets are moved to unique backups without replacement;
candidates are installed without replacement. There is no replacing-rename
fallback.

Recovery classifies every relevant location as original, candidate, absent, or
unexpected using identity, length, digest, type, and parent identity. It completes
or rolls back only when the complete classification is unambiguous. Corrupt or
unexpected state changes nothing and reports an explicit conflict.

Terminal active transaction directories move without replacement to a suffix-
classified directory below `.codesplice/completed/` before bounded cleanup.

## Version 1 record envelope

Manifest records begin with `CODESPLICE-MANIFEST\0`; state records begin with
`CODESPLICE-STATE\0`. The magic is followed by a big-endian format-version `u32`,
a big-endian payload-length `u64`, the exact UTF-8 JSON payload, and SHA-256 of all
preceding envelope bytes. Published record checksums—not hashes of a second
serialization—link the manifest and contiguous state chain.

Records are created exclusively as mode-`0600` temporaries, completely written,
flushed, file-synced, renamed without replacement, and followed by a transaction-
directory sync. A published record is immutable. Each state record is a complete
snapshot and names the prior published record checksum; sequence zero has no prior
checksum. Gaps, forks, invalid combinations, bad checksums, truncation, excess
size, and unknown fields fail closed.

The private payload schemas are
`docs/schema/transaction-v1/manifest.schema.json` and
`docs/schema/transaction-v1/state.schema.json`; golden JSON and complete envelope
bytes are under `tests/golden/transaction-v1/`.

## Phase 8 multi-target execution

`recover --list` is the workspace-level status operation, and
`recover ID --status` is the transaction-level operation. Both are read-only,
create nothing, and retain a nonblocking shared lock through validation, scanning,
and report construction when a valid control tree exists. An empty list means no
recorded transaction needing recovery was observed. `orphan_record`,
`manifest_only`, or `active` requires recovery before a new mutation;
`cleanup_only` alone is terminal cleanup state. An exclusive holder yields
`TRANSACTION_BUSY` rather than a scan of a journal being published.

Explicit recovery classifies every target before changing any of them, then
completes in normalized UTF-8 path order or rolls back in reverse order. Candidate
identity is authoritative even when replacement bytes are equal. Each candidate
readiness, target backup, install, and rollback restoration publishes a full
indexed state snapshot. A new-transaction gate rejects every active transaction
and may delete only fully validated suffix-classified cleanup-only directories.

Commit responses state that visibility is `recoverable_not_atomic`. Recovery
reports classify visibility as `all_original`, `mixed_old_new_possible`, or
`all_planned`; the mixed classification is expected during `Committing` and
`RollingBack` and does not weaken transaction-wide recovery.
