# CodeSplice Technical Implementation Plan

**Status:** Revised after fifth technical review; no-go for Phase 1 until every Phase 0 blocker and evidence item passes
**Implementation language:** Rust
**Working binary name:** `codesplice`
**Initial release target:** `v0.1.0`

---

# 1. Instructions for the implementing agent

Implement this project one phase at a time.

At the end of every phase:

1. Run every required checkpoint command.
2. Demonstrate the required behavior.
3. Produce a checkpoint report.
4. Commit the completed phase separately.
5. Stop before beginning the next phase.

Do not silently alter the specification.

When implementation reveals an ambiguity or conflict:

1. Stop the current phase.
2. Describe the conflict precisely.
3. Identify the affected specification sections.
4. Propose one or more amendments.
5. Mark the checkpoint `PASS WITH CONCERNS` or `FAIL`.
6. Do not proceed until the specification is amended.

## Checkpoint report template

```text
Phase:
Status: PASS | PASS WITH CONCERNS | FAIL

Commit:
Files added:
Files changed:

Commands executed:
- ...

Tests:
- Passed:
- Failed:
- Skipped:

Demonstration:
- ...

Specification deviations:
- None | ...

Known limitations:
- ...

Risks discovered:
- ...

Recommended next action:
- Approve next phase
- Revise current phase
- Amend specification
```

---

# 2. Product objective

CodeSplice is a filesystem-editing tool for coding agents.

It moves or copies existing code without requiring the agent to reproduce the selected content.

The defining guarantee is:

> In exact mode, the bytes inserted at the destination are identical to the bytes selected from the source snapshot.

The selected and inserted payloads must have the same SHA-256 digest.

CodeSplice is intended for tasks such as:

* Moving a function between files.
* Moving a class or declaration into another module.
* Reordering code within one file.
* Splitting a large file into multiple files.
* Copying an existing declaration without regenerating it.
* Applying several coordinated movements as one recoverable transaction.

CodeSplice does not initially:

* Fix imports.
* Update references.
* Format code.
* Reindent code.
* Normalize line endings.
* Parse programming languages.
* Print or regenerate syntax trees.
* Create parent directories.
* Preserve every form of filesystem metadata.

---

# 3. Release roadmap

## `v0.1.0` — Exact transfer engine

The first release includes:

```text
inspect
apply
move
copy
recover
doctor
capabilities
protocol-version
line selectors
byte selectors
line anchors
byte-offset anchors
preview
human-readable diffs
JSON request and response protocol
input digest preconditions
new-file absence preconditions
plan digests
expected-plan commit preconditions
single-target recoverable transactions
multi-target recoverable transactions
workspace mutation locking
transaction inspection and recovery
workspace lock diagnosis and explicit repair
exact payload digest reporting
```

## `v0.2.0` — Additional exact-edit operations

Deferred until `v0.1.0` is released:

```text
delete
insert
swap
extract
```

Because protocol v1 is closed, exposing these operations on the wire requires protocol v2 (or a later major protocol), even if the executable release remains named `v0.2.0`.

## `v0.3.0` — Structural selection

Deferred until the exact-transfer engine succeeds in real agent usage:

```text
Tree-sitter query selectors
symbol selectors
trivia expansion
language capability registry
```

## Later releases — Semantic refactoring

Potential future capabilities:

```text
update imports
remove obsolete imports
update references
leave compatibility re-exports
detect dependency cycles
move related tests
language-server integration
```

Semantic transformations must remain separate from the exact-transfer guarantee.

---

# 4. Core guarantees

## 4.1 Exact content guarantee

Exact mode guarantees file-content bytes only.

It does not automatically change:

* Indentation.
* Whitespace.
* Line endings.
* Encoding.
* Comments.
* Documentation.
* Attributes.
* Decorators.
* Imports.
* Syntax.

The required invariant is:

```text
SHA256(selected source payload)
==
SHA256(inserted destination payload)
```

## 4.2 Metadata guarantee

The word “exact” does not mean that every property of the filesystem object is preserved.

Documentation must state:

> Exact mode guarantees selected and inserted file-content bytes. Filesystem metadata preservation is limited to explicitly documented fields and platform behavior.

For `v0.1.0`:

* Preserve current ordinary permission bits when modifying an existing file where supported.
* Read those permission bits from the actual original object after it has been moved to the transaction backup and validated.
* Apply those current permission bits to the candidate before installation.
* Do not silently restore permission bits captured during an earlier preview or planning snapshot.
* Do not promise preservation of ownership, ACLs, extended attributes, timestamps, alternate data streams, resource forks, platform flags, Windows read-only attributes, or hard-link relationships.
* Inspect link count and supported security metadata before planning a destructive replacement. This inspection is charged to the planning-resource budget and its result is revalidated before the first target mutation.
* A target with multiple hard links, a nontrivial ACL, security-relevant extended attributes, a macOS resource fork, a Windows alternate data stream, ownership that the installed candidate will not preserve, platform flags, or a Windows read-only attribute is rejected by default with `METADATA_LOSS_REQUIRES_OPT_IN`.
* A trusted caller may proceed only with the explicit execution option `--allow-metadata-loss`. The report must then emit the applicable stable warnings (`HARD_LINK_RELATIONSHIP_BROKEN`, `ACL_NOT_PRESERVED`, `SECURITY_XATTR_NOT_PRESERVED`, `RESOURCE_FORK_NOT_PRESERVED`, `ALTERNATE_STREAM_NOT_PRESERVED`, `OWNERSHIP_NOT_PRESERVED`, `FILE_FLAGS_NOT_PRESERVED`, or `WINDOWS_READ_ONLY_NOT_PRESERVED`) and structured before/after metadata classes without exposing sensitive ACL principals by default.
* The flag is not request-controlled, is excluded from content planning, and never suppresses metadata revalidation. An uninspectable metadata class fails closed on platforms where Phase 0 classifies that inspection as required.
* Reject multiple requested paths that identify the same existing physical file.
* Freeze Unix ACL/xattr/flag detection, macOS resource-fork behavior, Windows inherited-ACL and read-only-attribute behavior, and every unsupported case in `docs/metadata.md` and `docs/platform-support.md` before Phase 1.

## 4.3 Immutable initial snapshot

All selectors and destination anchors in one batch resolve against one initial workspace snapshot.

An earlier operation must not change the meaning of a later operation.

## 4.4 Planning and mutation separation

The architecture must separate:

1. Establishing the workspace root.
2. Reading and validating snapshots.
3. Resolving selectors and anchors.
4. Detecting conflicts.
5. Constructing an immutable edit plan.
6. Previewing the plan.
7. Creating a transaction record.
8. Preparing candidate files.
9. Committing the transaction.
10. Recovering an interrupted transaction.

Planning and preview must not intentionally mutate the filesystem.

## 4.5 Preview guarantee

Do not promise that reading files leaves all filesystem metadata unchanged.

The supported guarantee is:

> Preview performs no intentional filesystem mutation. It creates no lock, candidate, backup, journal, destination file, or control-directory artifact and does not intentionally change permissions or timestamps. Filesystem-managed access-time behavior is platform and mount dependent.

Preview checkpoints must verify:

* No file-content changes.
* No creation or removal.
* No rename.
* No permission change.
* No explicit timestamp update.
* No `.codesplice` artifact.
* No modification-time change caused by a CodeSplice write.

Access time is excluded from the portable guarantee.

## 4.6 Recoverable commits

Every commit, including a one-target commit, uses the same versioned transaction model.

Do not create a separate non-journaled single-file replacement implementation.

The supported guarantee is:

> Every transaction is recorded before candidate creation or target replacement. Partial commit states are detectable. An interrupted transaction can be inspected and either completed or rolled back unless an external modification creates an explicit conflict.

A multi-target transaction is **recoverable but not atomically visible** to unrelated readers. Between per-target installations, an unrelated process may observe a mixture of old and new files. Do not describe a transaction as “atomic” without the qualifier “record-atomic” or “recoverable.” Recovery converges to all-old or all-new when no external conflict exists; it does not make intermediate filesystem visibility atomic.

## 4.7 Concurrency guarantee

CodeSplice uses a workspace mutation lock to coordinate cooperating CodeSplice processes.

It also validates:

* Workspace identity.
* Parent-directory identity.
* File identity.
* File type.
* File digest.
* Destination absence.
* Transaction state.

Documentation must state:

> CodeSplice protects against cooperating CodeSplice processes and detects ordinary external modifications. Absolute protection against hostile filesystem races may require platform-specific APIs beyond portable standard-library operations.

After all candidates reach `Prepared` and immediately before the first target mutation, commit must revalidate **every input record from the immutable snapshot**, not only files that are themselves targets. This includes unchanged copy sources, move sources, existing destinations, absent destinations, parent identities, required metadata, and workspace identity. Existing files are reopened through the secure parent handle and fully rehashed unless a Phase 0 platform proof defines a retained secure handle plus change-detection primitive with equivalent or stronger guarantees. Any mismatch aborts before mutation with a registered conflict.

---

# 5. Workspace architecture

```text
codesplice/
├── Cargo.toml
├── Cargo.lock
├── rust-toolchain.toml
├── README.md
├── LICENSE
├── docs/
│   ├── specification.md
│   ├── protocol.md
│   ├── transaction-model.md
│   ├── security.md
│   ├── metadata.md
│   ├── resource-limits.md
│   ├── platform-support.md
│   ├── agent-integration.md
│   └── adr/
├── crates/
│   ├── codesplice-core/
│   ├── codesplice-fs/
│   ├── codesplice-protocol/
│   ├── codesplice-cli/
│   └── codesplice-test-support/
├── tests/
│   ├── fixtures/
│   ├── golden/
│   └── scenarios/
└── .github/
    └── workflows/
```

A Tree-sitter crate is not introduced before `v0.3.0`.

---

# 6. Crate ownership and dependency direction

## 6.1 Dependency graph

```text
codesplice-core
    ↑
    ├── codesplice-fs
    ├── codesplice-protocol
    └── codesplice-cli

codesplice-cli depends on:
    codesplice-core
    codesplice-fs
    codesplice-protocol
```

There must be no dependency from `codesplice-core` to `codesplice-fs`.

There must be no dependency from `codesplice-fs` to `codesplice-protocol`.

## 6.2 `codesplice-core` ownership

`codesplice-core` owns the immutable planning model:

```text
BatchSpecification
Operation
MoveOperation
CopyOperation
SourceSelection
Destination
FilePrecondition
Selector
Anchor
ByteRange
WorkspacePath
PathEquivalenceKey
WorkspaceSnapshot
FileSnapshot
SnapshotFileId
PlannedFileId
WorkspaceIdentityToken
FilePlanningMetadata
LineIndex
CompactLineOffsets
PackedLineTerminatorKinds
ResolvedOperation
EditPlan
PlannedOutput
OutputSegment
PlanDigest
PlanError
```

### Core snapshot model

```rust
pub struct WorkspaceSnapshot {
    pub workspace_identity: WorkspaceIdentityToken,
    pub files: BTreeMap<PathEquivalenceKey, FileSnapshot>,
    pub absent_paths: BTreeMap<PathEquivalenceKey, WorkspacePath>,
    pub path_keys: BTreeMap<WorkspacePath, PathEquivalenceKey>,
}

pub struct FileSnapshot {
    pub path: WorkspacePath,
    pub path_equivalence_key: PathEquivalenceKey,
    pub id: SnapshotFileId,
    pub bytes: Arc<[u8]>,
    pub sha256: Digest,
    pub planning_metadata: FilePlanningMetadata,
    pub line_index: LineIndex,
}

pub struct SnapshotFileId {
    pub kind: IdentityKindToken,
    pub canonical_bytes: Arc<[u8]>,
}

pub enum PlannedFileId {
    Existing(SnapshotFileId),
    New(NewPlannedFileId),
}
```

`SnapshotFileId` is opaque to the planner.

The core may compare and serialize the token but must not interpret platform-specific identity fields.

`PathEquivalenceKey` is likewise opaque, byte-orderable, and supplied by `codesplice-fs`; the core uses it for grouping, duplicate rejection, deterministic ordering, and plan encoding without reimplementing platform path comparison.

## 6.3 `codesplice-fs` ownership

`codesplice-fs` owns:

```text
SecureWorkspaceRoot
PlatformWorkspaceIdentity
PlatformFileIdentity
PlatformDirectoryIdentity
SnapshotReader
SnapshotAcquisition
CommitContext
secure path validation
metadata acquisition
platform identity extraction
conversion into core snapshots
workspace lock
transaction directories
candidate creation
no-replace rename
backup handling
manifest persistence
state persistence
commit
rollback
recovery
```

The filesystem layer converts:

```text
PlatformFileIdentity
→ SnapshotFileId
```

through a stable canonical encoding.

Snapshot acquisition may return:

```rust
pub struct SnapshotAcquisition {
    pub planning_snapshot: codesplice_core::WorkspaceSnapshot,
    pub commit_context: CommitContext,
}
```

`CommitContext` contains filesystem-layer information that the pure planner does not need, such as securely resolved parent identities and platform handles or tokens.

On supported platforms, mutation primitives must be relative to securely opened workspace or parent-directory handles whenever the operating system exposes the required semantics. A path-based fallback is permitted only for a non-rename operation whose documented platform primitive cannot be made handle-relative, and only after immediate parent and target identity revalidation. Such fallback must be named in `docs/platform-support.md`; it may not silently weaken the guarantee. Target-to-backup, candidate-to-target, and backup-to-target restoration never use this exception.

## 6.4 `codesplice-protocol` ownership

`codesplice-protocol` owns:

```text
RequestEnvelopeDto
BatchSpecificationDto
OperationDto
ResponseEnvelopeDto
ErrorDto
WarningDto
CapabilitiesDto
RecoveryRequestDto
RecoveryResponseDto
DoctorRequestDto
DoctorResponseDto
DTO-to-domain conversion
domain-to-response conversion
protocol compatibility validation
```

It must not perform filesystem access or mutation.

## 6.5 `codesplice-cli` ownership

`codesplice-cli` owns:

```text
argument parsing
workspace-root establishment
command orchestration
preview-versus-commit execution
expected-plan handling
trusted metadata-loss option handling
doctor orchestration
human-readable output
JSON output
exit codes
```

---

# 7. Workspace-root and control-directory model

## 7.1 CLI-established workspace root

The workspace root is established out of band by the CLI.

Supported forms:

```bash
codesplice --workspace /path/to/repo ...
```

or:

```bash
cd /path/to/repo
codesplice ...
```

Rules:

* `--workspace` may be absolute.
* A relative `--workspace` is resolved against the process working directory.
* It is never resolved relative to the JSON request file.
* The workspace root must already exist.
* It must be a directory.
* The root path and its inspected path components must not be symlinks, junctions, mount aliases, or unsupported reparse points under the documented platform policy.
* The physical identity of the root directory is captured.
* Root identity is revalidated before candidate creation, commit, rollback, and recovery mutation.

## 7.2 Protocol workspace field

Protocol v1 retains:

```json
{
  "workspace_root": "."
}
```

For `v0.1.0`:

* The only accepted semantic value is `"."`.
* `"."` refers to the workspace root established by the CLI.
* An absolute path in the request is rejected.
* A different relative path is rejected.
* An untrusted request cannot select another workspace.

The CLI may later support a protocol mode where the field is omitted, but protocol v1 must freeze one unambiguous representation.

## 7.3 Workspace identity

The filesystem layer creates an opaque canonical workspace identity token from platform directory identity.

The token is:

* Included in the core workspace snapshot.
* Included in `plan_sha256`.
* Included in transaction manifests.
* Revalidated during commit and recovery.

Replacing or redirecting the workspace root after planning causes a conflict.

## 7.4 Reserved control directory

The reserved directory is:

```text
<workspace>/.codesplice
```

Rules:

* Operation paths may never target `.codesplice` or any descendant.
* The restriction is enforced using the platform’s relevant case and normalization behavior.
* `.codesplice` must be a real directory.
* It must not be a symlink.
* It must not be a junction.
* It must not be an unsupported reparse point.
* If it exists as a non-directory, commit and recovery fail.
* If it exists as a symlink or reparse point, commit and recovery fail.
* It is created securely and exclusively when first needed.
* Preview does not create it.
* On Unix, use restrictive permissions such as `0700`, subject to documented behavior.
* On Windows, use the platform’s normal secure directory semantics and document ACL limitations.

