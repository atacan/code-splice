# CodeSplice v0.1.0 implementation plan

**Status:** READY FOR IMPLEMENTATION
**Plan baseline:** 2026-08-11
**Implementation language:** Rust
**Binary:** `codesplice`
**Next phase:** Phase 1

This document is the implementation authority for the `v0.1.0` pilot. It replaces
the earlier draft that combined an implementation plan with a cross-platform
filesystem certification program. That draft remains available in Git history at
commit `14dd23a`.

The plan deliberately optimizes for a trustworthy pilot that can be built and
tested phase by phase. It does not claim hostile-filesystem security, stable public
API compatibility, or Windows support.

---

# 1. How to run the phases

Use one Codex session per numbered phase. Start each session with:

```text
Implement Phase N from notes/implementation_plan.md.
Read the whole plan, verify the previous checkpoint, and stay within Phase N.
Run the phase checkpoint, write notes/checkpoints/phase-N.md, commit the phase,
and stop without beginning Phase N+1.
```

Each phase session must:

1. Read this whole document before changing code.
2. Inspect the current worktree and the preceding checkpoint.
3. Implement only the current phase's in-scope work.
4. Add tests with the behavior, not in a later cleanup phase.
5. Run the common checks and the phase-specific checks.
6. Write `notes/checkpoints/phase-N.md` using the template below.
7. Update only the status table in this document.
8. Commit the completed phase separately and stop.

Do not reopen a product decision merely because another design is possible.
Implementation-level choices are allowed when they preserve the decisions and
invariants below. Stop for a plan amendment only when an ambiguity would change:

* user-visible semantics;
* persisted or protocol data;
* the exact-byte guarantee;
* conflict or recovery safety; or
* the declared platform/threat-model boundary.

When a required platform primitive proves unavailable, fail the phase with the
evidence. Do not add a weaker fallback silently.

Where a phase explicitly creates a schema or private persisted payload, that phase
is authorized to choose concrete field names and integer widths consistent with
the semantic records in this plan. It must check in the schema and golden examples
before using the format, and the choice freezes at that phase's checkpoint. This is
an implementation decision, not an open product question.

## 1.1 Phase status

| Phase | Name | Status |
|---|---|---|
| Planning gate | Scope and decisions | Complete |
| 1 | Workspace, contracts, and test foundation | Complete |
| 2 | Protocol v1 and CLI shell | Complete |
| 3 | Workspace inspection and immutable snapshots | Complete |
| 4 | Pure planner and plan digest | Complete |
| 5 | Lock, journal, and recovery classifier | Complete |
| 6 | Preview and reporting | Complete |
| 7 | Single-target commit and recovery | Not started |
| 8 | Multi-target commit and recovery | Not started |
| 9 | Hardening and platform qualification | Not started |
| 10 | Codex pilot and v0.1.0 release | Not started |

Allowed status values are `Not started`, `In progress`, `Complete`, and `Blocked`.
Only the current phase may be `In progress`.

## 1.2 Checkpoint report

```markdown
# Phase N checkpoint

Status: PASS | PASS WITH CONCERNS | FAIL
Commit: <hash or "not committed">

## Delivered
- ...

## Verification
- `<command>` — pass/fail

## Demonstrated behavior
- ...

## Decisions made within phase authority
- None | ...

## Deviations or concerns
- None | ...

## Next phase readiness
- Ready | Blocked because ...
```

`PASS WITH CONCERNS` is allowed only for a documented non-blocking risk. A skipped
acceptance criterion is `FAIL`, not a concern.

---

# 2. Product contract

CodeSplice moves or copies bytes already present in files so a coding agent does
not have to reproduce those bytes.

For every effectful operation in exact mode:

```text
SHA256(bytes selected from the immutable input snapshot)
==
SHA256(bytes inserted into the planned output)
```

A same-file move anchored exactly at its own start or end is reported as a no-op:
it performs no insertion, so its `inserted_payload_sha256` is `null` and the
payload-equality assertion is not applicable to that operation.

`v0.1.0` supports:

* batch `move` and `copy` operations;
* line and byte selectors;
* file-start, file-end, line, and byte-offset destination anchors;
* existing and new destination files;
* same-file and cross-file operations;
* immutable-snapshot planning;
* preview and a plan digest precondition;
* JSON requests and JSON/human reports;
* recoverable single-target and multi-target commits;
* explicit recovery after process interruption; and
* Linux/ext4 and macOS/APFS local-filesystem pilots.

It does not:

* parse programming languages;
* update imports or references;
* format, reindent, or normalize line endings;
* create parent directories;
* preserve hard-link relationships, ownership, ACLs, xattrs, resource forks,
  timestamps, or platform flags;
* provide atomic visibility across multiple files;
* defend against a malicious process that can mutate the workspace concurrently;
* support Windows in `v0.1.0`; or
* promise protocol stability before the `v0.1.0` schema is tagged.

---

# 3. Decisions that close the former open points

These decisions are final for this plan.

## 3.1 Release and threat model

1. **`v0.1.0` is a pilot, not a general filesystem security boundary.** The user
   running CodeSplice and other CodeSplice processes are trusted. The tool detects
   ordinary concurrent edits and refuses ambiguous recovery. It does not attempt
   to defeat a hostile principal with write access to the workspace.
2. **Linux and macOS only.** The supported architecture targets are Linux x86_64
   and macOS arm64. Windows, NTFS collation, DACLs, reparse-point primitives, and
   Windows packaging are deferred.
3. **Local, single-device transactions only.** The initial release matrix is Linux
   x86_64 on local ext4 and macOS arm64 on local APFS. The control directory and
   every changed target parent must report the same filesystem device. Every other
   filesystem, including network and overlay filesystems, is rejected for commit;
   adding a row requires the complete Phase 9 qualification suite.
4. **One durability policy.** CodeSplice flushes record and candidate data and
   syncs affected directories where the platform supports it. The v0.1 guarantee
   is recovery after abrupt process termination. Power-loss durability is not
   claimed.

## 3.2 Files, paths, and metadata

5. **Regular files only.** Operation paths and their parent chain may not traverse
   symlinks. Directories, symlinks, sockets, devices, and other special files are
   rejected.
6. **UTF-8 workspace-relative paths only.** Absolute paths, empty components,
   `.`, `..`, and NUL are rejected. Parents must already exist.
7. **`.codesplice` is the sole reserved tree.** The first component is rejected
   with ASCII-case-insensitive comparison so aliases such as `.CodeSplice` cannot
   be operation paths on a case-insensitive filesystem.
8. **Existing aliases use physical identity.** Two different request paths that
   resolve to the same `(device, inode)` are rejected. Repeated references using
   exactly the same normalized path are allowed only with the same precondition.
9. **Absent paths use normalized spelling.** The planner groups identical absent
   paths. A native collision between distinct absent spellings is caught by the
   no-replace install primitive and causes transaction rollback. v0.1 does not
   implement a universal Unicode/filesystem collation table.
