# Transaction and recovery model

CodeSplice uses one transaction engine for one or many changed targets. Multi-file
visibility is recoverable, not atomic: unrelated readers can observe a mixture
during commit or rollback.

Mutating commands hold a nonblocking exclusive advisory workspace lock. Read-only
commands create nothing and use a shared lock only when a valid lock already
exists. A changing commit refuses an unfinished active transaction.

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
Details of the record envelope and private schemas remain authoritative in
Sections 8.2–8.7 of the implementation plan until Phase 5 checks in the schemas.