## 7.5 Workspace mutation-lock bootstrap

The mutation lock is the persistent regular file:

```text
<workspace>/.codesplice.lock
```

Both `.codesplice.lock` and `.codesplice`, including every platform-equivalent spelling, are reserved. Operation paths may not name either path or their descendants. The lock is outside `.codesplice` so it can serialize the first creation and validation of the control directory.

Bootstrap and acquisition are normative:

1. Securely open the already-existing workspace root and capture its physical identity.
2. Relative to that root handle, open `.codesplice.lock` as a regular file with read/write access, close-on-exec, and no-follow/no-reparse semantics. If absent, attempt exclusive creation. Unix creation requests mode `0600` and immediately applies/verifies exact `0600` through the retained handle so an unusually restrictive umask cannot leave an unusable persistent lock; Windows uses a non-inheritable handle and the root directory’s normal ACL inheritance.
3. Reject a symlink, junction, unsupported reparse point, directory, special file, multiply linked file where link count is available, or a lock name whose `PathEquivalenceKey` is not the reserved key. On Unix, an existing lock must have link count `1`, be owned by the effective user ID, and have exactly owner read/write permission (`0600`) with no group, other, set-ID, or sticky bits. On Windows, Phase 0 must freeze the owner and DACL validation that prevents a less-privileged principal from replacing or writing the lock.
4. Acquire a non-blocking exclusive OS file lock on that opened handle. Contention returns `TRANSACTION_BUSY`; v1 does not steal locks or infer staleness from PID or time.
5. While holding the lock, revalidate the workspace-root identity and that the reserved directory entry still identifies the opened lock object.
6. If this invocation created the empty file, initialize the first slot while holding the lock. If an earlier creator crashed during first initialization, follow only the Phase 0-frozen recognizer/reinitialization rule for that exact partial shape; otherwise validate both complete checksummed slots and select the active generation.
7. The lock-identity record is a fixed-capacity, two-slot record in the existing lock-file object. Each independently checksummed slot contains magic `CODESPLICE-LOCK\0`, format version `1`, a monotonically increasing nonzero `generation`, and the canonical workspace identity token. The active slot is the valid slot with the highest generation. Equal-generation nonidentical slots, generation wrap, an oversized identity, or no valid slot outside the narrowly recognized first-initialization crash state is `WORKSPACE_LOCK_INVALID`. Phase 0 freezes the exact file length, slot offsets, padding, byte grammar, initialization crash states, and golden vectors.
8. A malformed active record or one bound to another workspace identity fails with `WORKSPACE_LOCK_INVALID`; normal commit and recovery never rewrite or replace it.
9. Only after these checks may commit or mutating recovery validate or create `.codesplice` and `.codesplice/transactions` relative to secure directory handles.
10. Hold the same open lock handle through planning revalidation, transaction preparation, commit or rollback, state persistence, and required cleanup. Releasing or closing the handle releases the OS lock.

Required native open/lock semantics are:

```text
Unix:
  first try openat(root_fd, ".codesplice.lock",
                   O_RDWR | O_CLOEXEC | O_NOFOLLOW | O_CREAT | O_EXCL, 0600)
  on EEXIST openat with O_RDWR | O_CLOEXEC | O_NOFOLLOW and no O_CREAT
  fstat regular-file/type, link count, identity, owner/mode policy
  acquire a whole-file exclusive advisory lock in non-blocking mode

Windows:
  a reviewed NtCreateFile wrapper with OBJECT_ATTRIBUTES.RootDirectory set to
  the validated root handle; use FILE_CREATE first, then FILE_OPEN only after
  STATUS_OBJECT_NAME_COLLISION
  request read/write and synchronization access, non-inheritable handle,
  FILE_SHARE_READ | FILE_SHARE_WRITE, no delete sharing, and open-reparse-point
  semantics without following the final component
  reject reparse tags and non-disk files; capture FileIdInfo
  LockFileEx exclusive + FAIL_IMMEDIATELY over the whole v1 lock range
```

`CreateFileW` is not a handle-relative substitute and is forbidden for this operation. An equivalent reviewed crate is acceptable only when it demonstrably preserves `NtCreateFile`/`RootDirectory`, every listed flag, and every failure semantic. Lock-record initialization flushes runtime buffers in Normal mode and additionally syncs the file and root directory in Durable mode.

The lock file persists after release. Normal cleanup never removes it. Preview, inspect, capabilities, protocol-version, `recover --list`, and `recover --status` do not create or acquire it. Concurrent first creators may both open the resulting file, but only the process that acquires the OS lock may initialize or validate it; all control-directory creation remains serialized.

The operator repair surface is:

```text
codesplice doctor --status [--json]
codesplice doctor --repair-lock --acknowledge-workspace-rebind [--json]
```

`doctor --status` is read-only and noncreating. If the lock path is absent, it reports that fact without creating it. If present, it securely opens the existing regular object and attempts a nonblocking shared diagnostic lock over the same v1 range. Failure to acquire it returns `TRANSACTION_BUSY`; after acquisition it validates the lock record and performs its bounded control scan. It never diagnoses an empty, partial, or invalid record while initialization or repair owns the exclusive lock. The shared lock is retained through the complete observation, so no stable-double-read substitute is needed.

Lock repair is deliberately narrower than normal commit. It securely opens and exclusively locks the existing regular lock object, validates the current root and control-directory identities, and performs a locked scan of every transaction entry. It refuses repair with `TRANSACTION_RECOVERY_REQUIRED` if any valid unfinished, committed-but-not-cleaned, corrupt, partially published, or unclassifiable transaction/control entry exists. It also refuses insecure lock ownership, mode, link count, type, or namespace races; those require manual operator remediation documented per platform. Only when the transaction directory is absent or proven empty may repair write the inactive or older fixed slot in the **same locked file object**, flush/sync it as required, reread and validate both slots, and select the new higher generation. The prior valid slot remains intact until the new slot is durably valid. Repair never renames, unlinks, or replaces `.codesplice.lock`, so participants cannot split across old and new lock objects; the Windows choreography remains compatible with the normal handle's deliberate omission of delete sharing. Phase 0 must freeze crash cases before, during, and after every slot write and prove that each yields either the old valid generation, the new valid generation, or explicit `WORKSPACE_LOCK_INVALID`, never two lock objects. Neither normal commit nor recovery silently invokes repair.

### Noncreating no-op transaction-health check

Before a commit may report successful no-op, it performs this normative, mutation-free check:

1. Inspect `.codesplice.lock` and `.codesplice` relative to the retained root handle without creating, initializing, repairing, truncating, or opening either for write.
2. If neither exists, return the frozen no-op success immediately.
3. If the control directory exists but the lock does not, return `WORKSPACE_LOCK_INVALID`; transaction health cannot be serialized safely.
4. If the lock exists, securely open and validate it and attempt the normal nonblocking exclusive lock. Contention returns `TRANSACTION_BUSY`, even when the content plan is unchanged.
5. While holding that existing lock, revalidate the root and lock namespace identities and boundedly inspect the existing control directory, if any, using the stale-transaction gate. Any unfinished, corrupt, partially published, unclassifiable, or not-fully-cleaned transaction returns `TRANSACTION_RECOVERY_REQUIRED`. A scan/control limit that prevents a clean determination also returns recovery required with its frozen reason.
6. Only a valid lock record and absent or provably transaction-clean control directory permit no-op success. Release the handle without writing either reserved artifact.

The no-op report still uses `transaction_id: null`, `transaction_state: "not_created"`, and `files_changed: []`; it additionally reports `transaction_health: "clean"`. This check never advances cleanup, because doing so would violate mutation-free no-op semantics.

## 7.6 Authoritative path equivalence

Every accepted operation, parent, target, reserved path, manifest path, and sort key has a filesystem-layer `PathEquivalenceKey`, whether or not the final entry exists. The key is an opaque, length-delimited sequence of:

```text
workspace identity
for each component:
  securely resolved parent-directory physical identity
  platform component-comparison key
```

The platform component key is frozen as follows for a filesystem that has passed the runtime support gate in Section 7.7:

* Linux: use exact UTF-8 bytes only when the Section 7.7 evidence hierarchy selects a matrix row whose Phase 0 mutation-based spike established case-sensitive, exact-byte component lookup without Unicode normalization. Do not infer these semantics from the Linux kernel, CPU architecture, or a read-only runtime observation alone.
* macOS: query the mounted volume’s case-sensitivity behavior; use the platform filesystem comparison form, including its canonical Unicode decomposition behavior, and apply case folding only on a case-insensitive volume. If the behavior cannot be queried or represented, reject with `PATH_EQUIVALENCE_UNSUPPORTED`.
* Windows: reject Win32 device names, alternate-data-stream syntax, trailing dot/space spellings, and unsupported reparse behavior. Preserve each accepted component's original UTF-16 code-unit sequence; do not apply NFC, NFD, or any other Unicode normalization. On a case-sensitive directory, the component key contains that exact sequence. On a case-insensitive directory, derive the key with the Phase 0-validated Windows ordinal case-insensitive mapping/comparison semantics. Canonically equivalent precomposed and decomposed sequences remain distinct unless the native directory lookup itself identifies the same entry.

The filesystem implementation must validate these rules against native same-entry probes in platform tests. It must not use locale-sensitive casing. Existing entries additionally require matching physical identity; equivalence keys do not replace physical-identity validation.

Use `PathEquivalenceKey` to:

* Reject duplicate source/destination path records that the platform interprets as one entry.
* Reject aliases of `.codesplice` and `.codesplice.lock`.
* Detect absent-target basename collisions under one physical parent.
* Group all operations targeting one new file.
* Sort input and output tables, manifest targets, and commit order; ties are errors, never spelling-based tie breaks.
* Bind every manifest parent/basename record to the same platform interpretation used during planning.

The core retains the normalized user-visible path spelling for diagnostics. No two distinct retained spellings may share one authoritative equivalence key in a valid batch, except repeated references to the same logical operation path with one identical mandatory precondition.

## 7.7 OS-plus-filesystem support gate

An operating-system architecture alone never establishes support. Phase 0 must publish a matrix keyed by OS version family, filesystem type/version where observable, local versus network/overlay/mounted-volume status, case behavior, normalization behavior, physical-identity stability, hard-link reporting, ACL/xattr/flag inspection, handle-relative open/rename support, no-replace semantics, and file/directory synchronization semantics.

Runtime support decisions use this formal evidence hierarchy:

1. Read-only identification records the OS build, filesystem and mount/volume identity, local/network/overlay/removable status, directory case-sensitivity settings, and exposed volume capabilities for the workspace root, control location, every source/target parent, and every rename pair.
2. Those observations are looked up in a frozen, versioned support matrix backed by the Phase 0 mutation-based feasibility spikes for that exact row. Matrix evidence, not a runtime read-only operation, establishes no-replace rename, staging-to-target rename, and file/directory durability semantics.
3. When the matrix explicitly permits it but read-only identification is insufficient, commit may run an optional mutation-based probe only after acquiring the workspace lock and before manifest creation, inside a newly created secured test directory whose identity, cleanup, byte/directory budget, and crash residue handling are frozen. Preview and other read-only commands never run this probe. Any residue or cleanup failure returns `TRANSACTION_RECOVERY_REQUIRED` and blocks transaction creation.
4. If the observations do not select one exact matrix row, or the permitted write probe cannot determine the result, reject conservatively with `FILESYSTEM_SEMANTICS_UNSUPPORTED` before manifest creation or target mutation.

The evidence must establish or conservatively reject:

* Exact component equivalence behavior for existing and absent names.
* Stable file and directory identity for the duration required by commit and fresh-process recovery.
* Handle-relative, no-follow resolution and no-replace rename for target-to-backup, candidate-to-target, and backup-to-target.
* Same-filesystem rename between each target parent and its transaction-private staging directory.
* Durable file sync and parent-directory sync when Durable mode is requested.
* Network, FUSE, overlay, virtualized, bind-mounted, removable, and nested-mounted volume behavior.

No runtime read-only probe may be described as proving no-replace rename, cross-directory rename, or crash durability. Failure or ambiguity returns `FILESYSTEM_SEMANTICS_UNSUPPORTED` before transaction creation or target mutation. Phase 0 feasibility spikes must exercise at least Linux ext4, XFS, Btrfs, and overlay/network rejection paths; macOS APFS in case-sensitive and case-insensitive configurations plus network rejection; and Windows NTFS plus reparse/network rejection. The published matrix may support fewer filesystems when a spike fails, but it may not silently generalize from one filesystem to an OS family.

---

# 8. Protocol and capability model

## 8.1 Protocol envelope

Initial protocol version:

```text
protocol_version = 1
```

Protocol version and supported capabilities are separate. Protocol v1 is nevertheless a permanently closed wire vocabulary: every enum value, operation tag, error code, warning code, and object field accepted by v1 is frozen before Phase 1. Additions require protocol v2 even when the semantic feature is optional or capability-gated. Capability reporting may describe which frozen v1 features this executable implements; it does not make the v1 schema open-ended.

The operations deferred to `v0.2.0` therefore require a protocol-v2 schema/envelope unless Phase 0 chooses an explicitly open, registry-governed string extension point and updates every v1 compatibility fixture before v1 is frozen. The default and preferred decision is protocol v2.

## 8.2 `v0.1.0` capabilities

```text
operations:
  move
  copy

selectors:
  lines
  bytes

anchors:
  file_start
  file_end
  before_line
  after_line
  byte_offset

preconditions:
  sha256
  must_not_exist
```

Unknown semantic capabilities fail explicitly.

## 8.3 Capabilities command

```bash
codesplice capabilities --json
```

Example:

```json
{
  "protocol_versions": [1],
  "operations": ["move", "copy"],
  "selectors": ["lines", "bytes"],
  "anchors": [
    "file_start",
    "file_end",
    "before_line",
    "after_line",
    "byte_offset"
  ],
  "preconditions": [
    "sha256",
    "must_not_exist"
  ],
  "plan_hash_versions": [1],
  "transaction_manifest_versions": [1],
  "transaction_state_versions": [1]
}
```

## 8.4 Transformation versus execution

```rust
pub struct BatchSpecification {
    pub protocol_version: u32,
    pub workspace_root: WorkspaceMarker,
    pub operations: Vec<Operation>,
}

pub struct ExecutionOptions {
    pub mode: ExecutionMode,
    pub durability: Durability,
    pub output: OutputMode,
    pub expected_plan: ExpectedPlanPolicy,
    pub metadata_loss: MetadataLossPolicy,
}
```

```rust
pub enum ExecutionMode {
    Preview,
    Commit,
}

pub enum Durability {
    Normal,
    Durable,
}

pub enum ExpectedPlanPolicy {
    Require(Digest),
    AcceptCurrentPlan,
}

pub enum MetadataLossPolicy {
    Reject,
    AllowWithWarnings,
}
```

The same `BatchSpecification` must be reusable for preview and commit.

## 8.5 Normative protocol-v1 and CLI surface

Phase 0 must publish validating JSON Schema 2020-12 documents in:

```text
docs/schema/v1/request.schema.json
docs/schema/v1/response.schema.json
docs/schema/v1/recovery-request.schema.json
docs/schema/v1/recovery-response.schema.json
docs/schema/v1/doctor-request.schema.json
docs/schema/v1/doctor-response.schema.json
docs/schema/v1/common.schema.json
```

Together they must close and type every request envelope, move/copy operation, selector, anchor, mandatory precondition, execution response, preview report, transaction report, recovery request/report, error, warning, and context object. Every object uses `additionalProperties: false`; duplicate JSON object keys are rejected by the streaming parser before DTO construction. Unknown enum values are rejected. Integers are JSON integers with schema bounds and must convert without narrowing loss.

Digest strings use exactly lowercase:

```text
sha256:<64 lowercase hexadecimal digits>
```