10. **Exact means content bytes.** For an existing target, POSIX permission bits
    are preserved from the object verified immediately before replacement. A new
    file receives `0666 & !startup_umask`. Other metadata is outside the guarantee
    and produces `METADATA_NOT_PRESERVED` in the commit report. A changed target with
    `st_nlink != 1` is rejected; there is no override in v0.1.

## 3.3 Protocol and CLI

11. **One mutation interface.** `apply` is the only move/copy execution command.
    Direct `move` and `copy` convenience commands and `doctor` are deferred.
12. **Protocol version 1 is closed at the release tag, not before implementation.**
    Requests reject unknown fields and enum values. During Phases 1–9, a deliberate
    plan amendment may change v1; after the release tag, a breaking wire change
    requires a new protocol version.
13. **Workspace selection is out of band.** The request has no workspace field.
    `--workspace` selects the root and defaults to the current directory.
14. **Every referenced path has an explicit precondition.** Existing files use a
    SHA-256 digest. A new destination uses `must_not_exist`.
15. **A commit requires intent.** It accepts exactly one of `--expect-plan` or
    `--accept-current-plan`. Agents use `--expect-plan`; the latter is an explicit
    human convenience.

## 3.4 Planning and transactions

16. **All selectors and anchors use one immutable initial snapshot.** Request order
    never changes the coordinates of a later operation.
17. **The plan stores segments, not materialized output files.** Output bytes are
    streamed from immutable snapshot slices during preview hashing and candidate
    creation.
18. **Plan digests use deterministic CBOR.** The precise positional schema is in
    Section 7.4. The format has its own version and golden byte vectors.
19. **One transaction engine serves one or many targets.** Phase 7 restricts the
    target count to one through validation; Phase 8 removes that restriction.
20. **Journal state uses append-only full snapshots.** The v0.1 target limit is
    small enough that a complete state record after each transition is clearer
    than delta records plus periodic compaction.
21. **Recovery is conservative.** It completes or rolls back only when every
    relevant path matches a recorded identity and digest. Otherwise it reports a
    conflict and changes nothing.
22. **Multi-target commits are recoverable, not atomically visible.** Unrelated
    readers may observe a mixture while a commit or rollback is in progress.
23. **A plan-level no-op creates no control artifacts or transaction.** It returns
    success with `transaction_id: null` and `files_changed: []`.
24. **The project license is Apache-2.0.** Phase 1 adds the standard license text;
    changing the license is a product decision, not a phase-local choice.

## 3.5 Explicitly deferred decisions

These are not blockers and must not be designed during `v0.1.0` phases:

* Windows support and Windows path comparison;
* hostile-workspace race resistance and handle-relative APIs on every operation;
* power-loss guarantees and selectable durability modes;
* ACL/xattr/resource-fork/ownership preservation;
* public protocol compatibility beyond v0.1;
* direct move/copy CLI ergonomics;
* additional operations (`delete`, `insert`, `swap`, `extract`);
* Tree-sitter or symbol selectors; and
* semantic refactoring.

There are no unresolved implementation-blocking product decisions at this
baseline.

---

# 4. Normative editing semantics

## 4.1 Line model

Line numbers are one-based. A line selector includes both endpoint lines and each
selected line's original terminator.

Recognized terminators are LF, CRLF, and lone CR. They are preserved byte for
byte. A nonempty unterminated final line is selectable. A file ending in a line
terminator has no phantom final line. An empty file has zero lines.

## 4.2 Selectors

```text
lines(start, end): 1 <= start <= end <= line_count
bytes(start, end): 0 <= start < end <= file_length, range [start, end)
```

Empty selections are rejected.

## 4.3 Anchors

```text
file_start
file_end
before_line(line)
after_line(line)
byte_offset(offset)
```

`before_line(n)` is the first byte of line `n`. `after_line(n)` is immediately
after its full terminator, or EOF for an unterminated last line. A line anchor
requires `1 <= n <= line_count`. A byte offset accepts `0..=file_length` and may
split any byte sequence, including CRLF.

For a new file, the initial content is empty and the only valid anchors are
`file_start`, `file_end`, and `byte_offset(0)`.

## 4.4 Same-file moves

For source `[start, end)` and destination offset `d` in the initial snapshot:

```text
d < start       effectful backward move
d == start      no-op
start < d < end invalid
d == end        no-op
d > end         effectful forward move
```

A no-op remains in the operation report and plan digest but contributes no edit
event. If all resulting files are byte-identical to their inputs, the whole plan
is a no-op.

## 4.5 Composition

All selections and anchors resolve before composition. A move contributes a
deletion interval and an insertion of its original selected slice. A copy
contributes only an insertion. Every inserted slice refers to the initial
snapshot, even when another operation deletes the same source bytes.

Rules:

* effectful move deletion ranges may not overlap;
* copies may overlap copies or moved source ranges;
* an insertion strictly inside a deletion range is invalid;
* insertion at either deletion boundary is valid; and
* multiple insertions at one offset use ascending operation index (request order).

For each file, sort events by:

```text
(initial_offset, event_class, operation_index)

event_class:
1 deletion_end
2 insertion
3 deletion_start
4 end_of_file
```

Sweep the initial bytes once. At each offset, emit surviving original bytes up to
the offset, end deletions, emit insertions, then begin deletions. At EOF, emit the
last surviving slice followed by EOF insertions. This event order is the only
composition algorithm.

After building the segment recipe, stream-compare it with the original bytes when
length and digest match. Actual byte equality, not the presence of edit events,
decides whether an existing file changes.

## 4.6 Preconditions and conflicts

An existing path precondition is the exact lowercase form:

```text
sha256:<64 lowercase hexadecimal digits>
```

A new destination uses `must_not_exist`. A source can never use
`must_not_exist`. All references to one normalized path must repeat the same
precondition.

Planning returns a conflict for out-of-range coordinates, overlapping move
deletions, insertion inside a deletion, aliases, incompatible preconditions, or a
path used as both absent and existing.

Before the first target mutation, commit reopens and rehashes every input file,
including copy-only sources, and rechecks every required absence, target link
count, and parent identity. Any mismatch aborts before mutation.

---

# 5. Workspace and architecture

## 5.1 Repository shape

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml
README.md
LICENSE
docs/
  specification.md
  protocol.md
  transaction-model.md
  security.md
  metadata.md
  resource-limits.md
  platform-support.md
  agent-integration.md
  schema/v1/request.schema.json
  schema/v1/response.schema.json
  schema/transaction-v1/manifest.schema.json
  schema/transaction-v1/state.schema.json
crates/
  codesplice-core/
  codesplice-fs/
  codesplice-protocol/
  codesplice-cli/
  codesplice-test-support/
tests/
  fixtures/
  golden/
  scenarios/