Uppercase hexadecimal, missing prefixes, whitespace, and other algorithms are invalid. JSON output emits one complete UTF-8 JSON value followed by `LF` on stdout. In `--json` mode, stdout contains no progress or prose; diagnostics intended for humans go to stderr and never replace the structured response.

`apply` is the canonical batch command:

```text
codesplice apply --request <file-or-> (--preview | --commit) [execution options]
```

Direct `move` and `copy` commands are convenience constructors for exactly one protocol-v1 operation and use the same DTO conversion, planner, report, expected-plan policy, and transaction implementation. They are not independent protocols. `inspect`, `capabilities`, `protocol-version`, `doctor`, and `recover` have dedicated closed request/response schemas where applicable.

Phase 0 must freeze an executable CLI grammar, not merely examples. It defines every positional argument, required and mutually exclusive option, stdin/stdout behavior, `--json` placement, default, and interaction for `apply`, `inspect`, direct `move`/`copy`, `recover`, `doctor`, `capabilities`, and `protocol-version`; the exact JSON request and response shape for each command; error JSON placement; no-op response fields (`transaction_id: null`, `transaction_state: "not_created"`, `transaction_health: "clean"`, `files_changed: []`); and recovery selection/confirmation semantics. Golden examples must cover every command, selector and anchor variant, existing and absent targets, preview, commit, no-op, every recovery state, each stable error category, and each warning shape. Missing CLI or schema semantics are a Phase 0 blocker and may not be deferred to Phase 2 documentation.

Execution options—including preview/commit, durability, output form, diff policy, expected-plan policy, and trusted metadata-loss policy—are excluded from `BatchSpecification` and from `plan_sha256`. Any option that changes resulting content bytes is therefore prohibited as an execution option; metadata-loss permission may change only whether an otherwise byte-identical commit is allowed and which frozen warnings are emitted.

## 8.6 Stable errors, warnings, and exits

Protocol v1 freezes these process exit categories:

```text
0  success, including a valid unchanged/no-op plan
2  invalid CLI or protocol request; non-retryable without changing input
3  precondition, identity, path, expected-plan, or external-modification conflict; retryable only after a fresh inspect/preview or external-state change
4  resource or supported-capability limit; retryability depends on trusted local configuration
5  transaction requires recovery or is busy; retryable/recoverable as identified by the response
6  corrupt transaction/control record; non-retryable without explicit operator repair
7  unsupported or unavailable platform primitive
8  internal failure
```

Every error includes stable `code`, `category`, `retryable`, `message`, and `context`. Request-validation errors additionally include an RFC 6901 JSON pointer and, when applicable, a zero-based `operation_index`. Filesystem errors include only normalized workspace-relative paths by default; absolute workspace paths, raw physical identities, OS usernames, and adjacent unrelated names are redacted unless a trusted `--diagnostic-paths` option is used. Warnings have stable identifiers and structured context. Because v1 uses closed schemas, adding an error or warning identifier, changing an identifier’s meaning or exit category, or changing a context field requires protocol v2. There is no "backward-compatible new v1 error code" exception.

Every human-readable renderer uses one centralized terminal-safe escaping contract for all untrusted or filesystem-derived strings, including paths, error messages/context, warnings, request values, transaction IDs/metadata, recovery observations, and diff labels/content. C0/C1 controls, ESC, bidi controls, and noncharacters receive visible escaped forms; tabs/newlines are allowed only where the renderer itself inserts or explicitly structures them. JSON mode relies on canonical JSON escaping and never interpolates pre-rendered terminal text.

Phase 0 must add exhaustive error and warning registry tables to `docs/protocol.md`, assigning every code to one exit category and defining every context field. No production code may emit an unregistered identifier. The registry must at least cover all codes named in this plan, parser/schema failures, selector/anchor/conflict variants, snapshot and path failures, lock/control failures, transaction/recovery classifications, diff truncation, platform limitations, and internal failure. Golden CLI tests bind each registered error to its exit code and stdout/stderr behavior.

---

# 9. Plan digest specification

## 9.1 Purpose

`plan_sha256` identifies one fully resolved plan in one particular workspace snapshot.

It is used to detect:

* A changed request file.
* Changed CLI semantics.
* Changed selectors or anchors.
* Changed input bytes.
* Changed workspace identity.
* Changed file identity.
* Changed normalization behavior.
* A different resolved operation ordering.

It is not intended as a portable repository-content identifier.

Documentation must state:

> `plan_sha256` identifies a resolved plan within a particular workspace snapshot. It is not necessarily reproducible after copying the workspace, recreating its files, or changing physical file identities.

## 9.2 Commit precondition

Support:

```bash
codesplice apply \
  --request split.json \
  --commit \
  --expect-plan sha256:PREVIEWED_PLAN
```

Before creating `.codesplice`, a transaction record, or any candidate:

1. Establish the workspace.
2. Read a fresh snapshot.
3. Recompute the complete plan.
4. Recompute `plan_sha256`.
5. Compare it with `--expect-plan`.
6. Reject on mismatch.

Error:

```text
EXPECTED_PLAN_MISMATCH
```

with:

```text
expected_plan_sha256
actual_plan_sha256
```

For commit commands, require exactly one of:

```text
--expect-plan sha256:...
--accept-current-plan
```

`--expect-plan` is the recommended agent workflow.

`--accept-current-plan` is an explicit human convenience mode that commits the currently resolved plan without a prior preview hash.

## 9.3 Canonical plan encoding

Plan-hash version 1 uses the following complete binary grammar. Concatenation is written `||`; no alignment or implicit padding is inserted.

```text
u8(x)      = one unsigned byte
u16(x)     = unsigned 16-bit big-endian
u32(x)     = unsigned 32-bit big-endian
u64(x)     = unsigned 64-bit big-endian
blob(b)    = u32(byte_length(b)) || b
utf8(s)    = blob(exact UTF-8 bytes of s)
digest(d)  = exactly 32 raw digest bytes
none       = u8(0)
some(x)    = u8(1) || encode(x)
list(xs)   = u32(item_count) || encode(each item in order)
```

Before converting a byte length or item count to `u32`, reject values greater than `u32::MAX` with `PLAN_ENCODING_LIMIT`; wrapping, truncation, native-width integers, varints, and host byte order are forbidden. Every operation index is encoded as `u32`, never `usize`; protocol limits guarantee conversion succeeds.

Normalized paths are UTF-8 workspace-relative strings with `/` separators, no empty, `.` or `..` components, and no trailing separator. Case is retained. A path-equivalence key is encoded independently:

```text
path_key = identity(workspace) || list(path_key_component)
path_key_component = identity(parent_directory) || blob(platform_component_key)
path = utf8(normalized_visible_path) || blob(complete_path_key_encoding)
```

Identity encoding and the v1 identity-kind registry are:

```text
identity = u16(identity_kind) || blob(canonical_identity_bytes)

0x0000 synthetic test identity; forbidden for live filesystem plans
0x0001 Linux file/directory identity v1
0x0002 macOS file/directory identity v1
0x0003 Windows file/directory identity v1
```

The canonical bytes for each live kind are fixed in `docs/adr/0004-path-identity-policy.md`. Existing tag meanings never change. A new representation receives a new tag even on the same OS.

`NewPlannedFileId` is not platform identity. It is exactly:

```text
SHA256(
  ASCII "CODESPLICE-NEW-ID\0"
  || blob(complete destination PathEquivalenceKey encoding)
)
```

The top-level record is exactly:

```text
plan_v1 =
  ASCII "CODESPLICE-PLAN\0"
  || u32(1)                         # plan-hash version
  || u32(protocol_version)
  || identity(workspace_identity)
  || list(input_record)
  || list(resolved_operation)
  || list(output_record)

plan_sha256 = SHA256(plan_v1)
```

Input records contain every distinct logical source or destination path mentioned by the batch. Sort by encoded `PathEquivalenceKey` bytes, then reject rather than tie-break if two records have the same key.

```text
input_record =
  path
  || input_state

input_state =
  u8(0)                             # absent
  | u8(1) || identity(file) || u64(length) || digest(content_sha256)
```

Operations remain in request order. Duplicate operations are retained and distinguished by index. The record includes both syntax-level semantics and resolution so changes in selector or anchor spelling cannot collide merely because they currently resolve to the same offset.

```text
resolved_operation =
  u32(operation_index)
  || operation_kind
  || path(source_path)
  || identity(source_file)
  || selector
  || precondition(source_precondition)
  || u64(resolved_source_start)
  || u64(resolved_source_end)
  || digest(selected_payload_sha256)
  || path(destination_path)
  || planned_file_id(destination_file)
  || anchor
  || precondition(destination_precondition)
  || u64(resolved_destination_offset)
  || operation_effect

operation_kind = u8(1) move | u8(2) copy

selector =
  u8(1) || u64(first_line_inclusive) || u64(last_line_inclusive)
  | u8(2) || u64(start_byte) || u64(end_byte_exclusive)

anchor =
  u8(1)                              # file_start
  | u8(2)                            # file_end
  | u8(3) || u64(line_number)        # before_line
  | u8(4) || u64(line_number)        # after_line
  | u8(5) || u64(byte_offset)

precondition =
  u8(1) || digest(expected_sha256)
  | u8(2)                            # must_not_exist

planned_file_id =
  u8(1) || identity(existing_file)
  | u8(2) || digest(NewPlannedFileId)

operation_effect = u8(1) changed | u8(2) no_op
```

Every operation path has an explicit precondition. Therefore there is no absent/optional precondition tag. Source preconditions and existing destination preconditions use tag `1`; absent destinations use tag `2`.

Output records are sorted by encoded `PathEquivalenceKey` bytes, again rejecting ties. The set is exact: include every file that has at least one deletion or insertion edit event, plus every file containing at least one same-file no-op move report event. Exclude an otherwise unchanged file used only as a copy payload source. There is exactly one output record per included `PathEquivalenceKey`; no other unchanged/source-only file is encoded.

```text
output_record =
  path
  || planned_file_id
  || optional_original_digest
  || digest(resulting_sha256)
  || u64(resulting_length)
  || change_kind
  || list(output_segment)

optional_original_digest =
  u8(0)
  | u8(1) || digest(original_sha256)

change_kind =
  u8(1) unchanged
  | u8(2) modified_existing
  | u8(3) created_new
  | u8(4) emptied_existing

output_segment =
  u8(1) || identity(source_file) || u64(start) || u64(end_exclusive)
  | u8(2) || u32(operation_index) || identity(source_file)
          || u64(start) || u64(end_exclusive) || digest(payload_sha256)
```

Segment tag `1` is an original surviving slice; tag `2` is an insertion sourced by the named operation. Segments are encoded in final byte-emission order. Empty outputs have an empty segment list.

The following are excluded from `plan_v1`: execution options (`preview`, `commit`, durability, output/diff mode, expected-plan policy, metadata-loss policy), warnings, human messages, executable version, timestamps, temporary or transaction paths, transaction ID, permission/security metadata, and preview truncation. No nested digest may reuse the plan domain: payload and file digests are ordinary SHA-256 of content bytes, while new-file IDs use the explicit `CODESPLICE-NEW-ID\0` domain above.

Phase 0 golden vectors must include at least absent and existing destinations, every enum variant, a no-op, multiple same-offset insertions, non-UTF-8 payload bytes, and maximum-width numeric examples. Each vector includes the semantic input, annotated complete hexadecimal byte dump, byte length, and expected lowercase `sha256:` digest. Golden vectors use synthetic identity kind `0x0000`, never live inodes or Windows file indexes.

---

# 10. Resource limits

Freeze `v0.1.0` defaults before implementation.

Initial defaults:

```text
maximum JSON request size:
  16 MiB

maximum JSON nesting depth:
  64

maximum serialized JSON response size:
  64 MiB

maximum operations per batch:
  10,000

maximum distinct operation paths:
  5,000

maximum UTF-8 relative-path length:
  4,096 bytes

maximum encoded PathEquivalenceKey length:
  64 KiB

maximum canonical physical-identity token length:
  512 bytes

maximum metadata entries inspected per input file:
  4,096

maximum metadata bytes inspected per input file:
  1 MiB

maximum total metadata-inspection bytes:
  32 MiB

maximum transaction targets:
  5,000

maximum manifest size:
  64 MiB

maximum state-file size:
  16 MiB

maximum state records per transaction:
  50,000

maximum cumulative state-record bytes per transaction:
  512 MiB

maximum transaction-directory entries scanned:
  100,000

maximum transaction directories in the control directory:
  10,000

maximum total recovery bytes read in one command:
  1 GiB

maximum total control-directory bytes:
  8 GiB

maximum human-readable diff:
  8 MiB

maximum JSON-embedded diff:
  8 MiB

maximum individual snapshot file:
  1 GiB

maximum total in-memory snapshot bytes:
  2 GiB

maximum resulting bytes per changed file:
  1 GiB

maximum total planned-output bytes:
  4 GiB

maximum total candidate bytes:
  4 GiB

maximum segments per output:
  100,000

maximum total output segments:
  1,000,000

maximum line records per file:
  10,000,000

maximum total line records:
  20,000,000

maximum line-index memory:
  256 MiB

maximum total planning memory:
  3 GiB

maximum projected transaction disk use:
  8 GiB
```

Limit ownership follows when the bounded value first exists:

* Phase 2 validates request bytes, JSON nesting, textual UTF-8 path lengths, operation/path request counts, and response projections derivable from the request/schema before large allocation.
* Phase 3 validates filesystem-generated `PathEquivalenceKey` encodings, physical-identity tokens, metadata entry/byte counts, snapshot bytes, line indexes, and aggregate snapshot/planning acquisition budgets.
* Phase 4 and later validate segment counts, resulting-output amplification, candidate bytes, transaction/control projections, state/recovery records, disk use, and final response projections as those values are constructed.

An earlier phase must not claim to validate a generated value that does not yet exist. Every handoff carries already-charged counters so later phases cannot double-count or leave allocations uncharged.

Requirements:

* Limits apply before uncontrolled allocation.
* Every parsed count, file length, line count, segment count, output length, candidate length, backup length, record length, response length, memory charge, and disk-use sum uses checked arithmetic in its canonical unsigned width and checked conversion to `usize`. Overflow is `RESOURCE_LIMIT_EXCEEDED`, never wraparound or saturation.
* Planning charges source snapshot bytes, compact line indexes, equivalence/identity tokens, bounded metadata names/values, segment vectors, response estimates, planned candidates, backups, manifest/state worst-case overhead, and recovery/control scans to separate counters and to the applicable aggregate budget. Metadata enumeration that exceeds its count/byte bound fails closed rather than assuming no security metadata.
* The planner computes each resulting length from segments before transaction creation, rejects per-file or aggregate output amplification, and computes projected transaction disk use as candidates + retained backups + worst-case bounded control records. A 1 GiB source copied to 10,000 outputs must be rejected during pure planning without creating `.codesplice.lock`, `.codesplice`, or transaction artifacts.
* Candidate preparation independently accounts actual bytes written and aborts safely if observations exceed the already accepted plan; it may never use the candidate budget to authorize a larger plan.
* Line indexes use a compact offset representation with terminator kind packed or stored separately. A heap-sized `LineRecord` object per line is forbidden. Every index allocation is charged before allocation to the per-file line count, total line count, line-index byte budget, and total planning-memory budget.
* Limit violations use structured errors.
* Request-provided data cannot increase the limits.
* Trusted CLI configuration may lower limits.
* Increasing limits requires an explicit trusted local option and documented risk.
* Preview truncation never changes plan or output hashes.
* Truncation is explicitly reported.

## 10.1 Diff policy for arbitrary bytes

Diff generation is reporting-only and may be disabled with `--no-diff`; disabling or truncating a diff cannot affect planning, hashing, or commit. A file is treated as binary when either side contains NUL or is not valid UTF-8. Binary reports contain lengths, SHA-256 digests, and bounded changed-span samples encoded as standard padded base64; they never place arbitrary bytes directly in JSON or terminal output.

Text diffs tokenize LF, CRLF, and lone CR as indivisible terminators using the same line index as selectors. JSON escaping is the only string escaping in JSON mode. Human mode visibly escapes every terminal control originating from paths, errors, warnings, transaction metadata, request strings, and diff content. Recognized line terminators may structure diff lines only after all non-terminator controls are escaped. ANSI/ECMA-48 escape characters are never emitted from untrusted text.

The implementation must enforce all of these budgets while computing, not only while rendering:

```text
maximum bytes admitted to detailed diff algorithm per side: 8 MiB
maximum lines admitted per side: 200,000
maximum edit-graph/work units: 10,000,000
maximum rendered bytes: selected configured diff limit
maximum binary sample bytes across one report: 64 KiB
```

If any budget is reached, stop detailed computation and emit stable warning `DIFF_TRUNCATED` with `reason`, emitted byte count, and omitted-file/hunk counts when known. Use a bounded-memory algorithm with explicit work accounting; an unbounded quadratic LCS matrix is forbidden. For mixed terminators, reports preserve and label terminator kinds. Diff allocation must remain `O(admitted input bytes + configured output limit)`.

---

# Phase 0 — Freeze complete `v0.1.0` semantics

## Objective

Resolve every foundational ambiguity before production implementation begins.

## Required documents

Create:

```text
docs/specification.md
docs/protocol.md
docs/cli.md
docs/metadata.md
docs/security.md
docs/resource-limits.md
docs/transaction-model.md
docs/platform-support.md
docs/protocol-v1-compatibility.md
docs/schema/v1/request.schema.json
docs/schema/v1/response.schema.json
docs/schema/v1/recovery-request.schema.json
docs/schema/v1/recovery-response.schema.json
docs/schema/v1/doctor-request.schema.json
docs/schema/v1/doctor-response.schema.json
docs/schema/v1/common.schema.json
docs/adr/0001-exact-content-model.md
docs/adr/0002-coordinate-semantics.md
docs/adr/0003-workspace-root-model.md
docs/adr/0004-path-identity-policy.md
docs/adr/0005-conflict-semantics.md
docs/adr/0006-new-file-policy.md
docs/adr/0007-plan-hash-format.md
docs/adr/0008-transaction-guarantees.md
docs/adr/0009-permission-preservation.md
docs/adr/0010-workspace-lock-bootstrap.md
docs/adr/0011-path-equivalence-keys.md
docs/adr/0012-transaction-state-and-durability.md
docs/adr/0013-candidate-ownership.md
docs/adr/0014-protocol-evolution.md
docs/performance-methodology.json
```

## 0.1 Line semantics

User-facing line numbers are 1-based.

A line selector such as:

```text
120:238
```

includes lines 120 through 238.

A whole-line selection includes its original terminator.

Recognized terminators:

```text
LF
CRLF
lone CR
```

Rules:

* Terminators are preserved exactly.
* An empty file has zero selectable lines.
* A nonempty unterminated final line is selectable.
* A file ending in a terminator does not expose a phantom extra line.
* Mixed line endings in one file are valid.

## 0.2 Byte selectors

Byte ranges are:

* Zero-based.
* Half-open.
* Expressed as `[start, end)`.

For `v0.1.0`:

```text
start < end
end <= file length
```

Empty byte selections are rejected.

## 0.3 Destination anchors

Supported:

```text
file_start
file_end
before_line
after_line
byte_offset
```

Valid byte offsets satisfy:

```text
0 <= offset <= initial file length
```

Do not expose `before_byte` or `after_byte`.

Exact line-anchor semantics are:

* `before_line(n)` resolves to the first byte of selectable line `n`.
* `after_line(n)` resolves immediately after that line’s complete terminator (`LF`, `CRLF`, or lone `CR`). For an unterminated final line, it resolves to file length.
* Valid `n` is `1 <= n <= line_count`; an empty file therefore accepts no line anchor.
* `after_line(last_line)` always equals `file_end`, whether or not the final line is terminated.
* A line anchor never resolves inside a CRLF pair. Byte anchors may split CRLF because they operate on arbitrary bytes.
* Invalid line numbers return `LINE_ANCHOR_OUT_OF_RANGE` with the line count and operation index.

## 0.4 Same-file moves

For source `[start, end)`:

```text
destination < start:
  valid backward move

destination > end:
  valid forward move

destination == start:
  valid no-op

destination == end:
  valid no-op

start < destination < end:
  invalid
```

A same-file no-op move:

* Is retained in the resolved operation report.
* Reports `operation_effect = no_op`.
* Produces `change_kind = unchanged` when no other operation changes the file.
* Creates no transaction when the entire plan is unchanged.

## 0.5 Deterministic composition and cross-operation conflicts

All sources and anchors resolve before composition.

Rules:

* Effectful move source ranges may not overlap. A same-file no-op move creates no deleted range and does not participate in deleted-range overlap checks.
* Copy ranges may overlap other copy ranges.
* Copy ranges may overlap moved source ranges.
* Multiple insertions at one offset retain ascending operation-index order, which is request order.
* An insertion at the start boundary of a deleted range is valid.
* An insertion at the end boundary of a deleted range is valid.
* An insertion strictly inside any deleted range is invalid.
* Conflicting preconditions for one physical file are invalid.
* Distinct paths resolving to one physical file or one `PathEquivalenceKey` are invalid as separate logical paths.

Composition is a deterministic event stream over each file’s immutable initial bytes. An effectful move contributes one deletion interval at its source and one insertion at its destination. A copy contributes only an insertion. A no-op move contributes a report event but neither deletion nor insertion. All inserted payloads refer to the initial snapshot even if another operation deletes the same source bytes.

For each file, the complete event enum is:

```text
deletion_end(operation_index)
insertion(operation_index)
deletion_start(operation_index)
end_of_file
```

The complete sort key is `(initial_byte_offset, event_class, operation_index)`. Event-class discriminants are the order below. `end_of_file` uses the reserved sentinel operation index `u32::MAX`; real operation indices are strictly smaller because of the frozen request limit. There is no `boundary_kind` field.

The total event-class order at one offset is:

```text
1. deletion_end, ascending source operation index
2. insertion, ascending operation index
3. deletion_start, ascending source operation index
4. end_of_file
```

Original-slice emission is defined by this sweep, which is the normative output-construction algorithm:

1. Sort and validate deletion intervals; adjacent intervals remain distinct, overlaps are rejected.
2. Set `cursor = 0` and `deletion_active = false`.
3. At each event offset, emit initial bytes `[cursor, offset)` only when `deletion_active` is false, then set `cursor = offset`.
4. Process all `deletion_end` events, setting `deletion_active = false`.
5. Emit all insertion payload slices in ascending operation-index order. Insertion does not advance `cursor`.
6. Process all `deletion_start` events, setting `deletion_active = true`.
7. Treat `file_length` as the synthetic EOF offset and run steps 3–5 there. Step 3 emits any final surviving original slice before EOF insertions; after those insertions, terminate. No deletion start is valid at EOF.

Because effectful move deletions do not overlap, `deletion_active` is boolean. At the shared boundary of adjacent deletions, the end is processed, insertions at that offset are emitted, then the next deletion begins. This makes boundary insertion legal and deterministic.

A forward same-file move needs no separate mutable-coordinate rule: its insertion is attached to the initial destination offset and naturally appears at final offset `destination - total_deleted_bytes_strictly_before_destination`. A backward move is analogous. A whole-file effectful move deletes `[0, file_length)`; absent other insertions, the existing source remains present with zero content bytes. New files run the same sweep over a zero-length initial snapshot, so all legal insertions occur at offset `0` in operation order.

Phase 0 must include annotated byte examples for: start/end insertion on one deletion, two insertions at each boundary, adjacent deletions with an insertion between them, forward/backward moves, a no-op plus a boundary insertion, line and byte anchors resolving to one offset, copy sourced from bytes another move removes, EOF insertion, and whole-file move.

## 0.6 Workspace and path rules

Freeze all workspace-root and `.codesplice` rules from Sections 7 and 8.

Operation paths:

* Are workspace-relative.
* Cannot be absolute.
* Cannot contain `..`.
* Cannot target `.codesplice`, `.codesplice.lock`, or any platform-equivalent spelling.
* Cannot traverse symlinks, junctions, or unsupported reparse points.
* Cannot refer to directories or special files.
* Cannot escape the root.
* Cannot rely on parent-directory creation.
* Must receive and retain an authoritative `PathEquivalenceKey` under Section 7.6, including when the final entry is absent.

## 0.7 Physical identity

Physical identity support is a release requirement on every claimed OS-plus-filesystem matrix row for:

```text
Linux x86_64
macOS ARM64
Windows x86_64
```

It is not an optional capability on a supported matrix row. A release may reject an unproven filesystem on those operating systems, but may not silently downgrade identity validation.

Reject distinct requested paths identifying the same existing physical file.

Return:

```text
FILE_IDENTITY_ALIAS
```

## 0.8 Preconditions

Every operation path requires an explicit precondition. Omission is invalid, including when commit uses `--accept-current-plan`.

Existing source or destination:

```json
{
  "kind": "sha256",
  "value": "sha256:..."
}
```

New destination:

```json
{
  "kind": "must_not_exist"
}
```

A source cannot use `must_not_exist`.

Every reference to one logical path in a batch must repeat an identical precondition. Conflicting or lexically different digest values after canonical parsing are rejected before snapshot acquisition.

## 0.9 New files

A nonexistent destination has an initial empty snapshot.

Legal anchors:

```text
file_start
file_end
byte_offset 0
```

Multiple operations targeting the same new file insert in request order.

Parent directories must already exist.

## 0.10 Permission concurrency rule

For an existing target:

1. Move the actual target to backup with no-replace semantics.
2. Validate the moved object’s identity and digest.
3. Reinspect its current link/security metadata and enforce the frozen metadata-loss policy.
4. Read its current ordinary permission bits.
5. Apply those current bits and the frozen final metadata policy to the candidate.
6. Install the candidate.

Do not restore earlier snapshot permissions over a newer external permission change.

## 0.11 Recovery locking

Rules:

```text
recover --rollback:
  exclusive mutation lock required

recover --complete:
  exclusive mutation lock required

recover --list:
  may run without the exclusive lock

recover --status:
  may run without the exclusive lock
```

Read-only recovery output must include:

```text
observation_may_be_stale: true
```

when no shared or exclusive lock is held.

State and manifest files must be read through atomic, checksummed records.

If state changes during an unlocked status read:

1. Retry once.
2. If it changes again, return `TRANSACTION_BUSY`.

## 0.12 Stable snapshot acquisition

Each existing physical file is acquired from one open handle:

1. Open relative to a securely opened parent directory with read-only, close-on-exec, and no-follow/no-reparse semantics.
2. From the handle, capture physical identity, regular-file type, length, ordinary permissions where supported, and the strongest portable change indicator available on the platform (including high-resolution modification/change time when available).
3. Reject the file before allocation if its declared length exceeds the per-file or remaining batch limit.
4. Read to EOF in bounded chunks while hashing, rejecting more bytes than the limit or more bytes than the captured length permits.
5. From the same handle, capture identity, type, length, and change indicators again.
6. Treat different before/after metadata or short/long reads relative to the final length as an unstable attempt. Retry only when before/after observations demonstrate instability.
7. Re-resolve the parent entry without following and verify it still identifies the opened object and the same parent identity.
8. After a stable read, compare SHA-256 with the explicit digest precondition. A mismatch is the deterministic conflict `DIGEST_PRECONDITION_FAILED` and is returned immediately without consuming another snapshot retry.
9. Close only after all captured commit-context tokens are finalized.

Retry the complete open/read/validate sequence at most two additional times, for three attempts total. A third unstable result returns `SNAPSHOT_UNSTABLE` with the normalized path, attempt count, and which indicators changed. Never combine chunks from separate attempts. This detects ordinary concurrent in-place writes; the security documentation must state that platforms lacking a generation counter cannot prove absence of a hostile write that restores all observed metadata.

## 0.13 Lock and secure-handle contract

Freeze and test the exact Section 7.5 bootstrap. All `.codesplice`, transaction-directory, candidate, backup, target, rollback, and cleanup operations use handle-relative APIs where supported. Every path-based exception for a non-rename operation must identify its OS primitive, pre/post identity validation, and residual race in `docs/platform-support.md`. Target-to-backup, candidate-to-target, and backup-to-target restoration have no path-based or replace-capable exception: absence of the secure handle-relative no-replace primitive rejects commit support for that filesystem before target mutation. `WORKSPACE_LOCK_INVALID`, `TRANSACTION_BUSY`, and `PATH_EQUIVALENCE_UNSUPPORTED` are protocol-v1 stable errors.

## 0.14 Transaction records, states, and durability

The immutable manifest is published before any candidate creation. The mutable logical state is represented by a hash-chained sequence of atomic **delta events with bounded periodic full snapshots**, not by rewriting a full per-target vector for every candidate or target transition. Every record contains transaction ID, manifest SHA-256, `sequence`, record kind (`snapshot` or `delta`), state tag and payload, and the prior published state record’s stored checksum (`none` for sequence `0`). The checksum is SHA-256 over every record byte from magic through payload, excluding only the final checksum field.

```text
manifest magic: CODESPLICE-MANIFEST\0
state magic:    CODESPLICE-STATE\0
format version: u32 big-endian 1
first state sequence: 0
next sequence: exactly previous + 1, without wrap
```

`manifest SHA-256` means the manifest record’s stored checksum under the same rule. `sequence`, target indices, record kinds, event/state tags, optional tags, identity encodings, lengths, and list counts receive fixed big-endian widths and numeric discriminants in the Phase 0 byte grammar in `docs/transaction-model.md`; the grammar must include an annotated golden byte dump for every snapshot and delta kind before Phase 1.

Sequence `0` is one full `Preparing` snapshot. It authorizes every candidate slot before creation as `authorized_missing` and binds its exact derived basename, expected transaction-private parent identity, digest, and length. Subsequent candidate creation and commit progress use one-target delta events. A full snapshot is published after at most 512 deltas and at each phase boundary (`Prepared`, `Committed`, `RolledBack`, and cleanup entry); recovery may start from the newest valid snapshot whose preceding chain has been validated, then fold later deltas. Record count, individual bytes, cumulative bytes, scan entries, and total control-directory use are limited by Section 10 and a worst-case state/control projection must pass before transaction creation.

State tags and payloads are:

```text
Preparing
  initial snapshot: per-target authorized_missing(exact derived name and parent identity)
  deltas: candidate_created(target index, candidate physical identity)
Prepared
  full snapshot: every candidate identity present and content verified
Committing
  deltas: commit_started | backed_up(target index, backup identity) |
          installed(target index, final identity)
Committed
  full snapshot: every final target verified; this persisted record is the logical commit point
RollingBack
  deltas: rollback_started and per-target rollback stage/observed identities
RolledBack
  full snapshot: every existing original restored and every originally absent target absent
CleaningCommitted
  entry snapshot plus cleanup deltas; only all-new recovery is legal
CleaningRolledBack
  entry snapshot plus cleanup deltas; only all-old recovery is legal
```

Legal transitions are:

```text
Preparing -> Preparing | Prepared | RollingBack
Prepared -> Committing | RollingBack
Committing -> Committing | Committed | RollingBack
RollingBack -> RollingBack | RolledBack
Committed -> CleaningCommitted
CleaningCommitted -> CleaningCommitted | remove transaction directory
RolledBack -> CleaningRolledBack
CleaningRolledBack -> CleaningRolledBack | remove transaction directory
```

The mutation/progress order is normative, not an implementation summary:

1. After all candidates and inputs pass final revalidation, publish `commit_started` and enter `Committing` **before** the first target mutation.
2. For an existing target, rename target to backup with handle-relative no-replace semantics, perform required durability ordering, verify the backup's recorded original identity/digest/type, then publish `backed_up(target index, backup identity)`.
3. Derive, apply, and verify the target's final metadata on the candidate. For an existing target, use the validated backup observation; for a new target, use Section 0.16.
4. Rename candidate to target with handle-relative no-replace semantics, perform required durability ordering, verify final target identity/digest/type/metadata, then publish `installed(target index, final identity)`.
5. Repeat steps 2–4 in deterministic commit-index order. An originally absent target has no `backed_up` event and proceeds from metadata verification to install.
6. Only after every target has a published `installed` event and a complete revalidation may the full `Committed` snapshot be published.