notes/checkpoints/
```

## 5.2 Ownership and dependency direction

`codesplice-core` owns immutable domain types, line indexing, planning, segment
recipes, resource accounting, and plan-digest encoding. It performs no filesystem
access and depends on neither protocol nor filesystem crates.

`codesplice-fs` owns workspace resolution, path validation, physical identities,
snapshots, locking, record storage, candidate streaming, commit, and recovery. It
depends on core, never protocol.

`codesplice-protocol` owns JSON DTOs, schema-aligned parsing, domain conversion,
response DTOs, stable error/warning DTOs, and redaction. It depends on core, never
filesystem.

`codesplice-cli` owns argument parsing, orchestration, rendering, exit codes, and
startup umask capture. It depends on core, filesystem, and protocol.

`codesplice-test-support` owns fixtures, deterministic identities, fault injection,
temporary workspaces, and subprocess helpers. Production crates must not depend on
it outside dev dependencies.

```text
codesplice-core <- codesplice-fs
codesplice-core <- codesplice-protocol
codesplice-core + codesplice-fs + codesplice-protocol <- codesplice-cli
```

## 5.3 Core model

The exact Rust layout may evolve without a plan amendment, but it must preserve
these concepts:

```text
BatchSpecification
Operation { Move | Copy }
SourceSelection
Destination
Precondition { Sha256 | MustNotExist }
Selector { Lines | Bytes }
Anchor { FileStart | FileEnd | BeforeLine | AfterLine | ByteOffset }
WorkspaceSnapshot
FileSnapshot
FileIdentity
LineIndex
ResolvedOperation
EditPlan
PlannedOutput
OutputSegment { OriginalSlice | PayloadSlice }
PlanDigest
ResourceBudget
```

Snapshot file bytes are immutable shared storage. Segments refer to snapshot file
IDs and byte ranges; they do not own duplicate payload buffers.

## 5.4 Workspace root and path validation

The CLI resolves `--workspace` once to an absolute canonical root and opens it.
The root must exist and be a directory. A symlink used to spell the root may be
canonicalized, but operation paths beneath the canonical root may not traverse a
symlink.

For every path:

1. Parse and normalize the UTF-8 relative spelling.
2. Reject the reserved first component.
3. Walk existing parent components without following symlinks.
4. Record each parent `(device, inode)` identity.
5. For an existing final component, require a regular file and record its identity.
6. For an absent destination, record the existing parent identity and basename.

Revalidate the canonical root identity, every target parent identity, input
identity/digest, and absence precondition during commit.

## 5.5 Control directory

Mutating commands use:

```text
<workspace>/.codesplice/
  lock
  transactions/
    <32-lowercase-hex transaction id>/
  completed/
    <32-lowercase-hex transaction id>-committed/
    <32-lowercase-hex transaction id>-rolledback/
```

On first mutating commit, create `.codesplice`, `transactions`, and `completed`
with mode `0700` and `lock` with mode `0600`. Existing objects must be real,
user-owned directories or a regular user-owned file with no group/other write
permission. Never repair permissions silently.

Preview, inspect, capabilities, and protocol-version do not create control
artifacts.

---

# 6. Protocol and command surface

## 6.1 Request shape

The normative JSON Schema is created in Phase 2. Its semantic shape is:

```json
{
  "protocol_version": 1,
  "operations": [
    {
      "kind": "move",
      "source": {
        "path": "src/a.rs",
        "selector": { "kind": "lines", "start": 10, "end": 20 },
        "precondition": {
          "kind": "sha256",
          "value": "sha256:0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
        }
      },
      "destination": {
        "path": "src/b.rs",
        "anchor": { "kind": "before_line", "line": 5 },
        "precondition": {
          "kind": "sha256",
          "value": "sha256:abcdef0123456789abcdef0123456789abcdef0123456789abcdef0123456789"
        }
      }
    }
  ]
}
```

Byte selectors use `start` and exclusive `end`. Byte anchors use `offset`.
File-start and file-end anchors contain only `kind`. A new destination uses:

```json
{ "kind": "must_not_exist" }
```

All objects reject unknown fields and duplicate keys. Integers must be nonnegative
JSON integers that fit the domain type without narrowing.

## 6.2 Commands

```text
codesplice [--workspace PATH] inspect --path RELATIVE [--path RELATIVE ...] --json

codesplice [--workspace PATH] apply --request FILE|- --preview
           [--json] [--no-diff]

codesplice [--workspace PATH] apply --request FILE|- --commit
           (--expect-plan sha256:... | --accept-current-plan) [--json]

codesplice [--workspace PATH] recover --list [--json]
codesplice [--workspace PATH] recover ID --status [--json]
codesplice [--workspace PATH] recover ID --complete [--json]
codesplice [--workspace PATH] recover ID --rollback [--json]