A state event never anticipates its filesystem mutation: `backed_up` follows backup rename and verification; `installed` follows install rename and verification. Conversely, a crash after a filesystem mutation but before its event is classified only by the Section 0.15 identity matrix.

Any error after the first target mutation—including metadata-policy refusal, verification failure, no-replace collision, progress-record publication failure, or a later-target conflict—must attempt transaction-wide rollback, not merely restore the current target. If the state chain remains publishable, first publish `rollback_started` and enter `RollingBack`; classify **all** targets, then remove installed candidates and restore all recorded originals in reverse commit order while publishing per-target rollback progress. If the triggering failure prevents a trustworthy `RollingBack` publication, or rollback cannot reach and publish `RolledBack`, return `TRANSACTION_RECOVERY_REQUIRED` with both the triggering error and rollback status. Do not return the original conflict alone. Before the first target mutation, failures may return their ordinary error without a rollback transition.

No event may regress a target stage or sequence, apply twice where not idempotent in the logical fold, or exceed the bounded snapshot interval. Rollback from `Committing` is allowed only after filesystem classification proves all installed candidates can be removed and all backups still identify the recorded originals. `Committed` is the irrevocable logical commit point: rollback is rejected thereafter, and recovery may only verify/complete cleanup. State that names an unknown target, invalid index, impossible stage, candidate identity absent from `Prepared`, an invalid snapshot fold, or a manifest digest/transaction ID mismatch returns `TRANSACTION_RECORD_CORRUPT`. Filesystem observations never silently repair a corrupt record; valid lag after a crash is handled by the recovery matrix.

Record publication protocol:

1. Exclusively create a bounded single-component temporary name inside the transaction directory: `manifest-<txn>.tmp` or `state-<20-digit-sequence>-<txn>.tmp`.
2. Serialize the complete bounded record in canonical binary form, append its checksum, write all bytes, and flush language/runtime buffers.
3. Apply the selected durability sync requirement below.
4. Publish with a handle-relative no-replace rename to `manifest.rec` or `state-<20-digit-sequence>.rec`.
5. Apply the selected directory-sync requirement.
6. Never overwrite a published record. On recovery, ignore only a validly bounded `.tmp` name owned by that manifest; never infer state from it.

The current state is the fold through the highest valid contiguous sequence whose hash chain begins at `0`. A gap, duplicate sequence, checksum failure in a published record, forked prior hash, delta that cannot apply to the preceding logical state, or overdue snapshot is corruption. This append-and-publish protocol is the v1 state-file replacement model; no in-place state write or separate checksum file is allowed.

Durability guarantees are:

| Operation | `Normal` | `Durable` |
|---|---|---|
| Record/candidate data write | `write_all`, flush runtime buffers, close before dependent rename | `write_all`, flush, `sync_all` file before dependent rename |
| Publish manifest/state | atomic no-replace rename; no directory sync required | sync temp file, rename, then sync transaction directory |
| Create control/transaction directory | ordinary secure create | secure create, then sync its parent directory at every new level |
| Exclusively create candidate | capture identity and retain the handle; publish no existence state yet | create and capture identity; publish no existence state yet |
| Candidate ready | flush and verify through the retained handle, publish the identity delta, then close before `Prepared` | apply restrictive permissions, write/verify, `sync_all` candidate file, sync candidate parent directory, publish the `candidate_created` identity delta, then close/reopen-verify before publishing/syncing `Prepared` |
| Target-to-backup rename | publish next state after rename | rename, sync both target parent and transaction directory, then publish and sync next state |
| Candidate-to-target rename | verify then publish next state | pre-sync candidate data/metadata, rename, sync both transaction directory and target parent, verify, then publish/sync state |
| Backup-to-target restoration | verify then publish rollback progress | rename no-replace, sync both transaction directory and target parent, verify, then publish/sync rollback state |
| Cleanup unlink/transaction removal | best effort with state retained until completion | sync affected parent after each cleanup batch and after transaction removal |

`Normal` guarantees recoverability after process termination provided the operating system and filesystem preserve acknowledged cached writes and renames; it makes no power-loss or kernel-crash persistence guarantee. `Durable` orders file and directory synchronization to survive a system crash or power loss under the documented filesystem’s `fsync`/equivalent contract. Unsupported directory sync or rename semantics fail before mutation with `UNSUPPORTED_DURABILITY_PRIMITIVE`. Neither mode makes multi-target visibility atomic.

## 0.15 Candidate identity and recovery comparison

Candidates and backups are created only inside the transaction-private directory, not adjacent to user targets. Before the manifest is published, the implementation captures that directory’s physical identity, verifies it is a newly and exclusively created real directory, and uses Section 7.7's read-only identity plus matrix evidence (and only a matrix-authorized secured write probe) to establish the same supported filesystem/rename relationship to every target parent. On Unix the directory must be owned by the effective user and have exact mode `0700` with no special bits; on Windows it must have the Phase 0-frozen owner-only/service-required DACL, reject inheritance that grants another writable principal, and reject every reparse tag. Transactions spanning a nested mount or any target for which staging-to-target and target-to-staging no-replace rename cannot be established fail before manifest publication.

Candidate ownership does not rely on an unobservable nonce. It derives from the secured, recorded transaction-directory identity; the exact authorized basename derived from the complete transaction ID and encoded target `PathEquivalenceKey`; sequence `0` proving that name was `authorized_missing`; and exclusive creation through the retained directory handle. No normalized path spelling is a hash input. After exclusive candidate creation, capture its canonical physical identity from the retained open handle but publish no existence state yet. Prepare and verify the file through that handle. In Durable mode, `sync_all` the candidate file and then sync the transaction directory; only afterward may the `candidate_created` delta publish the identity. Persist the identity before closing the last retained secure handle; `Prepared` is forbidden until every candidate identity is recorded and revalidated. Reopen/revalidate the directory entry against that identity immediately before installation. Recovery normally uses the recorded identity, never digest and length alone.

The exact crash after exclusive creation but before `candidate_created` publication is classified as `owned_unpublished_candidate`, not as a normal candidate. That classification is valid only when all of these hold: the last folded state is `authorized_missing`; the transaction-directory identity, ownership, mode/DACL, and no-reparse properties match the manifest; the entry has the exact authorized basename in that recorded directory; it is a regular, single-link object; and no containing-directory or state corruption exists. Its content, length, and digest are irrelevant to ownership. Fresh-process completion is forbidden because no recorded physical identity authorizes installation, but rollback may unlink this transaction-owned untrusted entry with handle-relative identity-before/after checks. If any ownership condition is missing, recovery returns explicit conflict and mutates nothing. This rule is part of the documented ordinary-race/cooperating-process threat model and does not claim protection from a hostile principal able to write inside the owner-only transaction directory.

For every target, classification compares:

```text
target path vs recorded original identity and digest
target path vs recorded candidate/final identity and planned digest
backup path vs recorded original identity and digest
candidate path vs recorded candidate identity and planned digest
each containing parent vs recorded parent identity and PathEquivalenceKey
an unpublished candidate only vs the complete `owned_unpublished_candidate` ownership predicate
```

Identical bytes with a different physical identity classify as `identity_mismatch`, except that a successfully installed candidate’s identity becomes the recorded final identity in the post-install state. A crash after rename but before that state update is recognized only when the target has the previously recorded candidate identity. Secure handles may be retained across installation when the platform permits; recovery must still work from persisted identities alone in a fresh process.

The minimum recovery-action matrix is normative:

| Original state | Target observation | Backup observation | Candidate observation | Safe action before `Committed` |
|---|---|---|---|---|
| present | recorded original | missing | recorded candidate | complete may back up then install; rollback removes candidate only |
| present | missing | recorded original | recorded candidate | complete installs candidate; rollback restores original |
| present | recorded candidate identity | recorded original | missing | complete records/verifies install; rollback removes target then restores original |
| absent | absent | not applicable | recorded candidate | complete installs; rollback removes candidate |
| absent | recorded candidate identity | not applicable | missing | complete records/verifies install; rollback removes target |
| either | unchanged original state | missing/not applicable | owned unpublished candidate | complete is forbidden; rollback deletes only that transaction-owned untrusted entry |
| either | any different identity/digest/type | any unexpected entry | any different identity/digest/type | explicit conflict; mutate nothing |

After `Committed`, rows that already name the recorded candidate/final object may only advance cleanup; a missing or mismatching final target is an explicit external-modification conflict, never grounds for rollback. Recovery applies this matrix to all targets before the first recovery mutation, then revalidates the affected row immediately before each handle-relative operation.

## 0.16 New-file permission and security-metadata policy

On Unix, every candidate is created exclusively at mode `0600`, and the implementation immediately verifies/reapplies `0600`. Before worker threads start, the single-threaded CLI captures the process umask using the Phase 0-frozen race-free bootstrap procedure, immediately restores it, and never changes it after worker creation. A new target’s final ordinary mode is exactly `0666 & ((!captured_umask) & 0o777)`; the complement is explicitly masked to the platform’s ordinary permission-bit width before the `0666` intersection. Apply that mode to the candidate immediately before installation. The candidate is never broader than `0600` during preparation and never broader than its final mode after that chmod. Existing targets continue to receive the current mode read from the validated backup. Protocol v1 has no requested-mode field or CLI mode override.

Phase 0 and platform tests include golden values for captured umasks `000`, `002`, `022`, `027`, `077`, and `777`, producing new-file modes `0666`, `0664`, `0644`, `0640`, `0600`, and `0000` respectively. The `0777` case must prove that a zero-mode candidate remains installable through the already-open handle and is reported accurately.

On Unix filesystems with default ACLs, transaction-private creation does not by itself reproduce target-parent inheritance. For a new target, immediately before installation the implementation must derive, apply, and verify the effective target-parent default ACL combined with the final ordinary mode according to the Phase 0-frozen platform rule. If required ACL inheritance cannot be inspected or reproduced, fail before mutation with `FILESYSTEM_SEMANTICS_UNSUPPORTED`; do not silently install a file with the transaction directory’s ACL. The candidate remains protected by the owner-only transaction directory while final metadata is applied.

On Windows, candidates use non-inheritable handles and restrictive ACLs while inside the transaction-private directory. Because rename from that directory does not reliably recreate target-parent inheritance, immediately before installation the implementation must compute and apply the same frozen effective inherited ACL/owner class that a newly created file in the validated target parent would receive, then verify it through the retained handle. If that cannot be represented or verified without granting unintended access, fail before target mutation with `FILESYSTEM_SEMANTICS_UNSUPPORTED`. For an existing target, any security-semantic difference remains subject to the default metadata-loss rejection and explicit opt-in in Section 4.2. The Windows read-only attribute is never silently copied or discarded: Phase 0 freezes its default error, opt-in warning, and final attribute state. No POSIX-mode equivalence is promised. Platform tests record effective ACL/attribute behavior before staging, before rename, and after installation.

For **every** new-target security metadata class recognized by Section 4.2 or by a claimed support-matrix row, `docs/metadata.md` and `docs/platform-support.md` must assign exactly one frozen disposition:

1. `derived_applied_verified`: derive the metadata that a direct secure creation in the validated target parent would receive, apply it to the staged candidate before installation, and verify it both before and after rename;
2. `intentionally_absent`: prove with the Phase 0 platform spike that an ordinary new regular file in that parent would not receive the class, and verify absence on the candidate and installed target; or
3. `unsupported`: fail before the first target mutation with `FILESYSTEM_SEMANTICS_UNSUPPORTED`.

At minimum the per-platform table covers:

* Linux/Unix default ACLs, security namespaces and labels (including SELinux/SMACK-style security xattrs where present), inheritable or creation-derived file flags, ownership/group derivation, and other security-relevant xattrs exposed by the filesystem.
* macOS ACLs, ownership/group derivation, security and quarantine-related xattrs, inherited flags, and resource-fork/Finder metadata. A resource fork or ordinary user xattr may be `intentionally_absent` only when the direct-create spike proves it is not parent-derived; absence is still verified.
* Windows effective inherited DACL/SACL and owner/group classes that the process is permitted to inspect, integrity labels and other security descriptors, creation-derived file attributes/flags, alternate data streams, and reparse state. Alternate streams may be `intentionally_absent` only with verified direct-create equivalence.

An uninspectable SACL or security-label class is not silently ignored: the matrix must prove it irrelevant/absent for that row or mark the row unsupported. Candidate metadata originating only from the transaction-private directory must never leak into the final target. These new-file rules are not bypassed by `--allow-metadata-loss`, which governs destructive replacement of existing targets rather than incorrect inheritance for a newly created target.

## 0.17 Deterministic crash failpoints

Test builds expose named failpoints after every published record and before/after every candidate create, permission application, backup rename, install rename, verification, state transition, cleanup unlink, and directory removal. Production builds do not enable failpoints.

Scenario tests launch CodeSplice as a child process with one failpoint selected through a test-only inherited channel, wait for a “reached” acknowledgement, forcibly terminate the child without unwinding, then run inspection and recovery in a fresh process. In-memory injected errors alone do not satisfy a crash checkpoint. Failpoint names are stable and include transaction sequence plus target commit index in test reports.

## 0.18 Required fixtures

Create at least 88 normative fixtures, including:

1. LF.
2. CRLF.
3. Lone CR.
4. Mixed terminators.
5. Empty file.
6. Only LF.
7. Only CRLF.
8. Missing final newline.
9. NUL bytes.
10. Non-UTF-8 bytes.
11. Very long line.
12. Cross-file move.
13. Cross-file copy.
14. Same-file forward move.
15. Same-file backward move.
16. No-op at source start.
17. No-op at source end.
18. Destination inside own source.
19. Destination inside another moved range.
20. Insertion at deletion start.
21. Insertion at deletion end.
22. Multiple insertions at one offset.
23. Overlapping moves.
24. Copy overlapping a move.
25. Conflicting preconditions.
26. Hard-link aliases.
27. Case aliases.
28. New-file destination.
29. New-file multiple insertions.
30. New-file invalid line anchor.
31. Whole-file move.
32. Workspace traversal.
33. Workspace root symlink.
34. Symlink parent.
35. Windows junction or reparse point.
36. `.codesplice` operation target.
37. `.codesplice` existing as symlink.
38. Root identity replacement.
39. Expected plan mismatch.
40. Permission change after preview.
41. Reserved candidate-name collision.
42. Reserved backup-name collision.
43. Manifest path traversal.
44. Torn state record.
45. Transaction directory without manifest.
46. `before_line` and `after_line` for every terminator.
47. Unterminated-final-line anchors.
48. Empty-file line-anchor rejection.
49. Adjacent deletions with a boundary insertion.
50. No-op move overlapping an effectful source range.
51. Line and byte anchors resolving to one offset.
52. Absent case-equivalent targets.
53. Absent Unicode-normalization-equivalent targets follow the native per-platform rule (including distinct Windows UTF-16 sequences).
54. `.codesplice.lock` spelling alias.
55. Snapshot mutation during read and bounded retry.
56. Candidate replaced with identical bytes and different identity.
57. State/manifest digest disagreement.
58. State sequence gap, regression, and fork.
59. New-file mode under representative umasks.
60. Binary and truncated diff.
61. Normal-mode process crash.
62. Durable-mode system-crash model.
63. Committed transaction with partial cleanup.
64. Candidate hash-truncation collision detected before publication.
65. Crash immediately after candidate creation and before candidate-identity publication, classified as `owned_unpublished_candidate`.
66. Durable power loss after candidate file sync but before candidate-parent-directory sync.
67. Source-only copy input changes after `Prepared` and before the first target mutation.
68. Unfinished older transaction blocks a new commit.
69. Corrupt or partially cleaned older transaction blocks a new commit.
70. A 1 GiB source copied 10,000 times is rejected during planning with no filesystem artifact.
71. A high-line-count file exhausts each line-count/index-memory budget without uncontrolled allocation.
72. Candidate installation encounters an externally created target and no-replace fails without overwrite.
73. Rollback restoration encounters an externally created target and no-replace fails without overwrite.
74. A previous protocol-v1 validating client accepts every frozen v1 response; proposed new v1 enum/error/operation values fail the compatibility gate and require v2.
75. Workspace lock repair with no transaction entries.
76. Workspace lock repair refused for unfinished, corrupt, and cleanup-only transactions according to the frozen matrix.
77. Existing ACL, security xattr, macOS resource fork, Windows alternate stream, file flag, multiple hard link, ownership mismatch, and Windows read-only metadata produce the frozen default error and opt-in warning behavior.
78. JSON nesting, response-size, identity-token, `PathEquivalenceKey`, metadata-inspection, transaction-count, state-record-count, recovery-byte, and control-directory budgets.
79. No-op commit on a workspace without a lock or control directory creates neither.
80. Runtime filesystem probes reject unsupported case, normalization, identity, network/overlay, cross-mount rename, no-replace, and directory-sync semantics.
81. Windows precomposed and decomposed canonically equivalent names remain distinct path keys on both case-sensitive and case-insensitive NTFS directories unless native lookup identifies the same entry.
82. Windows lock bootstrap uses `NtCreateFile` with `RootDirectory`; backup/install/restore use `SetFileInformationByHandle`, `FILE_RENAME_INFO.RootDirectory`, and `ReplaceIfExists = FALSE`.
83. A no-op commit with an existing busy lock returns `TRANSACTION_BUSY`; with unfinished/corrupt/partially cleaned state it returns `TRANSACTION_RECOVERY_REQUIRED`; each case writes nothing.
84. A failure after backup and a failure after an earlier target install both enter `RollingBack`, record progress for every affected target, and return recovery required when rollback cannot finish.
85. Lock repair crashes at every dual-slot write boundary and never replaces the lock object; status selects the old/new valid generation or returns the frozen invalid result.
86. Concurrent lock initialization or repair makes `doctor --status` return `TRANSACTION_BUSY`, never a transient corruption diagnosis.
87. The evidence hierarchy selects exact matrix rows, permits a secured post-lock/pre-manifest write probe only where frozen, and rejects ambiguity; no read-only result claims to prove rename or durability semantics.
88. Every claimed platform row assigns and verifies a new-file disposition for ACLs, security labels/xattrs, ownership, flags/attributes, resource forks, and alternate streams.

## Phase 0 checkpoint

Pass only when:

* Crate ownership is cycle-free.
* Workspace-root behavior is frozen.
* `.codesplice` behavior is frozen.
* `.codesplice.lock` bootstrap, Unix owner/mode validation, Windows owner/DACL validation, `NtCreateFile` root-relative opening, identity binding, persistence, lifetime, in-place dual-slot repair, and `doctor --status` diagnostic locking are frozen.
* The OS-plus-filesystem evidence hierarchy, support matrix, optional secured write probes, and conservative rejection behavior are frozen; path equivalence for existing and absent entries is proven on every supported matrix row without Windows Unicode normalization.
* Plan-hash encoding is frozen.
* Golden plan vectors include annotated byte dumps.
* Edit composition and every line-anchor offset are frozen.
* Single-target and multi-target commits share one transaction model.
* The delta-plus-periodic-snapshot transaction grammar, record/control bounds, complete state machine, and both durability protocols are frozen.
* Transaction-private staging, observable candidate pre-creation authorization rules, the `owned_unpublished_candidate` crash window, and candidate/final identity comparisons are frozen.
* Preview wording does not promise stable access time.
* Successful no-op commit semantics and the noncreating transaction-health check are frozen, including busy and recovery-required outcomes.
* Permission concurrency behavior is frozen.
* New-file permissions, every security-metadata disposition, and all required umask golden values are frozen.
* Handle-relative no-replace target-to-backup, candidate-to-target, and backup-to-target restoration—including Windows `FILE_RENAME_INFO.RootDirectory` with `ReplaceIfExists = FALSE`—are required and proven feasible for every supported matrix row.
* `commit_started`, `backed_up`, and `installed` publication order and transaction-wide `RollingBack` behavior are frozen for single- and multi-target failure paths.
* Manifest target records are fully specified.
* Orphan handling is frozen.
* Protocol-v1 schemas and the complete CLI grammar validate all normative examples, reject duplicate/unknown keys, freeze every v1 enum/error/warning, and pass previous-v1-client compatibility tests.
* Output-amplification, line-index, metadata-inspection, planning-memory, disk, JSON, response, identity/key, state/recovery, and control-directory limits are frozen with checked-arithmetic rules.
* Source-only inputs are included in the final pre-mutation revalidation boundary.
* Metadata detection, default rejection, explicit opt-in, and Unix/macOS/Windows warning/error behavior are frozen.
* All human output contexts escape terminal controls.
* Exit categories, error context, arbitrary-byte diff behavior, and failpoint mechanics are frozen.
* `docs/performance-methodology.json` freezes the Phase 8 reference hardware/filesystem profiles, benchmark method, workloads, corpus durations/seeds, hard resource/SLO ceilings, and the 15% time/10% peak-memory budgets for changes after the first approved measurement; it contains no invented measured results and no `TBD` values.
* At least 88 fixtures exist.
* No production editing engine exists.

### Phase 0 evidence

Phase 0 does not require or permit production filesystem editing or transaction behavior. Its evidence is divided into:

1. **Normative examples and state-transition tables:** required for every item below and checked for internal consistency.
2. **Required non-production executable specification tests:** pure models, schema-validation/compatibility tests, golden encoders, resource-accounting models, and in-memory/test-double state machines live under documentation tooling or a Phase 0-only test-support package. They may not be linked into production crates as an editing or transaction engine and may not mutate real workspace targets.
3. **Required platform feasibility spikes:** minimal disposable programs may operate only inside test-owned temporary directories to prove identity, no-replace rename, directory sync, case/normalization, metadata inspection, and lock-repair primitives for each proposed support-matrix row. They are evidence, not production editing code.
4. **Production behavioral demonstrations:** explicitly deferred to the phase named below and not part of the Phase 0 pass/fail decision.

Present normative examples for:

1. A same-file no-op move.
2. A cross-file move with expected plan.
3. A permission change between preview and commit.
4. A workspace-root replacement conflict.
5. A transaction directory created before candidate generation.
6. A target backup collision rejected through no-replace rename.

Phase 0 must additionally check in executable or model-based evidence for these blocking cases:

1. Crash immediately after candidate creation but before candidate-identity publication; recovery folds to `owned_unpublished_candidate`, refuses completion, and permits only identity-checked rollback deletion.
2. Power loss after candidate file sync but before candidate-parent-directory sync; no durably published state may promise candidate existence.
3. A source-only copy input changes after `Prepared`; final input revalidation aborts before the first target mutation.
4. An unfinished, corrupt, or partially cleaned older transaction exists when a new commit starts; the locked stale-transaction gate returns `TRANSACTION_RECOVERY_REQUIRED`, except for the frozen safe cleanup-only path.
5. A 1 GiB source is copied 10,000 times; checked planning rejects output amplification before any lock/control/transaction write.
6. A high-line-count file reaches the per-file, aggregate, index-byte, or total planning-memory limit without an uncharged allocation or panic.
7. Candidate installation and backup-to-target rollback restoration each encounter an externally created destination; handle-relative no-replace fails without overwrite.
8. Every response validates against the frozen previous protocol-v1 schema; attempting to add a v1 enum, operation, warning, or error demonstrates a compatibility failure and requires v2.
9. Lock repair succeeds only in the no-outstanding-transaction model and is refused for unfinished, corrupt, or not-fully-cleaned transaction states.
10. ACL, security-xattr, flag, multiple-hard-link, ownership, macOS resource-fork, Windows alternate-stream, inherited-ACL, and read-only cases produce exactly the frozen default error or opt-in warning.
11. Windows native comparison fixtures preserve distinct precomposed/decomposed UTF-16 names and validate ordinal ignore-case behavior without normalization.
12. Windows feasibility spikes prove root-relative lock opening with `NtCreateFile` and no-replace rename with `SetFileInformationByHandle`/`FILE_RENAME_INFO`.
13. No-op health models cover no artifacts, clean existing artifacts, busy lock, missing lock with control state, unfinished state, corrupt state, and partially cleaned state without any write.
14. Single- and multi-target failures at every post-mutation boundary publish/attempt `RollingBack` and cover rollback-record failure as recovery required.
15. Dual-slot lock repair failpoints preserve one lock object and a selectable old/new generation; concurrent `doctor --status` returns busy.
16. The evidence-hierarchy model prevents read-only observations from independently asserting no-replace or durability support and bounds/cleans any permitted write probe.
17. New-file metadata tables have no unclassified security class on any claimed row, and each `derived_applied_verified`, `intentionally_absent`, or `unsupported` behavior has a platform fixture.
18. Canonical planner vectors prove the EOF sentinel/event order and exact output-record inclusion set, including exclusion of an unchanged copy-only source.

Production evidence is deferred as follows:

```text
same-file no-op and cross-file expected-plan behavior: Phases 4–5
workspace-root replacement detection: Phase 3 and commit revalidation in Phase 6
permission change, transaction-before-candidate, and backup collision: Phase 6
multi-target state/recovery visibility: Phase 7
systematic crash failpoints: Phases 6–8
```

**Stop after the checkpoint.**

---

# Phase 1 — Create the Rust workspace and enforce boundaries

## Objective

Establish the crate structure without implementing editing behavior.

## Tasks

Create:

```text
codesplice-core
codesplice-fs
codesplice-protocol
codesplice-cli
codesplice-test-support
```

Use:

* Stable Rust pinned in `rust-toolchain.toml`.
* Rustfmt.
* Clippy.
* Serde.
* Clap.
* SHA-256.
* Typed errors.
* Property testing.
* CLI testing.
* Temporary-file test support.
* Reviewed cross-platform locking and filesystem primitive crates.

Production code should avoid direct unsafe code.

Platform operations should use reviewed crates that encapsulate required operating-system APIs.

Configure CI for:

```text
Linux
macOS
Windows
```

## Phase 1 checkpoint

Pass only when:

* The workspace compiles.
* CI passes.
* `codesplice-core` owns immutable snapshots.
* `codesplice-fs` depends on core, not vice versa.
* `codesplice-fs` does not depend on protocol.
* `FileId`, `SnapshotFileId`, and `PlannedFileId` ownership is explicit.
* No editing or transaction behavior exists.

### Commands

```bash
cargo metadata --no-deps
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
```

**Stop after the checkpoint.**

---

# Phase 2 — Implement protocol v1 and domain conversion

## Objective

Implement the frozen protocol-v1 schemas and the one-operation `move`/`copy` convenience constructors. Phase 2 may not invent or revise wire semantics; any schema ambiguity returns the project to Phase 0.

## Required protocol behavior

* The checked-in protocol-v1 JSON Schemas are normative and every example validates against them.
* Duplicate JSON keys and unknown object fields fail before domain conversion.
* `workspace_root` must equal `"."`.
* Unknown operations fail explicitly.
* Unknown selectors fail explicitly.
* Unknown anchors fail explicitly.
* Unknown semantic fields fail explicitly.
* Every source and destination path has an explicit precondition.
* Source preconditions and existing-destination preconditions must be SHA-256.
* Absent-destination preconditions must be `must_not_exist`.
* Request bytes, JSON nesting depth, textual UTF-8 path lengths, request operation/path counts, and request-derived response estimates are validated before large allocation with checked arithmetic. Filesystem-generated path-equivalence keys, identity tokens, and metadata are explicitly deferred to Phase 3.
* Direct `move` and `copy` parse into a one-operation `BatchSpecification` and have no separate semantics.

## Expected-plan CLI policy

Commit commands must require one of:

```text
--expect-plan sha256:...
--accept-current-plan
```

The expected-plan value is an execution option, not part of `BatchSpecification`.

## Error codes

Add:

```text
EXPECTED_PLAN_MISMATCH
PLAN_CHANGED_DURING_COMMIT
DIGEST_PRECONDITION_FAILED
FILE_IDENTITY_ALIAS
INVALID_WORKSPACE_ROOT
WORKSPACE_IDENTITY_CHANGED
RESERVED_CONTROL_PATH
CONTROL_DIRECTORY_INVALID
TRANSACTION_BUSY
TRANSACTION_RECOVERY_REQUIRED
RESOURCE_LIMIT_EXCEEDED
UNSUPPORTED_COMMIT_PRIMITIVE
WORKSPACE_LOCK_INVALID
PATH_EQUIVALENCE_UNSUPPORTED
FILESYSTEM_SEMANTICS_UNSUPPORTED
SNAPSHOT_UNSTABLE
LINE_ANCHOR_OUT_OF_RANGE
PLAN_ENCODING_LIMIT
TRANSACTION_RECORD_CORRUPT
UNSUPPORTED_DURABILITY_PRIMITIVE
RESERVED_NAME_COLLISION
CANNOT_COMPLETE_PREPARING
METADATA_LOSS_REQUIRES_OPT_IN
```

Errors and warnings require structured context. Protocol v1 also freezes the metadata warnings named in Section 4.2 and every other warning in the Phase 0 registry; Phase 2 may not add identifiers absent from the checked-in v1 schema.

## Phase 2 checkpoint

Pass only when:

* DTOs convert to core specifications.
* Invalid workspace-root values fail.
* Invalid domain states cannot reach planning.
* Expected-plan policy is parsed separately.
* Resource limits are applied during parsing.
* No filesystem access exists in protocol conversion.
* Stable exit categories, retryability, JSON pointers, operation indices, redaction, warnings, and JSON stdout/stderr rules pass golden tests.
* Every human renderer escapes controls in every untrusted field, not only diff bodies.
* The frozen prior-v1 schema accepts every v1 response and the compatibility test proves that proposed enum/error additions require v2.

### Demonstration

Deserialize:

1. Valid move.
2. Valid new-file copy.
3. Invalid workspace root.
4. Unknown selector.
5. Oversized request.
6. Commit options with expected plan.
7. Missing mandatory precondition.
8. Duplicate key and unknown field.
9. Every recovery request/response state.
10. Every stable error exit category.

**Stop after the checkpoint.**

---

# Phase 3 — Implement secure workspace and snapshot acquisition

## Objective

Create trustworthy immutable core snapshots and filesystem commit context.

## Snapshot acquisition API

```rust
pub trait SnapshotReader {
    fn acquire(
        &self,
        root: &SecureWorkspaceRoot,
        specification: &BatchSpecification,
    ) -> Result<SnapshotAcquisition, SnapshotError>;
}
```

## Requirements

* Resolve the CLI workspace root.
* Validate root path components.
* Reject root symlinks, junctions, and unsupported reparse points.
* Capture root physical identity.
* Validate `.codesplice` only when it already exists; preview must not create it.
* Validate operation paths.
* Reject reserved control paths.
* Build and validate `PathEquivalenceKey` values for every existing and absent operation path.
* Read each physical file through one handle per attempt using Section 0.12; retry no more than three total attempts.
* Convert platform identity into core identity tokens.
* Populate core `WorkspaceSnapshot`.
* Populate filesystem `CommitContext`.
* Enforce snapshot-byte, per-file/total line-count, compact line-index, identity/key, and total planning-memory limits before allocation.
* Detect path aliases.
* Detect stable digest-precondition conflicts without retrying them as snapshot instability.
* Represent absent destinations explicitly.
* Prefer handle-relative resolution and record every documented platform fallback.

## Line indexing

Implement line indexing in `codesplice-core`.

Property-test arbitrary bytes, mixed terminators, and long lines.

## Phase 3 checkpoint

Pass only when:

* Snapshot acquisition returns cycle-free core types.
* Root identity is captured.
* Root and operation path policies pass.
* Read-only identification selects an exact frozen OS-plus-filesystem matrix row; preview never runs write probes, and every ambiguous or unsupported observation fails without mutation with `FILESYSTEM_SEMANTICS_UNSUPPORTED`.
* Junction and reparse-point behavior is tested on Windows.
* Preview acquisition creates no control directory.
* Snapshot, line-count, compact-index, key/identity, and total planning-memory limits are enforced with checked arithmetic.
* Mutation-during-read detection and the bounded `SNAPSHOT_UNSTABLE` result pass.
* Case/normalization-equivalent absent destinations and both reserved names are rejected according to native platform behavior.
* No filesystem writes occur.