codesplice capabilities --json
codesplice protocol-version --json
```

`inspect` reports existence, SHA-256, byte length, line count, file type, and an
opaque hash of physical identity. It reports an absent final path only when its
parent is valid.

JSON mode emits exactly one UTF-8 JSON value plus LF on stdout. Human diagnostics
go to stderr. Human rendering visibly escapes control and bidi characters from
paths or request data.

## 6.3 Response requirements

Preview success includes:

```text
protocol_version
plan_hash_version
plan_sha256
workspace_identity_hash
resolved_operations[]
outputs[] with before/after length and SHA-256
selected_payload_sha256 per operation
diff or diff summary
warnings[]
```

Commit success additionally includes:

```text
transaction_id or null
transaction_state
files_changed[]
inserted_payload_sha256 per operation (`null` for a reported no-op)
recoverability_status
preserved_permission_mode where applicable
```

Every error includes `code`, `category`, `retryable`, `message`, and structured
`context`. Paths are workspace-relative by default.

## 6.4 Error and warning registry

The Phase 2 schema freezes these v0.1 identifiers:

| Category / exit | Codes |
|---|---|
| Request / 2 | `INVALID_CLI`, `INVALID_JSON`, `UNSUPPORTED_PROTOCOL_VERSION`, `INVALID_REQUEST`, `INVALID_DIGEST` |
| Conflict / 3 | `PRECONDITION_FAILED`, `FILE_CHANGED`, `FILE_ALIAS`, `EXPECTED_PLAN_REQUIRED`, `EXPECTED_PLAN_MISMATCH`, `PLAN_CHANGED_DURING_COMMIT`, `EDIT_CONFLICT`, `RECOVERY_CONFLICT` |
| Limit or support / 4 | `RESOURCE_LIMIT_EXCEEDED`, `UNSUPPORTED_PLATFORM`, `UNSUPPORTED_FILESYSTEM`, `UNSUPPORTED_FILE_TYPE`, `SYMLINK_NOT_ALLOWED`, `HARD_LINK_NOT_SUPPORTED`, `CROSS_DEVICE_TRANSACTION`, `NO_REPLACE_UNAVAILABLE` |
| Transaction / 5 | `TRANSACTION_BUSY`, `TRANSACTION_RECOVERY_REQUIRED`, `TRANSACTION_NOT_FOUND`, `RECOVERY_ACTION_NOT_ALLOWED` |
| Corruption / 6 | `CONTROL_DIRECTORY_INVALID`, `TRANSACTION_RECORD_CORRUPT` |
| Internal / 8 | `IO_ERROR`, `INTERNAL_ERROR` |

Path, selector, anchor, and reserved-path validation failures use
`INVALID_REQUEST` with a stable `reason` field rather than creating a distinct
code for every syntax case.

Warnings are:

```text
OBSERVATION_MAY_BE_STALE
METADATA_NOT_PRESERVED
DIFF_TRUNCATED
```

No phase adds an identifier without updating this section, schemas, documentation,
and golden tests.

---

# 7. Planning and plan digest

## 7.1 Planning pipeline

```text
CLI workspace selection
-> protocol parse and domain conversion
-> immutable snapshot acquisition
-> selector/anchor resolution
-> conflict validation
-> segment recipe construction
-> byte comparison and output classification
-> plan digest
-> preview OR transaction commit
```

Planning is pure after snapshot acquisition.

## 7.2 Output classification

For an existing path:

* `unchanged` when the recipe is byte-for-byte equal to the snapshot;
* `emptied_existing` when the result is empty and differs; or
* `modified_existing` otherwise.

For an absent path, the classification is `created_new`.

Create one output record for each path receiving at least one deletion or insertion
event. Exclude copy-only sources and a file mentioned only by a no-op move; those
semantics remain represented by input and resolved-operation records. An effectful
recipe that reproduces its original bytes is still an `unchanged` output record.

Only non-unchanged outputs become transaction targets. An operation can be
effectful while its output is `unchanged`; it remains in the resolved operation
list and plan digest.

## 7.3 Expected-plan commit

Commit order is:

1. Snapshot and plan without creating `.codesplice`.
2. Compare `--expect-plan`, when used. Mismatch stops here.
3. If the plan is a no-op, run the noncreating diagnostic check from Section 8.1
   and return only when it observes no active transaction.
4. Acquire the workspace mutation lock.
5. Refuse to start while any active transaction directory exists. A validated
   cleanup-only entry under `completed` may be finished before continuing; it can
   never cause target completion or rollback during a new commit.
6. Snapshot and plan again while locked.
7. Compare the expected digest again and compare with the pre-lock plan.
8. Prepare the transaction.
9. Revalidate every input, target parent, and retained control-directory identity
   immediately before mutation.
10. Commit.

A pre-lock plan that changes while acquiring the lock returns
`PLAN_CHANGED_DURING_COMMIT`, even with `--accept-current-plan`.

## 7.4 Plan-hash version 1

```text
plan_sha256 = SHA256(
  ASCII "CODESPLICE-PLAN-V1\0"
  || deterministic_cbor(plan_record)
)
```

Use RFC 8949 deterministic encoding. Definite lengths are mandatory; maps, tags,
floats, and indefinite-length items are forbidden. The top-level record and all
nested records are arrays with the following positional schema:

```text
plan_record = [
  1,                              # plan-hash version
  1,                              # protocol version
  workspace_identity,
  input_records,
  resolved_operations,
  output_records
]

workspace_identity = [device_u64, inode_u64]

input_record = [
  normalized_path,
  state
]

state =
  [0, parent_device_u64, parent_inode_u64]
  | [1, parent_device_u64, parent_inode_u64,
       file_device_u64, file_inode_u64, length_u64, sha256_bytes]

resolved_operation = [
  operation_index_u64,
  kind_u64,                       # 1 move | 2 copy
  source_path,
  selector_record,
  source_precondition_record,
  source_start_u64,
  source_end_u64,
  selected_sha256_bytes,
  destination_path,
  anchor_record,
  destination_precondition_record,
  destination_offset_u64,
  effect_u64                      # 1 changed | 2 no_op
]

output_record = [
  normalized_path,
  original_sha256_or_null,
  resulting_sha256_bytes,
  resulting_length_u64,
  change_kind_u64,                # 1 unchanged | 2 modified | 3 created | 4 emptied
  segments
]

segment =
  [1, snapshot_input_index_u64, start_u64, end_u64]
  | [2, operation_index_u64, snapshot_input_index_u64, start_u64, end_u64,
       payload_sha256_bytes]
```

Selector, anchor, and precondition records are exactly:

```text
selector = [1, first_line_u64, last_line_u64]
         | [2, start_byte_u64, end_byte_exclusive_u64]

anchor = [1]                       # file_start
       | [2]                       # file_end
       | [3, line_u64]             # before_line
       | [4, line_u64]             # after_line
       | [5, offset_u64]           # byte_offset

precondition = [1, sha256_bytes]
             | [2]                 # must_not_exist
```

Sort input and output records by normalized path UTF-8 bytes. Operations remain in
request order. SHA-256 values are 32-byte CBOR byte strings, not text. Golden
vectors cover every enum, an absent input, arbitrary non-UTF-8 file content, a
same-file no-op, same-offset insertions, and maximum accepted integers.

The digest identifies one resolved plan in one physical workspace snapshot. It is
not portable across workspace copies because physical identities are included.
CLI output mode, diff settings, expected-plan policy, transaction ID, timestamps,
permission modes, and warnings are not part of the plan record because they do not
change resulting content bytes.

---

# 8. Transaction and recovery model

## 8.1 Locking

Commit and mutating recovery acquire a nonblocking exclusive advisory lock on
`.codesplice/lock` and retain the same open descriptor until mutation and required
record cleanup finish. Contention returns `TRANSACTION_BUSY`.

Preview, inspect, `recover --list`, and recovery status create nothing. If a valid
lock already exists, they acquire a nonblocking shared lock for the observation.
If no lock exists, preview/inspect proceed and emit `OBSERVATION_MAY_BE_STALE`;
recovery listing simply reports no transactions. Expected-plan validation remains
the commit authority.

While holding a shared lock, preview, inspect, and a no-op commit perform a bounded
control scan. An active transaction returns `TRANSACTION_RECOVERY_REQUIRED`; an
invalid record returns `TRANSACTION_RECORD_CORRUPT`. Cleanup-only entries under
`completed` do not change snapshot semantics and may be reported as pending
cleanup. If neither control directory nor lock exists, a no-op succeeds without
creating either and includes `OBSERVATION_MAY_BE_STALE`. A control directory
without its lock is `CONTROL_DIRECTORY_INVALID`.

The lock contains no identity record and has no repair command. An invalid control
tree returns `CONTROL_DIRECTORY_INVALID` and requires manual inspection.
After acquiring the lock, record the physical identities of the root, control
directories, and lock object and confirm that their directory entries still name
the opened objects. Revalidate those identities once immediately before the first
target mutation (or first target-mutating recovery action). This detects ordinary
replacement without claiming hostile namespace-race resistance.

## 8.2 Transaction files

```text
.codesplice/transactions/<transaction-id>/
  manifest.rec
  state-00000000.rec
  state-00000001.rec
  ...
  candidate-00000000
  backup-00000000
  manifest.tmp                     # unpublished only
  state-00000000.tmp               # unpublished only

.codesplice/completed/
  <transaction-id>-committed/      # cleanup-only
  <transaction-id>-rolledback/     # cleanup-only
```

The transaction ID is 128 random bits encoded as exactly 32 lowercase hex
characters. Exclusive directory creation checks the active name and both completed
suffix forms and retries a random collision at most eight times. Target indices and
state sequences are zero-padded eight-digit decimal values. All names are generated
by CodeSplice and contain no user path component.

Manifest records use magic `CODESPLICE-MANIFEST\0`; state records use magic
`CODESPLICE-STATE\0`. Each record is:

```text
magic bytes
u32 big-endian format version (= 1)
u64 big-endian payload length
UTF-8 JSON payload bytes
SHA256(all preceding bytes)
```

The payload is schema-validated with unknown fields rejected. The checksum covers
the exact stored bytes, so record JSON does not need a canonical serializer.

Publish a record by exclusive temporary creation, complete write, flush, file
sync, rename without replacement to its final name, and transaction-directory
sync. Published records are immutable. Under the exclusive workspace lock,
recovery may delete only a grammar-valid unpublished temporary after bounding and
validating it; it never treats a temporary as published state.

State records contain a full transaction-state snapshot, a strictly increasing
sequence number, the manifest checksum, the prior state-record checksum (null for
sequence zero), the global state, and every target's stage and recorded identities.
Recovery accepts only one complete contiguous checksum chain beginning at zero.

Each target state has these orthogonal persisted fields:

```text
candidate: missing | ready(candidate_identity)
commit: untouched | backed_up(backup_identity, preserved_mode) |
        installed(final_identity)
rollback: none | original_restored(restored_identity) | absence_restored
```

The global state constrains valid combinations. For example, `Prepared` requires
every candidate to be `ready` and every commit field to be `untouched`; `Committed`
requires every commit field to be `installed`; `RolledBack` requires every rollback
field to describe the original existence state. Any other combination is record
corruption, not a recoverable filesystem lag.

Limits are 100 targets and 512 state records, making full state snapshots bounded.

## 8.3 Manifest

Publish the immutable manifest before candidate creation. It records:

* transaction and format versions;
* transaction ID, workspace identity, and plan digest;
* every input path, parent identity, existence state, file identity, digest, length,
  and link count needed for final pre-mutation validation;
* normalized target ordering;
* each target and parent identity;
* original existence, identity, digest, and length;
* candidate and backup basenames;
* expected candidate digest and length;
* the existing-target permission policy (the actual mode is captured from the
  verified backup in state) or the new-file mode;
* source segment references needed only for initial candidate preparation; and
* the metadata limitations acknowledged by the v0.1 contract.

Manifest paths are data for validation and reports, never paths blindly joined for
filesystem mutation. Filesystem operations use the validated workspace-relative
target and generated single-component artifact names.

## 8.4 State machine

```text
Preparing -> Prepared -> Committing -> Committed
     |           |            |
     +-----------+------------+-> RollingBack -> RolledBack
```

Sequence zero is `Preparing` and is published after the manifest but before any
candidate. `Preparing` recovery permits rollback only.

After every candidate is completely written, synced, hashed, and identified,
publish `Prepared` with all candidate identities. `Prepared` recovery permits
complete or rollback. Completion from `Prepared` must first revalidate every
manifest input, including copy-only sources, because no target mutation has begun.

Publish `Committing` before the first target mutation. For each target, publish a
new full state record after a successful backup rename and after a successful
candidate installation. A record never claims a filesystem action before it
occurs.

Publish `Committed` only after every installed target is re-opened and verified.
After `Committed`, rollback is forbidden; recovery may only verify final targets
and finish cleanup.

Any failure after the first target mutation attempts transaction-wide rollback in
reverse target order. Publish `RollingBack` before the first rollback mutation and
record progress after each action. Publish `RolledBack` only after all original
states are verified. If `RollingBack` cannot be published, stop target mutation and
return `TRANSACTION_RECOVERY_REQUIRED`; the preceding valid state plus filesystem
classification remains the recovery authority.

An error after journal creation but before the first target mutation also rolls
back transaction-owned candidates and records. When that cleanup succeeds, return
the original error; when it cannot be recorded or completed, return
`TRANSACTION_RECOVERY_REQUIRED` with the original error in context.

## 8.5 Per-target filesystem order

For an existing target:

1. Revalidate target and parent identity and content digest.
2. Rename target to its absent backup name with no-replace semantics.
3. Reopen and verify the backup as the recorded original.
4. Read its current POSIX permission bits and apply them to the candidate.
5. Publish the backed-up state.
6. Rename candidate to the now-absent target with no-replace semantics.
7. Reopen and verify final identity, length, digest, and permission mode.
8. Publish the installed state.

For a new target, revalidate absence and parent identity, apply the recorded new
file mode, then perform steps 6–8.

The required primitive is `renameat2(RENAME_NOREPLACE)` on Linux and
`renamex_np(RENAME_EXCL)` on macOS, through a small reviewed abstraction. It is
used for target-to-backup, candidate-to-target, and backup-to-target restoration.
There is no check-then-replacing-rename fallback.

## 8.6 Recovery classification

Before any recovery mutation, classify every recorded target, candidate, and
backup location using file type, parent identity, physical identity, length, and
digest. Classification results are:

```text
original
candidate
absent
unexpected
```

The exact expected combinations follow from the last valid state:

| Last recorded stage | Valid filesystem lag | Complete | Rollback |
|---|---|---|---|
| Preparing | authorized candidate names may be absent or partial | no | delete transaction-owned candidates, then remove record |
| Prepared | originals at targets, candidates staged | yes | remove candidates |
| Committing, untouched existing target | original may be at target or backup | yes | restore original |
| Committing, untouched absent target | target may be absent or contain the recorded candidate | yes | restore absence |
| Committing, after backup record | original at backup, candidate staged or installed | yes | remove installed candidate and restore original |
| Committing, after install record | candidate at target, original at backup when applicable | yes | remove candidate and restore original |
| Committed | verified candidates at all targets | cleanup only | no |
| RollingBack | a suffix may still be installed/backed up | no | continue rollback |
| RolledBack | all original states restored | cleanup only | no |

Recovery adopts a candidate created during `Preparing` only for deletion during
rollback, never for completion. The transaction directory is mode `0700`, names
are manifest-authorized, and the threat model excludes a hostile same-user writer.

Any unexpected type, identity, digest, parent, unknown entry, state gap, bad
checksum, or ambiguous combination stops before mutation with
`RECOVERY_CONFLICT` or `TRANSACTION_RECORD_CORRUPT` as appropriate.

A canonical transaction directory with no `manifest.rec` is an `orphan_record`
only when it contains nothing or the manifest publication temporary. Completion
is forbidden; explicit rollback may remove only those entries and the directory.
A valid manifest with no state zero is `manifest_only` and has the same rollback-
only policy. Unknown contents are corruption and are never deleted automatically.

## 8.7 Cleanup

After `Committed` or `RolledBack`, atomically rename the entire active transaction
directory without replacement into `completed` using the corresponding exact
suffix, then sync both parent directories. Only after that rename may cleanup
delete backups, candidates, records, and finally the completed directory.

A crash before the rename leaves the terminal state record in the active directory.
A crash after it leaves a suffix-classified cleanup-only directory. A bounded
cleanup may remove only grammar-valid transaction artifacts; an unknown child is
`TRANSACTION_RECORD_CORRUPT`. Neither case permits another target mutation. A new
commit may finish validated completed-directory cleanup, but any active directory
still requires an explicit recovery command.

---

# 9. Resource limits

Defaults are trusted local configuration and may be lowered, not raised, in
v0.1.0.

| Resource | Limit |
|---|---:|
| JSON request bytes | 4 MiB |
| JSON nesting depth | 64 |
| Serialized JSON response | 16 MiB |
| Operations per batch | 1,000 |
| Distinct operation paths | 1,000 |
| UTF-8 relative path | 4,096 bytes |
| Individual snapshot file | 256 MiB |
| Total snapshot bytes | 1 GiB |
| Total line count | 5,000,000 |
| Line-index memory | 256 MiB |
| Total charged planning memory | 2 GiB |
| Resulting bytes per output | 512 MiB |
| Total planned output bytes | 1 GiB |
| Segments per output | 100,000 |
| Total segments | 250,000 |
| Changed transaction targets | 100 |
| Projected transaction disk use | 3 GiB |
| Manifest or state record | 16 MiB |
| State records per transaction | 512 |
| Cumulative state-record bytes | 128 MiB |
| Transaction directories scanned | 100 |
| Recovery bytes read per command | 256 MiB |
| Human or JSON diff bytes | 4 MiB |

Use checked arithmetic before allocation or I/O. Resource accounting belongs at
the first layer that knows the value: protocol limits in Phase 2, snapshot/index
limits in Phase 3, output/segment limits in Phase 4, and transaction limits in
Phase 5. Boundary tests may use accounting test doubles instead of allocating the
full limit.

Diffs are reporting-only. NUL-containing or invalid-UTF-8 files receive a binary
summary with lengths, digests, and bounded base64 samples. Text diffs preserve and
label original terminator kinds. Detailed diff input is limited to 8 MiB per side
and 10,000,000 explicitly counted algorithm work units; use a bounded-memory
algorithm rather than a quadratic matrix. Diff truncation never changes plan
hashes or commit behavior.

---

# 10. Implementation phases

# Phase 1 — Workspace, contracts, and test foundation

## Goal

Create a compiling workspace with dependency boundaries and turn the plan's
public contracts into short authoritative docs. Do not implement editing.

## In scope

* Create the five crates in Section 5.1.
* Pin stable Rust and add rustfmt/clippy configuration.
* Add typed placeholder errors and the core domain type skeleton.
* Add CI for Linux x86_64 and macOS arm64.
* Create the documentation and checkpoint skeleton from Section 5.1. Protocol
  schemas are completed in Phase 2 and transaction schemas in Phase 5.
* Copy normative behavior from this plan into those docs without expanding scope.
* Add architecture tests or metadata checks that enforce dependency direction.
* Establish fixture/golden/scenario directories and common test helpers.

## Out of scope

JSON parsing, filesystem reads, line indexing, planning, locking, and mutation.

## Acceptance

* Workspace metadata proves the dependency graph.
* `codesplice-core` has no filesystem or protocol dependency.
* Production crates contain no editing behavior and no direct unsafe code.
* Docs agree with this plan and contain no `TBD` product decisions.
* Common commands pass.

## Checkpoint commands

```bash
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