### Demonstration

Show:

* Valid snapshot acquisition.
* Hard-link alias rejection.
* Workspace-root symlink rejection.
* `.codesplice` symlink rejection.
* Stale digest rejection.
* Snapshot limit rejection.
* High-line-count/index-memory rejection.
* Mutation during read followed by retry and eventual structured failure.
* Stable wrong digest returning `DIGEST_PRECONDITION_FAILED` without retry.
* Absent-destination equivalence collision.

**Stop after the checkpoint.**

---

# Phase 4 — Implement the pure segment-based planner

## Objective

Resolve operations into immutable output recipes.

## Planner API

```rust
pub fn plan(
    snapshot: &WorkspaceSnapshot,
    specification: &BatchSpecification,
) -> Result<EditPlan, PlanError>;
```

This API now compiles because `WorkspaceSnapshot` belongs to `codesplice-core`.

## Planned output

```rust
pub struct PlannedOutput {
    pub file_id: PlannedFileId,
    pub path: WorkspacePath,
    pub path_equivalence_key: PathEquivalenceKey,
    pub original_sha256: Option<Digest>,
    pub resulting_sha256: Digest,
    pub resulting_length: u64,
    pub segments: Vec<OutputSegment>,
    pub change_kind: ChangeKind,
}

pub enum OutputSegment {
    OriginalSlice {
        source_file: SnapshotFileId,
        range: ByteRange,
    },
    PayloadSlice {
        operation_index: u32,
        source_file: SnapshotFileId,
        range: ByteRange,
        sha256: Digest,
    },
}
```

## Plan hashing

Implement the frozen canonical encoding.

Use synthetic identities for golden vectors.

Document that live filesystem identities make plan hashes workspace-snapshot-specific.

## No-op behavior

A no-op move remains visible in:

```text
resolved operations
human preview
JSON report
```

but produces no candidate or transaction when no other output changes.

A commit whose unlocked validated plan is a no-op runs the Section 7.5 noncreating transaction-health check before success. If neither reserved artifact exists it returns immediately; otherwise it diagnoses busy/invalid/recovery-required state under the existing lock without writing. Its successful report has `transaction_id: null`, `transaction_state: "not_created"`, `transaction_health: "clean"`, and an empty changed-file list. A successful no-op commit performs no intentional filesystem mutation, including first-time creation, initialization, repair, or cleanup of `.codesplice.lock` or `.codesplice`.

## Phase 4 checkpoint

Pass only when:

* Segment planning passes all fixtures.
* Complete output files are not retained.
* Per-file and total resulting bytes, candidate bytes, segment counts, projected transaction disk use, response estimate, and total planning memory are charged with checked arithmetic before transaction creation.
* The 1 GiB-to-10,000-output amplification fixture is rejected without a filesystem write.
* Plan hashing follows the frozen binary format.
* Golden hashes use synthetic identities.
* Same-file no-op behavior is correct.
* The Section 0.5 event stream is the sole composition algorithm and every boundary-order fixture passes.
* No filesystem writes occur.

### Demonstration

Show one plan containing:

* A real move.
* A copy.
* A no-op move.
* A new destination.
* Its canonical plan hash.
* The annotated canonical byte encoding that produced that hash.

**Stop after the checkpoint.**

---

# Phase 5 — Implement inspect and preview

## Objective

Expose snapshot and plan behavior without intentional mutation.

## Commands

```text
codesplice inspect
codesplice move --preview
codesplice copy --preview
codesplice apply --preview
codesplice capabilities
codesplice protocol-version
```

Commit remains disabled.

## Preview rules

Preview:

* Does not acquire the mutation lock.
* Does not create `.codesplice`.
* Does not create transactions.
* Does not create candidates.
* Does not create backups.
* Does not write destination files.
* Does not intentionally modify metadata.
* May cause filesystem-controlled access-time changes.
* Applies the bounded arbitrary-byte diff policy in Section 10.1, including `--no-diff`.

## Inspect output

Physical identity support is required on release platforms.

Do not expose:

```text
file_identity_available: false
```

on a supported release target.

Instead report an identity kind and a safe opaque identifier or hash:

```json
{
  "file_identity": {
    "kind": "unix_device_inode",
    "opaque_hash": "sha256:..."
  }
}
```

Do not expose sensitive raw platform identifiers unless required.

## Expected-plan output

Preview returns:

```text
plan_hash_version
plan_sha256
workspace_identity_hash
```

The workspace identity hash is diagnostic and does not replace the complete plan hash.

## Phase 5 checkpoint

Pass only when:

* Preview performs no intentional mutation.
* No `.codesplice` artifact appears.
* No content, permission, rename, create, or explicit timestamp change occurs.
* Access time is not part of the assertion.
* JSON output is one parseable value.
* Plan hash is returned.
* Commit remains disabled.
* Binary, mixed-terminator, disabled, and work-budget-truncated diff reports pass.

### Demonstration

Run preview, compare:

* File bytes.
* File existence.
* Permissions.
* Modification timestamps.
* Directory entries.
* `.codesplice` absence.

Document access-time behavior separately.

**Stop after the checkpoint.**

---

# Phase 6 — Implement the transaction substrate and single-target commits

## Objective

Implement the complete versioned transaction model first, then use it for one-target commits.

There is no non-journaled commit path.

## 6.1 Transaction creation order

Before creating any persistent artifact, perform an unlocked preflight snapshot/plan and reject a mismatched `--expect-plan`. Then:

0. If the unlocked validated plan changes no files, run the Section 7.5 noncreating transaction-health check. Return frozen no-op success only when health is clean; return `TRANSACTION_BUSY`, `TRANSACTION_RECOVERY_REQUIRED`, or `WORKSPACE_LOCK_INVALID` as specified otherwise. Do not create, initialize, open-for-write, repair, or clean `.codesplice.lock`/`.codesplice`, and do not create a transaction. A plan that becomes no-op only after a previously changing unlocked preflight is a plan-change conflict, not a successful no-op.
1. Acquire or bootstrap the exact persistent `.codesplice.lock` from Section 7.5.
2. Revalidate workspace identity and securely inspect an already-existing `.codesplice/transactions` directory.
3. Apply the normative stale-transaction gate: boundedly scan and fold every entry while locked. Any unfinished, corrupt, partially published, unclassifiable, partially cleaned, or scan/control-limit-exceeding transaction set returns `TRANSACTION_RECOVERY_REQUIRED` with a stable `reason` before replanning or new transaction creation. The only exception is a valid `Committed`, `RolledBack`, `CleaningCommitted`, or `CleaningRolledBack` state whose complete target classification already proves the recorded all-new/all-old outcome; the implementation may advance/complete cleanup-only work, rescan to empty, and continue. It may not complete or roll back target content as part of a new commit.
4. Recompute the complete stable snapshot and plan while holding the lock.
5. Verify `--expect-plan` again when provided and verify the plan still changes at least one file.
6. Securely validate or create `.codesplice` and `.codesplice/transactions` relative to root/control handles.
7. Generate a validated 128-bit random transaction ID encoded as 32 lowercase hexadecimal digits.
8. Select the exact support-matrix row for every rename pair and, only when that row permits it, run and completely clean the bounded secured write probe. Then exclusively create the restrictive transaction-private directory, capture its identity, and verify its volume relationship to every target parent. Do not claim the directory creation itself proves no-replace or durability semantics.
9. Determine and collision-check all bounded candidate and backup basenames.
10. Publish the complete versioned immutable manifest using Section 0.14.
11. Publish state sequence `0` as the full `Preparing` snapshot with all candidates `authorized_missing`.
12. Only then create candidate files, persisting each physical identity with a `candidate_created` delta.

This prevents a candidate from existing without a durable transaction ownership record and gives the post-create/pre-identity crash a normative recovery classification.

## 6.2 Bounded reserved names

Do not append an unbounded suffix to the original filename.

Use bounded basenames such as:

```text
.cs-<short-transaction-id>-<target-hash>-c
.cs-<short-transaction-id>-<target-hash>-b
```

Requirements:

* The short transaction component is the first 16 hex digits of the validated transaction ID.
* The target hash is the first 20 bytes (40 lowercase hex digits) of `SHA256("CODESPLICE-TARGET-NAME\0" || encoded PathEquivalenceKey)`. The encoded `PathEquivalenceKey` is the only path-derived input; the normalized target-path spelling is never substituted.
* Exact v1 basenames are `.cs-<16hex>-<40hex>-c` and `.cs-<16hex>-<40hex>-b`, each 63 ASCII bytes.
* Single path component.
* No user-controlled path separators.
* Candidate creation uses exclusive create.
* Backup installation uses no-replace rename.
* All generated candidate and backup names must be distinct within the transaction. A truncation collision returns `RESERVED_NAME_COLLISION` before manifest publication; v1 does not choose an alternate name.
* A collision with any existing entry, including stale transaction artifacts or unrelated user data, causes safe transaction failure. No colliding entry is removed or replaced.
* The manifest is immutable and is never rewritten because of a name collision.
* The 63-byte length is below the minimum 255-byte component limit required of supported target filesystems.

## 6.3 Handle-relative no-replace rename primitive

Do not reserve a backup through check-then-create or placeholder removal.

Implement a platform abstraction:

```rust
pub trait RenameNoReplace {
    fn rename_no_replace(
        &self,
        source_parent: &SecureDirectoryHandle,
        source_name: &ValidatedBasename,
        destination_parent: &SecureDirectoryHandle,
        destination_name: &ValidatedBasename,
    ) -> Result<(), RenameNoReplaceError>;
}
```

Use reviewed platform support such as:

```text
Linux:
  renameat2 with RENAME_NOREPLACE through a reviewed crate

macOS:
  exclusive no-replace rename primitive through a reviewed crate

Windows:
  SetFileInformationByHandle with FILE_RENAME_INFO.RootDirectory set to the
  validated destination-parent handle and ReplaceIfExists = FALSE
```

The Windows source is the securely opened source handle; `RootDirectory` supplies the handle-relative destination. Phase 0 must freeze the exact `FILE_RENAME_INFO` layout/flags used by the supported Windows versions and prove collision, reparse, cross-volume, and identity behavior. A reviewed crate wrapper is acceptable only when it preserves those documented semantics.

There must be no fallback that performs:

```text
check destination absent
then ordinary replacing rename
```

The abstraction accepts no arbitrary paths. It must be used for target-to-backup, candidate-to-target, and backup-to-target rollback restoration. All participating directory handles, basenames, equivalence keys, identities, and same-filesystem relationships are validated before the first target mutation. Secure no-replace support for all three directions is mandatory. If a required safe primitive is unavailable, fail before target mutation with:

```text
UNSUPPORTED_COMMIT_PRIMITIVE
```

## 6.4 Candidate preparation

After the manifest is durable enough for the selected mode:

1. Exclusively create the candidate.
2. Capture its physical identity from the retained handle but publish no existence state.
3. Apply restrictive candidate permissions from Section 0.16.
4. Stream plan segments into it and hash while writing.
5. Verify digest, length, path identity, and parent identity through the retained handle.
6. Perform the selected Normal/Durable file flush; in Durable mode, sync the candidate file and then its transaction-directory parent.
7. Only after those checks/orderings, publish a `candidate_created` delta containing the identity, close/reopen as required, and revalidate the recorded identity.
8. After all candidates are ready, publish the full `Prepared` snapshot containing every candidate identity.
9. Revalidate every initial input record under Sections 4.7 and 0.12 immediately before the first target mutation. A source-only copy input is not exempt.

## 6.5 Existing-target commit

1. Revalidate root, parent, target path type, and target identity.
2. Ensure `commit_started` has been published before this or any earlier target mutation.
3. Move target to backup with handle-relative no-replace semantics, apply the selected durability ordering, hash/identify/verify the moved object, then publish `backed_up`.
4. Inspect the backup’s current link/security metadata and read its current ordinary permission bits.
5. If the observation now requires metadata-loss opt-in that was not granted, or any post-backup verification/policy step fails, enter `RollingBack` and run transaction-wide recorded rollback. Do not report the conflict alone.
6. Apply and verify those current bits and the frozen final metadata policy on the recorded candidate.
7. Revalidate candidate identity, install it with handle-relative no-replace semantics, apply the selected durability ordering, verify final type/identity/length/digest/metadata, then publish `installed`.
8. After every target is installed and verified, publish `Committed`.
9. Enter committed cleanup and remove backup/transaction artifacts under the recorded cleanup protocol.

## 6.6 New-target commit

1. Revalidate root, parent identity, and complete target absence.
2. Ensure `commit_started` has been published before this or any earlier target mutation.
3. Derive, apply, and verify every Section 0.16 new-file permission/security-metadata class on the recorded candidate, then revalidate candidate identity.
4. Install without replacement, apply the selected durability ordering, verify final type/identity/length/digest/metadata, then publish `installed`.
5. Any failure after this or an earlier target mutation enters `RollingBack` and attempts transaction-wide recorded rollback; incomplete rollback returns `TRANSACTION_RECOVERY_REQUIRED`.
6. After every target is installed and verified, publish `Committed`.
7. Enter committed cleanup and remove transaction artifacts under the recorded cleanup protocol.

## 6.7 Manifest target record

Every target entry includes:

```text
deterministic commit index
normalized target path
authoritative target PathEquivalenceKey
normalized target parent path
authoritative parent PathEquivalenceKey
target basename
captured target-parent identity
original existence state
original physical identity when present
original digest when present
original length when present
original relevant metadata snapshot
transaction-private directory identity
candidate basename
candidate digest
candidate length
backup basename
expected final type
expected final digest
change kind
```

Candidate physical identity is necessarily unknown when the immutable manifest is published. Sequence `0` instead binds the observable pre-creation authorization (secured transaction-directory identity plus exact authorized basename); after exclusive creation the identity is retained in memory and is stored in the checksummed delta/snapshot chain only after the candidate verification and durability ordering required by Section 0.15, before the last secure handle closes. Backup identity and installed-final identity are similarly stored in progress delta events and snapshots. Manifest digest/length never substitutes for these identities.

For a new target:

```text
original existence state = absent
```

Candidate and backup locations are represented as:

```text
validated transaction-private directory reference
+
validated single-component basename
```

They are not arbitrary manifest paths.

## 6.8 Single-target recovery

Implement immediately:

```text
codesplice recover --list
codesplice recover <ID> --status
codesplice recover <ID> --rollback
codesplice recover <ID> --complete
```

These commands must work for one-target transactions before Phase 6 passes.

Recovery implements the exact state graph, commit point, record publication, candidate/original/final identity comparisons, and Normal/Durable protocols in Sections 0.14–0.15. `Committed` with partial cleanup can only complete cleanup. State/manifest disagreement is corruption, not a guessed recovery action.

## 6.9 Orphan rules

Because the manifest and authorization snapshot are written before candidate creation, transaction-private candidates should always have an ownership record.

Potential orphan states:

### Transaction directory without a manifest

Safe behavior:

* List as `incomplete_transaction_record`.
* Do not inspect or delete anything outside that transaction-directory entry.
* Allow explicit cleanup only after verifying the directory is inside the secure control directory, has the frozen owner-only transaction-directory policy, and contains only bounded unpublished record-temporary names permitted before manifest publication. An unknown or candidate-like entry is a conflict, not guessed ownership.

### Valid manifest with missing candidates

Fresh-process `--complete` is rejected with `CANNOT_COMPLETE_PREPARING` while state is `Preparing`, because the manifest does not retain payload bytes and a partially written candidate is not trusted. Rollback leaves `authorized_missing` slots absent, deletes `candidate_created` entries only when their recorded identity still matches, and handles an existing `authorized_missing` slot only through the complete `owned_unpublished_candidate` rule. In `Prepared` or later, an absent candidate is acceptable only when the state/location matrix proves that exact recorded identity was renamed to the target during an interrupted install; otherwise it is an external conflict. Recovery never synthesizes replacement bytes from unrelated current files.

### Candidate-like filename without a manifest reference