Stop after writing `notes/checkpoints/phase-1.md` and committing Phase 1.

---

# Phase 2 — Protocol v1 and CLI shell

## Goal

Parse every v0.1 request and command into validated domain values without touching
the filesystem.

## In scope

* Implement the request and response JSON Schemas.
* Reject duplicate keys, unknown fields, invalid enums, invalid numbers, and bad
  digest spelling before domain conversion.
* Implement DTO-to-domain conversion and request-level resource accounting.
* Implement the complete command grammar from Section 6.2.
* Keep execution commands wired to an explicit development-only unimplemented
  path using registered `INTERNAL_ERROR`; do not fake success. Read-only recovery
  and target-independent orphan cleanup remove their stubs in Phase 5, preview in
  Phase 6, and target-mutating commit/recovery in Phase 7.
* Implement `capabilities` and `protocol-version` fully.
* Implement centralized error/warning DTOs, exit mapping, JSON stdout discipline,
  redaction, and terminal-safe human escaping.
* Add golden tests for all request variants, errors, warnings, and commands.

## Acceptance

* Valid move/copy and every selector/anchor/precondition variant deserialize.
* Duplicate keys, unknown fields, missing preconditions, bad digests, oversized
  requests, excess depth, and operation/path count boundaries fail correctly.
* Commit requires exactly one expected-plan policy.
* Protocol conversion performs no filesystem access.
* Every registered code maps to one documented exit category.

## Checkpoint commands

Run the common four commands plus:

```bash
cargo test -p codesplice-protocol
cargo test -p codesplice-cli --test protocol_cli
```

Stop after `notes/checkpoints/phase-2.md` and the Phase 2 commit.

---

# Phase 3 — Workspace inspection and immutable snapshots

## Goal

Safely acquire all planning inputs and make `inspect` useful, with no filesystem
writes.

## In scope

* Resolve and retain the canonical workspace root identity.
* Implement path parsing, reserved-path rejection, no-symlink traversal, regular-
  file enforcement, parent identity capture, and existing-file alias detection.
* Implement POSIX `(device, inode)` identities on Linux and macOS.
* Capture link count for each existing file so the planner can reject only files
  that would be destructively replaced while multiply linked.
* Acquire each file through one open handle per attempt: metadata before, bounded
  read/hash, metadata after, then parent-entry identity confirmation.
* Retry a demonstrably unstable read at most twice after the first attempt. A
  stable wrong digest is a precondition conflict, not a retry.
* Represent absent destinations with their validated parent and basename.
* Implement the compact LF/CRLF/CR line index in core.
* Enforce snapshot, identity, line-count, index-memory, and aggregate limits.
* Implement `inspect` using the same acquisition primitives.

## Acceptance

* Snapshot bytes are immutable shared data owned by core types.
* Empty, unterminated, LF, CRLF, lone-CR, mixed, non-UTF-8, and long-line fixtures
  index correctly.
* Root/path symlinks, reserved paths, special files, aliases, missing parents,
  stale digests, and limit violations are rejected. Link count is retained for
  Phase 4; a multiply linked copy-only source remains readable.
* Mutation during reading retries boundedly and then returns `FILE_CHANGED`.
* Inspect and snapshot acquisition create and modify nothing.

## Checkpoint commands

Run the common commands plus:

```bash
cargo test -p codesplice-core line_index
cargo test -p codesplice-fs snapshot
cargo test -p codesplice-cli --test inspect_cli
```

Stop after `notes/checkpoints/phase-3.md` and the Phase 3 commit.

---

# Phase 4 — Pure planner and plan digest

## Goal

Resolve a batch into deterministic segment recipes and a golden-tested plan hash.

## In scope

* Implement selector and anchor resolution.
* Implement every conflict rule and the single event sweep in Section 4.5.
* Build resolved-operation and planned-output records without materializing outputs.
* Stream output length, SHA-256, and byte-equality comparison from segments.
* Classify unchanged/modified/created/emptied outputs.
* Reject every non-unchanged existing output whose snapshot link count is not one.
* Enforce output, segment, target, projected response, and total planning limits.
* Complete the selector/anchor/precondition discriminant table in
  `docs/specification.md` without changing Section 7.4's record structure.
* Implement deterministic CBOR plan encoding and golden byte/digest vectors.
* Property-test exact payload equality and determinism.

## Required fixture groups

* same-file forward, backward, start no-op, and end no-op moves;
* cross-file move and copy;
* overlapping copies and rejected overlapping moves;
* insertions at both deletion boundaries and rejection inside deletion;
* adjacent deletions, repeated offsets, EOF insertion, and whole-file move;
* copy from bytes another move removes;
* new-file multiple insertion ordering;
* an effectful edit whose final bytes equal the original; and
* every resource boundary through accounting test doubles.

## Acceptance

* The planner is deterministic and has no filesystem dependency.
* Selected payload digest equals every corresponding inserted segment digest.
* No complete output buffer is retained in `EditPlan`.
* Byte-identical results do not become transaction targets.
* Golden encodings are annotated sufficiently to diagnose a one-byte drift.

## Checkpoint commands

Run the common commands plus:

```bash
cargo test -p codesplice-core planner
cargo test -p codesplice-core plan_hash
```

Stop after `notes/checkpoints/phase-4.md` and the Phase 4 commit.

---

# Phase 5 — Lock, journal, and recovery classifier

## Goal

Implement persistent transaction records and recovery reasoning without mutating
user targets.

## In scope

* Implement secure-enough-for-threat-model control-directory creation/validation.
* Implement nonblocking exclusive mutation locking and shared diagnostic locking.
* Implement transaction ID/name generation and collision handling.
* Implement record envelope encoding, checksums, exclusive publication, fsync
  ordering, full state snapshots, and checksum-chain folding.
* Implement manifest and state schemas with size/count limits.
* Implement the state machine as a pure transition validator.
* Implement read-only `recover --list` and `recover ID --status`.
* Implement target-independent rollback cleanup for canonical `orphan_record` and
  `manifest_only` states.
* Implement recognition and bounded deletion of suffix-classified cleanup-only
  directories under `.codesplice/completed`.
* Implement recovery classification against synthetic filesystem observations.
* Add test-only subprocess failpoint plumbing; production builds cannot activate it.
* Scan for unfinished transactions before allowing a new transaction.

## Out of scope

Candidate creation from plan segments, target backup, target installation, and any
recovery action that changes a user target.

## Acceptance

* Torn, truncated, oversized, gapped, forked, or checksum-invalid chains are
  corruption and never guessed through.
* Unknown transaction entries are never deleted automatically.
* An active transaction blocks a new changing commit; only validated completed-
  directory cleanup may run as part of the new-commit gate.
* Lock contention and read-only status behavior are deterministic.
* State-machine and classifier tables have exhaustive unit tests.
* No test in this phase renames, removes, or replaces a user target.

## Checkpoint commands

Run the common commands plus:

```bash
cargo test -p codesplice-fs journal
cargo test -p codesplice-fs recovery_classifier
cargo test -p codesplice-cli --test recovery_status_cli
```

Stop after `notes/checkpoints/phase-5.md` and the Phase 5 commit.

---

# Phase 6 — Preview and reporting

## Goal

Expose the complete planning pipeline without intentional filesystem mutation.

## In scope

* Wire `apply --preview` to protocol, snapshot, planner, and report conversion.
* Implement human and JSON resolved-operation/output reports.
* Implement bounded text diff and binary summary behavior.
* Implement `--no-diff` without changing plan output or digest.
* If `.codesplice/lock` exists and is valid, use the Phase 5 nonblocking shared
  lock through control scan, snapshot, and report. If it does not exist, create
  nothing and emit `OBSERVATION_MAY_BE_STALE`.
* Return `plan_hash_version`, `plan_sha256`, and workspace identity hash.
* Keep commit and target-mutating recovery disabled.

## Acceptance

* Preview creates, removes, renames, or writes no file and intentionally changes
  no permission or timestamp. Access time is excluded.
* Preview under an active exclusive CodeSplice lock returns `TRANSACTION_BUSY`.
* Preview/inspect under a quiescent unfinished transaction return
  `TRANSACTION_RECOVERY_REQUIRED` rather than planning the partial workspace.
* JSON stdout is one parseable value plus LF with no prose.
* Terminal controls cannot be injected through any human-rendered field.
* Binary, mixed-terminator, truncated, and disabled diffs pass.

## Demonstration

Run one batch containing a real move, copy, no-op move, and new destination. Show
the resolved coordinates, payload/output digests, plan hash, and unchanged
workspace tree before/after.

## Checkpoint commands

Run the common commands plus:

```bash
cargo test -p codesplice-cli --test preview_cli
cargo test --test preview_read_only
```

Stop after `notes/checkpoints/phase-6.md` and the Phase 6 commit.

---

# Phase 7 — Single-target commit and recovery

## Goal

Use the complete transaction engine for exactly one changed target.

## In scope

* Add a temporary validation guard that rejects plans with more than one changed
  target; Phase 8 removes it.
* Implement the two-pass expected-plan flow and no-op fast path.
* Create the manifest and Preparing state before candidates.
* Stream a candidate from plan segments, hash/verify it, sync it, record its
  identity, and publish Prepared.
* Revalidate every input, absence, root, and parent before mutation.
* Implement reviewed Linux/macOS no-replace rename primitives.
* Implement existing-target and new-target commit order from Section 8.5.
* Preserve current POSIX permission bits and use recorded startup umask for new
  targets.
* Implement automatic rollback after post-mutation failure.
* Implement complete/rollback recovery for every single-target state.
* Implement the terminal active-to-completed directory rename and cleanup.
* Add subprocess crash failpoints before and after every record publication,
  candidate step, rename, verification, rollback step, and cleanup step.

## Acceptance

* Expected-plan mismatch and pre-lock plan change create no transaction.
* A successful no-op creates no `.codesplice` tree and refuses a quiescent active
  transaction without writing it.
* An external destination collision is never overwritten during install or restore.
* Source-only inputs are rehashed before the first target mutation.
* Every injected crash reaches all-old, all-new, or explicit conflict in a fresh
  recovery process.
* Candidate replacement with equal bytes but different identity is rejected.
* Existing modes and all startup-umask fixture modes are correct.
* Exact payload digests are present in the final report.

## Demonstration