Never delete it automatically merely because its name resembles a CodeSplice reserved name.

## Phase 6 checkpoint

Pass only when:

* Single-target commits use the complete journal.
* A manifest and `authorized_missing` sequence-0 snapshot exist before candidate creation.
* The post-create/pre-identity crash classifies as `owned_unpublished_candidate`; completion is refused and rollback deletes only the transaction-owned untrusted entry.
* Crashes between backup and install are recoverable.
* Handle-relative no-replace movement is used for backup, installation, and rollback restoration; externally created destinations are never overwritten.
* Backup names are bounded.
* Expected-plan mismatch is rejected before transaction creation.
* A successful no-op commit in a fresh workspace creates neither `.codesplice.lock` nor `.codesplice` and reports no transaction; existing clean artifacts are inspected without writes, a busy lock returns `TRANSACTION_BUSY`, and unhealthy transaction state returns `TRANSACTION_RECOVERY_REQUIRED`.
* The stale-transaction gate blocks new work until every older transaction is recovered or safely cleanup-completed.
* A source-only input change after `Prepared` aborts before the first target mutation.
* Current backup permissions are applied to the candidate.
* Single-target recovery commands work.
* Read-only status follows stale-observation rules.
* Every failpoint ends recoverably or in explicit conflict.
* `.codesplice.lock` bootstrap, persistence, malformed-state behavior, workspace binding, contention, and lifetime pass on every release platform.
* `doctor --status` takes a nonblocking diagnostic lock and returns busy during initialization/repair; `doctor --repair-lock` performs in-place dual-slot repair without replacing the lock object and refuses exactly the states frozen in Phase 0.
* Delta folding, periodic snapshots, state/control bounds, monotonic hash chains, atomic publication, checksum corruption, and commit-point behavior pass.
* Candidate replacement by an equal-byte different-identity object is rejected.
* Normal mode passes fresh-process termination recovery tests, and Durable mode proves candidate-parent sync precedes `candidate_created` publication plus all other ordered file/directory-sync traces on every supported filesystem.
* New-target umask goldens pass; ACL/xattr/flag/hard-link/ownership/Windows read-only default rejection and opt-in reporting match the frozen metadata policy; candidates are never intentionally broader.

### Demonstration

Inject crashes:

1. After transaction directory creation.
2. After manifest write.
3. Immediately after candidate creation and during candidate writing.
4. After candidate file sync but before candidate-parent sync.
5. After candidate-parent sync but before candidate-identity publication.
6. After candidate-identity publication.
7. After `Prepared`, then mutate a source-only input.
8. After target-to-backup move.
9. After candidate installation.
10. Before cleanup.

For each persisted transition and filesystem mutation, use the fresh-process failpoint framework rather than only returning an injected in-memory error.

Recover each state.

**Stop after the checkpoint.**

---

# Phase 7 — Generalize transactions to multiple targets

## Objective

Extend the already-working transaction substrate from one target to many targets.

Do not introduce a second transaction engine.

## Prepare sequence

1. Acquire exclusive mutation lock.
2. Revalidate root.
3. Run the locked stale-transaction gate and stop with `TRANSACTION_RECOVERY_REQUIRED` unless only frozen cleanup-only work can safely finish.
4. Recompute plan.
5. Verify expected plan.
6. Select exact support-matrix rows and run any permitted secured post-lock/pre-manifest write probes; reject ambiguity, then verify the transaction-private directory and every target parent have the required recorded same-filesystem relationships.
7. Create the transaction record.
8. Write all target entries, exact authorized candidate basenames, metadata classes, and resource projections to the manifest.
9. Persist the `authorized_missing` initial snapshot.
10. Create all candidates, syncing each parent before its identity delta in Durable mode.
11. Verify all candidate digests and identities.
12. Mark `Prepared`.
13. Revalidate every input record, including source-only copy inputs.
14. Begin no target replacement before every candidate is ready and every input revalidation passes.

## Commit ordering

Targets use deterministic encoded-`PathEquivalenceKey` order. Equivalent-key ties are rejected during planning.

The manifest records `commit_index`.

Publish `commit_started` once after final input revalidation and before the first target mutation.

For each target:

1. Revalidate parent.
2. Revalidate target or absence.
3. Move existing target to the transaction-private backup with handle-relative no-replace semantics.
4. Verify moved backup.
5. Publish `backed_up` for an existing target; an absent target has no backup event.
6. Revalidate current metadata-loss class and capture current permission bits/frozen metadata report, or derive all new-file metadata under Section 0.16.
7. Apply and verify the final metadata policy on the candidate.
8. Install candidate with handle-relative no-replace semantics.
9. Verify final output and metadata class.
10. Publish `installed`.

After all targets have a verified and published `installed` event, publish `Committed`. Any failure after the first target mutation publishes `rollback_started`, transitions to `RollingBack`, classifies every target, and attempts recorded rollback of all affected targets in reverse commit order. A failure to publish or complete rollback returns `TRANSACTION_RECOVERY_REQUIRED` with the original failure preserved as context; it never returns only a per-target conflict while earlier targets remain installed.

## Recovery classification

Each target is classified as:

```text
matches_original
matches_candidate
matches_neither
missing
unexpected_type
identity_mismatch
```

Completion or rollback proceeds only when every required transition is safe.

Classification is per location and uses the Section 0.15 comparison matrix: target against recorded original and candidate/final identity, backup against original identity, candidate entry against candidate identity, and every entry against its recorded parent identity/equivalence key. Digest and length are additional integrity checks, never object identity. The recovery report identifies the compared stored identity kind through an opaque hash rather than exposing raw identity bytes.

## Read-only recovery inspection

`recover --list` and `recover --status`:

* May run without the mutation lock.
* Must read atomic checksummed records.
* Include `observation_may_be_stale`.
* Retry once if state changes while reading.
* Return `TRANSACTION_BUSY` if a consistent observation cannot be obtained.

## Phase 7 checkpoint

Pass only when:

* Multi-target commit reuses Phase 6 machinery.
* No path escapes through manifest data.
* External modifications are not overwritten.
* Rollback restores exact originals.
* Completion produces exact planned outputs.
* Every target proves rename/verify/event ordering; a failure after any target mutation attempts recorded transaction-wide rollback of all affected targets and incomplete rollback returns `TRANSACTION_RECOVERY_REQUIRED`.
* Read-only status handles active transactions safely.
* Mutating recovery requires the exclusive lock.
* Failpoints cover every target and state update.
* Reports and documentation explicitly show that unrelated readers may observe mixed old/new files before recovery converges.

### Demonstration

Interrupt a three-target transaction after the first installation.

Demonstrate:

* Unlocked point-in-time status.
* Locked rollback.
* Locked completion.
* Conflict after third-party modification.

**Stop after the checkpoint.**

---

# Phase 8 — Hardening, fuzzing, security, and performance

## Objective

Validate adversarial input and platform behavior.

## Required security tests

Include:

```text
workspace-root replacement
workspace-root symlink
Windows junction
Windows reparse point
.codesplice symlink
.codesplice non-directory
reserved control-path operation
manifest path traversal
transaction-ID injection
candidate-name collision
backup-name collision
unsupported no-replace primitive
externally created install destination
externally created rollback-restoration destination
torn manifest
torn state
state sequence regression
state-record count/cumulative-byte/snapshot-interval exhaustion
candidate without manifest reference
owned unpublished candidate
transaction directory without manifest
unfinished transaction blocks new commit
permission change after preview
source-only copy input change after Prepared
external content change during commit
external content change during recovery
root identity change during recovery
parent identity change
concurrent commit
concurrent rollback
concurrent read-only status
resource-limit violations
output amplification and transaction-disk projection
line-count, line-index, and planning-memory exhaustion
JSON depth/response and identity/equivalence-key limits
transaction-directory/recovery/control-directory scan limits
concurrent first lock bootstrap
malformed or workspace-mismatched persistent lock record
lock owner/mode/DACL rejection and doctor repair refusal
absent case and Unicode-equivalent target aliases
snapshot changes during open-handle read
candidate replacement with identical bytes and different identity
committed state with partial cleanup
new-file umask and restrictive candidate mode
ACL/xattr/flag/hard-link/ownership/Windows read-only metadata policy
unsupported filesystem/evidence-hierarchy rejection
binary diff escaping and computation-budget exhaustion
terminal-control escaping in every human-output field
```

## Property tests

Required invariants:

```text
selected bytes equal inserted bytes
selected digest equals inserted digest
same snapshot and specification produce same plan hash
preview performs no intentional mutation
expected-plan mismatch produces no transaction artifact
every committed candidate matches planned digest
recovery reaches all-old, all-new, or explicit conflict
```

## Performance tests

Measure:

```text
workspace acquisition
physical identity acquisition
line indexing
segment planning
plan hashing
manifest serialization
candidate streaming
no-replace backup movement
single-target commit
multi-target commit
recovery classification
```

Confirm that full output files are not stored in the plan.

Acceptance separates the frozen method from the first measurement:

* Phase 0's `docs/performance-methodology.json` identifies the reference CPU model/count, RAM, storage, power/performance mode, OS build, filesystem and mount options, Rust toolchain, build profile, benchmark command/seed, workloads, and hard resource/SLO ceilings. It defines measurement; it does not contain pretend timings from a nonexistent transaction engine.
* Checked-in workloads cover 1 MiB, 100 MiB, and 1 GiB snapshots; 1, 100, and 5,000 targets; 1, 10,000, and 1,000,000 segments; 200,000 and 10,000,000 line indexes; a 1 GiB-to-10,000-output amplification rejection; maximum-size manifest/state snapshots; and bounded recovery scans at 1, 1,000, and 10,000 transaction entries.
* Each benchmark uses at least 10 measured runs after 3 warmups. Phase 8 records median, p95 wall time, peak resident memory, and bytes read/written in the first measured `docs/performance-baseline.json`, after the transaction engine exists, and submits it for approval. The first baseline must meet every Phase 0 hard ceiling but has no self-referential regression comparison.
* After that baseline is approved, every subsequent change on the same profile must keep median and p95 time regressions at or below 15% and peak resident-memory regression at or below 10%, unless an updated baseline has an explicit reviewed rationale. Results from other hardware are informational until normalized by a separately frozen profile.
* Limit-rejection workloads must allocate no more than the frozen charged budget, write no transaction artifact, and meet the hard workload-specific ceilings in `docs/performance-methodology.json`.
* Fuzz harnesses cover JSON streaming/schema boundaries, canonical plan/record decoders, path/equivalence parsing, arbitrary-byte line indexing, segment planning, state-chain folding, recovery classification, and terminal escaping. Each harness runs at least 12 aggregate CPU-hours with a pinned engine/version, corpus, dictionary, seed set, memory limit, and timeout. Acceptance is zero crash, panic, sanitizer finding, uncontrolled OOM, nontermination, or invariant violation; minimized reproducers are checked in. CI also runs a deterministic bounded corpus on every change.

## Phase 8 checkpoint

Pass only when:

* Every claimed OS-plus-filesystem matrix row passes identity, path-equivalence, no-replace, directory-sync, metadata, mount, matrix-lookup, and permitted write-probe suites; unclaimed or ambiguous rows reject before mutation.
* Handle-relative no-replace backup, installation, and restoration primitives pass external-destination races.
* Every fuzz harness meets its pinned 12-CPU-hour acceptance run with zero prohibited result and archived corpus metadata.
* Every resource and checked-arithmetic limit is enforced, including output amplification, line-index memory, metadata inspection, transaction disk, state/recovery/control size, JSON depth/response, and identity/key lengths.
* Recovery failpoints pass.
* Permission concurrency behavior passes.
* Workspace and control-directory security pass.
* The first measured `docs/performance-baseline.json` is recorded on the frozen reference profile and every workload meets the Phase 0 hard ceiling. If an approved baseline already exists, the 15% time and 10% peak-memory regression budgets also pass.

**Stop after the checkpoint.**

---

# Phase 9 — Package and run the Codex pilot

## Objective

Verify that agents use the tool safely.

## Recommended agent workflow

```bash
codesplice apply \
  --request split.json \
  --preview \
  --json
```

Record:

```text
plan_sha256
```

Then:

```bash
codesplice apply \
  --request split.json \
  --commit \
  --expect-plan sha256:PREVIEWED_PLAN \
  --json
```

The agent must not use `--accept-current-plan` unless explicitly instructed.

## Pilot scenarios

Run all 15 required scenarios:

1. Move one function to an existing file.
2. Move one function to a new file.
3. Copy a declaration.
4. Reorder methods.
5. Perform a same-file no-op.
6. Split one file into two.
7. Split one file into three.
8. Handle CRLF.
9. Handle non-ASCII identifiers.
10. Reject stale input.
11. Reject expected-plan mismatch.
12. Recover interrupted multi-file commit.
13. Preserve a permission change made after preview.
14. Reject workspace-root replacement.
15. Reject `.codesplice` path targeting.

## Phase 9 checkpoint

Pass only when:

* No payload mismatch occurs.
* Expected-plan workflow works.
* No silent stale overwrite occurs.
* Permission changes are handled according to specification.
* Recovery produces all-old, all-new, or explicit conflict.
* All 15 scenarios complete without manual repair; none is optional or skippable for release qualification.
* Every failure is categorized.

**Stop after the checkpoint.**

---

# Phase 10 — Release `v0.1.0`

Release only when:

* Phases 0–9 pass.
* No unresolved data-loss defect exists.
* No known workspace or manifest path escape exists.
* Physical identity works on every claimed OS-plus-filesystem matrix row, and unclaimed/ambiguous rows reject before mutation.
* Handle-relative no-replace backup, installation, and restoration primitives work on every claimed matrix row.
* Single-target and multi-target transactions share one implementation.
* Preview wording and tests are portable.
* Expected-plan commits work.
* Plan-hash format is documented.
* Protocol-v1 schemas, stable errors, warnings, and exit categories are documented and golden-tested.
* Persistent lock bootstrap/repair and absent-path equivalence work on every claimed matrix row.
* Snapshot mutation during read is detected according to the bounded retry contract.
* Transaction manifest and state formats are documented.
* Normal and Durable guarantees, commit point, transaction-private candidate ownership (including the unpublished-identity crash window), delta/snapshot state records, and partial-cleanup recovery are documented and tested.
* New-file umask goldens, metadata-loss policy, and arbitrary-byte/terminal-safe output behavior are documented and tested.
* Every resource limit is documented and its boundary tests pass.
* Pilot criteria pass.

Minimum release targets:

```text
Linux x86_64
macOS ARM64
Windows x86_64
```

These are packaging/CI architecture targets, not blanket filesystem claims; the Phase 0 support matrix defines which mounted filesystems are accepted at runtime.

---

# Deferred Phase 11 — Additional exact operations

Potential capabilities:

```text
delete
insert
swap
extract
```

Every operation must use the existing:

```text
snapshot model
conflict model
segment planner
plan hash
transaction engine
recovery engine
```

---

# Deferred Phase 12 — Tree-sitter selectors

Tree-sitter may resolve structural selections into byte ranges.

It must not print or regenerate code.

The exact engine remains authoritative.

---

# Definition of a successful `v0.1.0` operation

```bash
codesplice apply \
  --request split.json \
  --preview \
  --json
```

Output includes:

```text
workspace identity hash
plan hash version
plan SHA-256
resolved source ranges
resolved destination offsets
selected payload SHA-256 values
resulting output SHA-256 values
```

The agent then commits:

```bash
codesplice apply \
  --request split.json \
  --commit \
  --expect-plan sha256:PREVIEWED_PLAN \
  --json
```

Before creating transaction artifacts, CodeSplice must verify:

```text
recomputed plan SHA-256
==
expected plan SHA-256
```

The final report includes:

```text
protocol version
capabilities used
workspace identity hash
plan hash version
plan SHA-256
transaction ID
transaction state
source identities
destination identities or absence
source before and after digests
destination before and after digests
selected payload digests
inserted payload digests
current preserved permission mode where applicable
files changed
stable warnings
recoverability status
```

The required payload equality is:

```text
selected payload SHA-256
==
inserted payload SHA-256
```