Demonstrate a same-file effectful move, a copy into an existing target, a copy into
a new target, permission change between preview and commit, expected-plan mismatch,
crash after backup, crash after install, complete recovery, and rollback recovery.
Cross-file move waits for Phase 8 because it changes both source and destination.

## Checkpoint commands

Run the common commands plus:

```bash
cargo test -p codesplice-fs single_target
cargo test --test single_target_commit
cargo test --test single_target_crash_recovery
```

Stop after `notes/checkpoints/phase-7.md` and the Phase 7 commit.

---

# Phase 8 — Multi-target commit and recovery

## Goal

Generalize the proven transaction machinery to all changed outputs in one plan.

## In scope

* Remove the one-target guard without creating a second engine.
* Sort targets by normalized path UTF-8 bytes and persist `target_index`.
* Prepare and verify every candidate before mutating any target.
* Revalidate every input once more before publishing `Committing`.
* Commit targets in deterministic order and record after each backup/install.
* On any post-mutation error, classify the whole transaction and roll back in
  reverse target order.
* Implement multi-target complete/rollback recovery and terminal cleanup.
* Include mixed old/new visibility explicitly in reports and docs.
* Exercise failpoints at every target index and record boundary.

## Acceptance

* Single-target tests still use the same implementation and pass unchanged.
* No target mutation begins before all candidates and inputs verify.
* Failure at any target attempts rollback of every earlier affected target.
* Incomplete rollback returns `TRANSACTION_RECOVERY_REQUIRED`, never a misleading
  original per-file error alone.
* Recovery mutates nothing when any target classification is unexpected.
* Completion yields every planned digest; rollback restores every original digest
  and absence state.

## Demonstration

Interrupt a three-target transaction after the first install. In fresh processes,
show status, rollback, complete from an equivalent second fixture, and conflict
after a third-party modification.

## Checkpoint commands

Run the common commands plus:

```bash
cargo test -p codesplice-fs multi_target
cargo test --test multi_target_commit
cargo test --test multi_target_crash_recovery
```

Stop after `notes/checkpoints/phase-8.md` and the Phase 8 commit.

---

# Phase 9 — Hardening and platform qualification

## Goal

Prove the declared guarantees on the supported platform/filesystem pilot matrix.

## In scope

* Run Linux x86_64/ext4 and macOS arm64/APFS integration suites on local test
  filesystems. Other filesystems remain unclaimed unless the same suite passes and
  this plan plus platform documentation are amended.
* Verify filesystem detection rejects known network/virtual and cross-device cases.
* Fuzz JSON decoding, line indexing, selector/anchor resolution, event composition,
  deterministic CBOR decoding/encoding, record decoding, state folding, recovery
  classification, and human escaping.
* Add property tests for all invariants below.
* Run crash failpoints systematically in subprocesses.
* Measure 1 MiB, 100 MiB, 1/10/100 targets, 1/1,000/100,000 segments, and limit
  rejection without setting aspirational timing gates before a baseline exists.
* Record measured methodology and results in `docs/performance-baseline.json`.
* Audit all unsafe code; only the reviewed no-replace platform shim may contain it.

## Required invariants

```text
selected bytes == inserted bytes
selected digest == inserted digest
same snapshot + request == same plan digest
preview performs no intentional mutation
expected-plan rejection creates no transaction artifact
every committed target == planned length and digest
recovery reaches all-old, all-new, or explicit conflict
no no-replace collision overwrites an entry
```

## Acceptance

* Sanitizers/platform equivalents report no issue in supported jobs.
* Fuzzing runs a documented bounded campaign with no crash, panic, uncontrolled
  allocation, nontermination, or invariant failure; regressions are checked in.
* Every resource limit has below/at/above tests.
* The filesystem support table names only configurations actually exercised.
* The performance baseline reports measurements rather than invented targets.
* Security and metadata docs state the v0.1 threat-model boundary plainly.

## Checkpoint commands

Run the common commands, all integration tests, the documented fuzz regression
corpus, and the platform qualification script created in this phase.

Stop after `notes/checkpoints/phase-9.md` and the Phase 9 commit.

---

# Phase 10 — Codex pilot and v0.1.0 release

## Goal

Validate the agent workflow on real repositories, then package the qualified
pilot without expanding its claims.

## Agent workflow

```bash
codesplice --workspace /path/to/repo inspect \
  --path src/source.rs --path src/destination.rs --json

codesplice --workspace /path/to/repo apply \
  --request split.json --preview --json

codesplice --workspace /path/to/repo apply \
  --request split.json --commit \
  --expect-plan sha256:PREVIEWED_PLAN --json
```

Agents may not use `--accept-current-plan` during the qualification pilot.

## Pilot scenarios

1. Move a function to an existing file.
2. Move a function to a new file.
3. Copy a declaration.
4. Reorder code in one file.
5. Execute a same-file no-op.
6. Split one file into two outputs.
7. Split one file into three outputs.
8. Preserve CRLF and mixed terminators.
9. Move non-UTF-8 payload bytes.
10. Reject a stale source digest.
11. Reject an expected-plan mismatch.
12. Recover an interrupted multi-file commit by completion.
13. Recover an interrupted multi-file commit by rollback.
14. Preserve a permission change made after preview but before the locked commit
    snapshot.
15. Reject symlink traversal, hard-link targets, `.codesplice` targeting, a
    cross-device transaction, and an external no-replace collision.

## Release acceptance

* Phases 1–9 have passing checkpoint reports.
* All pilot scenarios pass on at least one qualified Linux and one qualified macOS
  environment.
* No unresolved exactness, overwrite, rollback, path-escape, or record-corruption
  defect exists.
* Schema, plan-hash version, record version, errors, warnings, limits, support
  matrix, and metadata exclusions are documented.
* Packages identify Linux x86_64 and macOS arm64 only.
* The tag is `v0.1.0`; protocol version 1 becomes closed at that tag.

Stop after `notes/checkpoints/phase-10.md`, the release commit, and the release tag.

---

# 11. Common verification

Every implementation phase runs:

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo build --workspace --all-features
```

When a platform-specific test cannot run in the current session, the checkpoint
must name the CI job that ran it. A phase cannot pass if neither local evidence nor
CI evidence exists for a required platform behavior.

No checkpoint may hide failures by weakening a test, broadening a retry, replacing
a no-replace operation, or changing a declared guarantee without a plan amendment.

---

# 12. Successful-operation definition

A successful changing operation has:

1. a fresh snapshot satisfying all explicit path preconditions;
2. one deterministic resolved plan and plan digest;
3. user or agent commitment to that digest;
4. a journal published before candidate creation;
5. every candidate verified before the first target mutation;
6. every input revalidated immediately before mutation;
7. no-replace backup, install, and restore operations;
8. final files matching all planned lengths and digests;
9. each selected payload digest matching its inserted payload digest; and
10. a terminal state that is committed, rolled back, or explicitly recoverable.

If CodeSplice cannot prove one of these conditions, it fails closed within the
v0.1 threat model and leaves enough validated journal state for explicit recovery.
