# CodeSplice Technical Implementation Plan

**Status:** Revised after third technical review; Phase 0 specification work required before production implementation
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
move
copy
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
* Do not promise preservation of ownership, ACLs, extended attributes, timestamps, alternate data streams, resource forks, platform flags, or hard-link relationships.
* Reject multiple requested paths that identify the same existing physical file.
* Document platform-specific behavior.

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
LineRecord
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

On supported platforms, mutation primitives must be relative to securely opened workspace or parent-directory handles whenever the operating system exposes the required semantics. A path-based fallback is permitted only for an operation whose documented platform primitive cannot be made handle-relative, and only after immediate parent and target identity revalidation. Such fallback must be named in `docs/platform-support.md`; it may not silently weaken the guarantee.

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
2. Relative to that root handle, open `.codesplice.lock` as a regular file with read/write access, close-on-exec, and no-follow/no-reparse semantics. If absent, attempt exclusive creation. Unix creation mode is `0600`; Windows uses a non-inheritable handle and the root directory’s normal ACL inheritance.
3. Reject a symlink, junction, unsupported reparse point, directory, special file, multiply linked file where link count is available, or a lock name whose `PathEquivalenceKey` is not the reserved key.
4. Acquire a non-blocking exclusive OS file lock on that opened handle. Contention returns `TRANSACTION_BUSY`; v1 does not steal locks or infer staleness from PID or time.
5. While holding the lock, revalidate the workspace-root identity and that the reserved directory entry still identifies the opened lock object.
6. If the file is empty because this invocation created it or a prior creator crashed before initialization, write the versioned lock-identity record while holding the lock. Otherwise validate the complete checksummed record.
7. The lock-identity record contains magic `CODESPLICE-LOCK\0`, format version `1`, the canonical workspace identity token, and a SHA-256 checksum over all preceding record bytes.
8. A nonempty malformed record or a record bound to another workspace identity fails with `WORKSPACE_LOCK_INVALID`; it is never silently replaced.
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
  CreateFileW relative to/opened beneath the validated root using CREATE_NEW first,
  then OPEN_EXISTING only after ERROR_FILE_EXISTS
  GENERIC_READ | GENERIC_WRITE, FILE_SHARE_READ | FILE_SHARE_WRITE,
  FILE_ATTRIBUTE_NORMAL | FILE_FLAG_OPEN_REPARSE_POINT, no delete sharing
  reject reparse tags and non-disk files; capture FileIdInfo
  LockFileEx exclusive + FAIL_IMMEDIATELY over the whole v1 lock range
```

Equivalent reviewed crate calls are acceptable only when they preserve every listed flag and failure semantic. Lock-record initialization flushes runtime buffers in Normal mode and additionally syncs the file and root directory in Durable mode.

The lock file persists after release. Normal cleanup never removes it. Preview, inspect, capabilities, protocol-version, `recover --list`, and `recover --status` do not create or acquire it. Concurrent first creators may both open the resulting file, but only the process that acquires the OS lock may initialize or validate it; all control-directory creation remains serialized.

## 7.6 Authoritative path equivalence

Every accepted operation, parent, target, reserved path, manifest path, and sort key has a filesystem-layer `PathEquivalenceKey`, whether or not the final entry exists. The key is an opaque, length-delimited sequence of:

```text
workspace identity
for each component:
  securely resolved parent-directory physical identity
  platform component-comparison key
```

The platform component key is frozen as follows:

* Linux: exact UTF-8 bytes. The supported Linux policy is case-sensitive and performs no Unicode normalization.
* macOS: query the mounted volume’s case-sensitivity behavior; use the platform filesystem comparison form, including its canonical Unicode decomposition behavior, and apply case folding only on a case-insensitive volume. If the behavior cannot be queried or represented, reject with `PATH_EQUIVALENCE_UNSUPPORTED`.
* Windows: reject Win32 device names, alternate-data-stream syntax, trailing dot/space spellings, and unsupported reparse behavior; normalize accepted UTF-8 to NFC and use the directory’s queried case-sensitivity setting, applying Windows ordinal invariant case mapping when insensitive.

The filesystem implementation must validate these rules against native same-entry probes in platform tests. It must not use locale-sensitive casing. Existing entries additionally require matching physical identity; equivalence keys do not replace physical-identity validation.

Use `PathEquivalenceKey` to:

* Reject duplicate source/destination path records that the platform interprets as one entry.
* Reject aliases of `.codesplice` and `.codesplice.lock`.
* Detect absent-target basename collisions under one physical parent.
* Group all operations targeting one new file.
* Sort input and output tables, manifest targets, and commit order; ties are errors, never spelling-based tie breaks.
* Bind every manifest parent/basename record to the same platform interpretation used during planning.

The core retains the normalized user-visible path spelling for diagnostics. No two distinct retained spellings may share one authoritative equivalence key in a valid batch, except repeated references to the same logical operation path with one identical mandatory precondition.

---

# 8. Protocol and capability model

## 8.1 Protocol envelope

Initial protocol version:

```text
protocol_version = 1
```

Protocol version and supported capabilities are separate.

A protocol-v1 implementation is not assumed to support every capability that might later be represented within protocol v1.

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
```

The same `BatchSpecification` must be reusable for preview and commit.

## 8.5 Normative protocol-v1 and CLI surface

Phase 0 must publish validating JSON Schema 2020-12 documents in:

```text
docs/schema/v1/request.schema.json
docs/schema/v1/response.schema.json
docs/schema/v1/recovery-request.schema.json
docs/schema/v1/recovery-response.schema.json
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

Direct `move` and `copy` commands are convenience constructors for exactly one protocol-v1 operation and use the same DTO conversion, planner, report, expected-plan policy, and transaction implementation. They are not independent protocols. `inspect`, `capabilities`, `protocol-version`, and `recover` have dedicated closed response schemas. Phase 0 examples must cover every command, every selector and anchor variant, existing and absent targets, preview, commit, no-op, recovery states, each stable error category, and each warning shape.

Execution options—including preview/commit, durability, output form, diff policy, and expected-plan policy—are excluded from `BatchSpecification` and from `plan_sha256`. Any option that changes resulting bytes is therefore prohibited as an execution option.

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

Every error includes stable `code`, `category`, `retryable`, `message`, and `context`. Request-validation errors additionally include an RFC 6901 JSON pointer and, when applicable, a zero-based `operation_index`. Filesystem errors include only normalized workspace-relative paths by default; absolute workspace paths, raw physical identities, OS usernames, and adjacent unrelated names are redacted unless a trusted `--diagnostic-paths` option is used. Warnings have stable identifiers and structured context. Adding a new error code within an existing category is backward compatible; changing a code’s meaning, exit category, or context field type requires a protocol-major change.

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

Output records are sorted by encoded `PathEquivalenceKey` bytes, again rejecting ties. They include every logical file whose final recipe or unchanged resolved operation is reported.

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

The following are excluded from `plan_v1`: execution options (`preview`, `commit`, durability, output/diff mode, expected-plan policy), warnings, human messages, executable version, timestamps, temporary or transaction paths, transaction ID, permission metadata, and preview truncation. No nested digest may reuse the plan domain: payload and file digests are ordinary SHA-256 of content bytes, while new-file IDs use the explicit `CODESPLICE-NEW-ID\0` domain above.

Phase 0 golden vectors must include at least absent and existing destinations, every enum variant, a no-op, multiple same-offset insertions, non-UTF-8 payload bytes, and maximum-width numeric examples. Each vector includes the semantic input, annotated complete hexadecimal byte dump, byte length, and expected lowercase `sha256:` digest. Golden vectors use synthetic identity kind `0x0000`, never live inodes or Windows file indexes.

---

# 10. Resource limits

Freeze `v0.1.0` defaults before implementation.

Initial defaults:

```text
maximum JSON request size:
  16 MiB

maximum operations per batch:
  10,000

maximum distinct operation paths:
  5,000

maximum UTF-8 relative-path length:
  4,096 bytes

maximum transaction targets:
  5,000

maximum manifest size:
  64 MiB

maximum state-file size:
  1 MiB

maximum human-readable diff:
  8 MiB

maximum JSON-embedded diff:
  8 MiB

maximum individual snapshot file:
  1 GiB

maximum total in-memory snapshot bytes:
  2 GiB
```

Requirements:

* Limits apply before uncontrolled allocation.
* Limit violations use structured errors.
* Request-provided data cannot increase the limits.
* Trusted CLI configuration may lower limits.
* Increasing limits requires an explicit trusted local option and documented risk.
* Preview truncation never changes plan or output hashes.
* Truncation is explicitly reported.

## 10.1 Diff policy for arbitrary bytes

Diff generation is reporting-only and may be disabled with `--no-diff`; disabling or truncating a diff cannot affect planning, hashing, or commit. A file is treated as binary when either side contains NUL or is not valid UTF-8. Binary reports contain lengths, SHA-256 digests, and bounded changed-span samples encoded as standard padded base64; they never place arbitrary bytes directly in JSON or terminal output.

Text diffs tokenize LF, CRLF, and lone CR as indivisible terminators using the same line index as selectors. JSON escaping is the only string escaping in JSON mode. Human mode visibly escapes control characters other than recognized line terminators.

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
docs/metadata.md
docs/security.md
docs/resource-limits.md
docs/transaction-model.md
docs/platform-support.md
docs/schema/v1/request.schema.json
docs/schema/v1/response.schema.json
docs/schema/v1/recovery-request.schema.json
docs/schema/v1/recovery-response.schema.json
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

For each file, create events with key:

```text
(initial_byte_offset, event_class, operation_index, boundary_kind)
```

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

Physical identity support is a release requirement on:

```text
Linux x86_64
macOS ARM64
Windows x86_64
```

It is not an optional capability on those targets.

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
3. Read its current ordinary permission bits.
4. Apply those current bits to the candidate.
5. Install the candidate.

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
6. Treat different before/after metadata, short/long reads relative to final length, or a changed digest precondition as an unstable attempt.
7. Re-resolve the parent entry without following and verify it still identifies the opened object and the same parent identity.
8. Close only after all captured commit-context tokens are finalized.

Retry the complete open/read/validate sequence at most two additional times, for three attempts total. A third unstable result returns `SNAPSHOT_UNSTABLE` with the normalized path, attempt count, and which indicators changed. Never combine chunks from separate attempts. This detects ordinary concurrent in-place writes; the security documentation must state that platforms lacking a generation counter cannot prove absence of a hostile write that restores all observed metadata.

## 0.13 Lock and secure-handle contract

Freeze and test the exact Section 7.5 bootstrap. All `.codesplice`, transaction-directory, candidate, backup, target, rollback, and cleanup operations use handle-relative APIs where supported. Every path-based exception must identify its OS primitive, pre/post identity validation, and residual race in `docs/platform-support.md`. `WORKSPACE_LOCK_INVALID`, `TRANSACTION_BUSY`, and `PATH_EQUIVALENCE_UNSUPPORTED` are protocol-v1 stable errors.

## 0.14 Transaction records, states, and durability

The immutable manifest is published before any candidate creation. The mutable logical state is represented by a sequence of complete atomic records. Every record contains transaction ID, manifest SHA-256, `sequence`, state tag and payload, and the prior published state record’s stored checksum (`none` for sequence `0`). The checksum is SHA-256 over every record byte from magic through payload, excluding only the final checksum field.

```text
manifest magic: CODESPLICE-MANIFEST\0
state magic:    CODESPLICE-STATE\0
format version: u32 big-endian 1
first state sequence: 0
next sequence: exactly previous + 1, without wrap
```

`manifest SHA-256` means the manifest record’s stored checksum under the same rule. `sequence`, target indices, state tags, optional tags, identity encodings, lengths, and list counts receive fixed big-endian widths and numeric discriminants in the Phase 0 byte grammar in `docs/transaction-model.md`; the grammar must include an annotated golden byte dump for every state tag before Phase 1.

State tags and payloads are:

```text
Preparing
  per-target candidate status: missing | created(candidate physical identity)
Prepared
  every candidate identity present and verified
Committing
  per-target stage: original | backed_up(backup identity) | installed(final identity)
Committed
  every final target verified; this persisted record is the logical commit point
RollingBack
  per-target rollback stage and observed identities
RolledBack
  every existing original restored and every originally absent target absent
CleaningCommitted
  cleanup progress; only all-new recovery is legal
CleaningRolledBack
  cleanup progress; only all-old recovery is legal
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

No transition may regress a target stage or sequence. Rollback from `Committing` is allowed only after filesystem classification proves all installed candidates can be removed and all backups still identify the recorded originals. `Committed` is the irrevocable logical commit point: rollback is rejected thereafter, and recovery may only verify/complete cleanup. State that names an unknown target, invalid index, impossible stage, candidate identity absent from `Prepared`, or a manifest digest/transaction ID mismatch returns `TRANSACTION_RECORD_CORRUPT`. Filesystem observations never silently repair a corrupt record; valid lag after a crash is handled by the recovery matrix.

Record publication protocol:

1. Exclusively create a bounded single-component temporary name inside the transaction directory: `manifest-<txn>.tmp` or `state-<20-digit-sequence>-<txn>.tmp`.
2. Serialize the complete bounded record in canonical binary form, append its checksum, write all bytes, and flush language/runtime buffers.
3. Apply the selected durability sync requirement below.
4. Publish with a handle-relative no-replace rename to `manifest.rec` or `state-<20-digit-sequence>.rec`.
5. Apply the selected directory-sync requirement.
6. Never overwrite a published record. On recovery, ignore only a validly bounded `.tmp` name owned by that manifest; never infer state from it.

The current state is the highest valid contiguous sequence whose hash chain begins at `0`. A gap, duplicate sequence, checksum failure in a published record, or forked prior hash is corruption. This append-and-publish protocol is the v1 state-file replacement model; no in-place state write or separate checksum file is allowed.

Durability guarantees are:

| Operation | `Normal` | `Durable` |
|---|---|---|
| Record/candidate data write | `write_all`, flush runtime buffers, close before dependent rename | `write_all`, flush, `sync_all` file before dependent rename |
| Publish manifest/state | atomic no-replace rename; no directory sync required | sync temp file, rename, then sync transaction directory |
| Create control/transaction directory | ordinary secure create | secure create, then sync its parent directory at every new level |
| Candidate ready | close and verify before `Prepared` | apply permissions, `sync_all`, close/verify, then publish and sync `Prepared` |
| Target-to-backup rename | publish next state after rename | rename, sync target parent directory, then publish and sync next state |
| Candidate-to-target rename | verify then publish next state | pre-sync candidate data/metadata, rename, sync target parent, verify, then publish/sync state |
| Cleanup unlink/transaction removal | best effort with state retained until completion | sync affected parent after each cleanup batch and after transaction removal |

`Normal` guarantees recoverability after process termination provided the operating system and filesystem preserve acknowledged cached writes and renames; it makes no power-loss or kernel-crash persistence guarantee. `Durable` orders file and directory synchronization to survive a system crash or power loss under the documented filesystem’s `fsync`/equivalent contract. Unsupported directory sync or rename semantics fail before mutation with `UNSUPPORTED_DURABILITY_PRIMITIVE`. Neither mode makes multi-target visibility atomic.

## 0.15 Candidate identity and recovery comparison

After exclusive candidate creation, capture its canonical physical identity from the retained open handle. Persist that identity in the next `Preparing` state record before closing the handle; `Prepared` is forbidden until every candidate identity is recorded. Reopen/revalidate the directory entry against that identity immediately before installation. Recovery uses the recorded identity, never digest and length alone.

For every target, classification compares:

```text
target path vs recorded original identity and digest
target path vs recorded candidate/final identity and planned digest
backup path vs recorded original identity and digest
candidate path vs recorded candidate identity and planned digest
each containing parent vs recorded parent identity and PathEquivalenceKey
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
| either | any different identity/digest/type | any unexpected entry | any different identity/digest/type | explicit conflict; mutate nothing |

After `Committed`, rows that already name the recorded candidate/final object may only advance cleanup; a missing or mismatching final target is an explicit external-modification conflict, never grounds for rollback. Recovery applies this matrix to all targets before the first recovery mutation, then revalidates the affected row immediately before each handle-relative operation.

## 0.16 New-file permission policy

On Unix, every candidate is created exclusively at mode `0600`, and the implementation immediately verifies/reapplies `0600`. Before worker threads start, the CLI captures and restores the process umask. A new target’s final ordinary mode is exactly `0666 & captured_umask`; apply it to the candidate immediately before installation. The candidate is never broader than `0600` during preparation and never broader than its final mode after that chmod. Existing targets continue to receive the current mode read from the validated backup. Protocol v1 has no requested-mode field or CLI mode override.

On Windows, candidates use non-inheritable handles and the target parent’s normal inherited ACL policy. Read-only attribute behavior is documented separately and no POSIX-mode equivalence is promised. Candidate creation must not deliberately grant access beyond the final inherited policy. Platform tests record the effective ACL/attribute behavior.

## 0.17 Deterministic crash failpoints

Test builds expose named failpoints after every published record and before/after every candidate create, permission application, backup rename, install rename, verification, state transition, cleanup unlink, and directory removal. Production builds do not enable failpoints.

Scenario tests launch CodeSplice as a child process with one failpoint selected through a test-only inherited channel, wait for a “reached” acknowledgement, forcibly terminate the child without unwinding, then run inspection and recovery in a fresh process. In-memory injected errors alone do not satisfy a crash checkpoint. Failpoint names are stable and include transaction sequence plus target commit index in test reports.

## 0.18 Required fixtures

Create at least 60 normative fixtures, including:

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
53. Absent Unicode-normalization-equivalent targets.
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

## Phase 0 checkpoint

Pass only when:

* Crate ownership is cycle-free.
* Workspace-root behavior is frozen.
* `.codesplice` behavior is frozen.
* `.codesplice.lock` bootstrap, identity binding, persistence, and lifetime are frozen.
* Path equivalence for existing and absent entries is frozen on all release platforms.
* Plan-hash encoding is frozen.
* Golden plan vectors include annotated byte dumps.
* Edit composition and every line-anchor offset are frozen.
* Single-target and multi-target commits share one transaction model.
* The complete transaction state machine and both durability protocols are frozen.
* Candidate and final identities are present in recovery comparisons.
* Preview wording does not promise stable access time.
* Permission concurrency behavior is frozen.
* New-file permissions are frozen.
* Backup no-replace behavior is specified.
* Manifest target records are fully specified.
* Orphan handling is frozen.
* Protocol-v1 schemas validate all normative examples and reject duplicate/unknown keys.
* Exit categories, error context, arbitrary-byte diff behavior, and failpoint mechanics are frozen.
* At least 60 fixtures exist.
* No production editing engine exists.

### Phase 0 evidence

Phase 0 does not require or permit production filesystem editing or transaction behavior. Its evidence is divided into:

1. **Normative examples and state-transition tables:** required for every item below and checked for internal consistency.
2. **Optional non-production executable specification tests:** pure models, schema-validation tests, golden encoders, and in-memory/test-double state machines are allowed under `codesplice-test-support` or documentation tooling. They may not be linked into the production CLI/crates as an editing or transaction engine and may not mutate real workspace targets.
3. **Production behavioral demonstrations:** explicitly deferred to the phase named below and not part of the Phase 0 pass/fail decision.

Present normative examples for:

1. A same-file no-op move.
2. A cross-file move with expected plan.
3. A permission change between preview and commit.
4. A workspace-root replacement conflict.
5. A transaction directory created before candidate generation.
6. A target backup collision rejected through no-replace rename.

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
* Resource limits are validated before large allocation.
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
INVALID_WORKSPACE_ROOT
WORKSPACE_IDENTITY_CHANGED
RESERVED_CONTROL_PATH
CONTROL_DIRECTORY_INVALID
TRANSACTION_BUSY
RESOURCE_LIMIT_EXCEEDED
UNSUPPORTED_COMMIT_PRIMITIVE
WORKSPACE_LOCK_INVALID
PATH_EQUIVALENCE_UNSUPPORTED
SNAPSHOT_UNSTABLE
LINE_ANCHOR_OUT_OF_RANGE
PLAN_ENCODING_LIMIT
TRANSACTION_RECORD_CORRUPT
UNSUPPORTED_DURABILITY_PRIMITIVE
RESERVED_NAME_COLLISION
CANNOT_COMPLETE_PREPARING
```

Errors and warnings require structured context.

## Phase 2 checkpoint

Pass only when:

* DTOs convert to core specifications.
* Invalid workspace-root values fail.
* Invalid domain states cannot reach planning.
* Expected-plan policy is parsed separately.
* Resource limits are applied during parsing.
* No filesystem access exists in protocol conversion.
* Stable exit categories, retryability, JSON pointers, operation indices, redaction, warnings, and JSON stdout/stderr rules pass golden tests.

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
* Enforce snapshot memory limits.
* Detect path aliases.
* Detect precondition conflicts.
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
* Linux, macOS, and Windows physical identity support works.
* Junction and reparse-point behavior is tested on Windows.
* Preview acquisition creates no control directory.
* Memory limits are enforced.
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
* Mutation during read followed by retry and eventual structured failure.
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

## Phase 4 checkpoint

Pass only when:

* Segment planning passes all fixtures.
* Complete output files are not retained.
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

1. Acquire or bootstrap the exact persistent `.codesplice.lock` from Section 7.5.
2. Revalidate workspace identity.
3. Recompute the complete stable snapshot and plan while holding the lock.
4. Verify `--expect-plan` again when provided and verify the plan changes at least one file.
5. Securely validate or create `.codesplice` and `.codesplice/transactions` relative to root/control handles.
6. Generate a validated 128-bit random transaction ID encoded as 32 lowercase hexadecimal digits.
7. Exclusively create the transaction directory.
8. Determine and collision-check all bounded candidate and backup basenames.
9. Publish the complete versioned immutable manifest using Section 0.14.
10. Publish state sequence `0` as `Preparing` with all candidates `missing`.
11. Only then create candidate files, persisting each physical identity in a new `Preparing` state.

This prevents adjacent candidate files from existing without a durable transaction record.

## 6.2 Bounded reserved names

Do not append an unbounded suffix to the original filename.

Use bounded basenames such as:

```text
.cs-<short-transaction-id>-<target-hash>-c
.cs-<short-transaction-id>-<target-hash>-b
```

Requirements:

* The short transaction component is the first 16 hex digits of the validated transaction ID.
* The target hash is the first 20 bytes (40 lowercase hex digits) of `SHA256("CODESPLICE-TARGET-NAME\0" || encoded PathEquivalenceKey)`.
* Exact v1 basenames are `.cs-<16hex>-<40hex>-c` and `.cs-<16hex>-<40hex>-b`, each 63 ASCII bytes.
* Single path component.
* No user-controlled path separators.
* Target hash derived from normalized target path.
* Candidate creation uses exclusive create.
* Backup installation uses no-replace rename.
* All generated candidate and backup names must be distinct within the transaction. A truncation collision returns `RESERVED_NAME_COLLISION` before manifest publication; v1 does not choose an alternate name.
* A collision with any existing entry, including stale transaction artifacts or unrelated user data, causes safe transaction failure. No colliding entry is removed or replaced.
* The manifest is immutable and is never rewritten because of a name collision.
* The 63-byte length is below the minimum 255-byte component limit required of supported target filesystems.

## 6.3 No-replace backup primitive

Do not reserve a backup through check-then-create or placeholder removal.

Implement a platform abstraction:

```rust
pub trait RenameNoReplace {
    fn rename_no_replace(
        &self,
        source: &Path,
        destination: &Path,
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
  move/rename primitive that fails when destination exists
```

There must be no fallback that performs:

```text
check destination absent
then ordinary replacing rename
```

If a safe primitive is unavailable, fail with:

```text
UNSUPPORTED_COMMIT_PRIMITIVE
```

## 6.4 Candidate preparation

After the manifest is durable enough for the selected mode:

1. Exclusively create the candidate.
2. Capture its physical identity from the retained handle and publish a `Preparing` state containing that identity.
3. Apply restrictive candidate permissions from Section 0.16.
4. Stream plan segments into it and hash while writing.
5. Verify digest, length, path identity, and parent identity.
6. Perform the selected Normal/Durable flush and sync protocol.
7. After all candidates are ready, publish `Prepared` containing every candidate identity.

## 6.5 Existing-target commit

1. Revalidate root and parent identity.
2. Revalidate target path type and identity.
3. Move target to backup with no-replace semantics.
4. Hash and identify the object actually moved.
5. On mismatch, restore it and report conflict.
6. Read its current ordinary permission bits.
7. Apply those bits to the candidate.
8. Revalidate the candidate against its recorded physical identity and install it with handle-relative safe platform semantics where available.
9. Verify final type, identity, length, and digest.
10. Record commit progress.
11. Mark transaction committed.
12. Clean backup and transaction artifacts.

## 6.6 New-target commit

1. Revalidate root and parent identity.
2. Revalidate complete absence.
3. Apply and verify the Section 0.16 new-file final mode, revalidate candidate identity, and install without replacement.
4. Verify final type, identity, length, and digest.
5. Record progress.
6. Mark committed.
7. Clean transaction artifacts.

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
candidate basename
candidate digest
candidate length
backup basename
expected final type
expected final digest
change kind
```

Candidate physical identity is necessarily unknown when the immutable manifest is published. It is stored in the checksummed `Preparing`/`Prepared` state chain immediately after exclusive creation, as required by Section 0.15. Backup identity and installed-final identity are similarly stored in progress state records. Manifest digest/length never substitutes for these identities.

For a new target:

```text
original existence state = absent
```

Candidate and backup locations are represented as:

```text
validated parent-directory reference
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

Because the manifest is written before candidate creation, adjacent candidates should always have a transaction record.

Potential orphan states:

### Transaction directory without a manifest

Safe behavior:

* List as `incomplete_transaction_record`.
* Do not inspect or delete unrelated adjacent files.
* Allow explicit cleanup only after verifying the directory is inside the secure control directory and contains no valid manifest references.

### Valid manifest with missing candidates

Fresh-process `--complete` is rejected with `CANNOT_COMPLETE_PREPARING` while state is `Preparing`, because the manifest does not retain payload bytes and a partially written candidate is not trusted. Rollback leaves candidates recorded as `missing` absent and deletes only candidate entries whose recorded identity still matches. A recorded candidate that is absent in `Preparing` is safe for rollback. In `Prepared` or later, an absent candidate is acceptable only when the state/location matrix proves that exact recorded identity was renamed to the target during an interrupted install; otherwise it is an external conflict. Recovery never synthesizes replacement bytes from unrelated current files.

### Candidate-like filename without a manifest reference

Never delete it automatically merely because its name resembles a CodeSplice reserved name.

## Phase 6 checkpoint

Pass only when:

* Single-target commits use the complete journal.
* A manifest exists before candidate creation.
* Crashes between backup and install are recoverable.
* No-replace backup movement is used.
* Backup names are bounded.
* Expected-plan mismatch is rejected before transaction creation.
* Current backup permissions are applied to the candidate.
* Single-target recovery commands work.
* Read-only status follows stale-observation rules.
* Every failpoint ends recoverably or in explicit conflict.
* `.codesplice.lock` bootstrap, persistence, malformed-state behavior, workspace binding, contention, and lifetime pass on every release platform.
* State transitions, monotonic/hash-chained sequences, atomic publication, checksum corruption, and commit-point behavior pass.
* Candidate replacement by an equal-byte different-identity object is rejected.
* Normal mode passes fresh-process termination recovery tests, and Durable mode passes ordered file/directory-sync trace tests on every supported filesystem.
* New-target modes/ACL behavior match Section 0.16 and candidates are never intentionally broader.

### Demonstration

Inject crashes:

1. After transaction directory creation.
2. After manifest write.
3. After candidate creation.
4. After target-to-backup move.
5. After candidate installation.
6. Before cleanup.

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
3. Recompute plan.
4. Verify expected plan.
5. Create transaction record.
6. Write all target entries to the manifest.
7. Persist initial state.
8. Create all candidates.
9. Verify all candidate digests.
10. Mark `Prepared`.
11. Begin no target replacement before every candidate is ready.

## Commit ordering

Targets use deterministic encoded-`PathEquivalenceKey` order. Equivalent-key ties are rejected during planning.

The manifest records `commit_index`.

For each target:

1. Revalidate parent.
2. Revalidate target or absence.
3. Move existing target to backup with no-replace semantics.
4. Verify moved backup.
5. Capture current permission bits.
6. Apply permissions to candidate.
7. Install candidate.
8. Verify final output.
9. Persist progress.

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
torn manifest
torn state
state sequence regression
candidate without manifest reference
transaction directory without manifest
permission change after preview
external content change during commit
external content change during recovery
root identity change during recovery
parent identity change
concurrent commit
concurrent rollback
concurrent read-only status
resource-limit violations
concurrent first lock bootstrap
malformed or workspace-mismatched persistent lock record
absent case and Unicode-equivalent target aliases
snapshot changes during open-handle read
candidate replacement with identical bytes and different identity
committed state with partial cleanup
new-file umask and restrictive candidate mode
binary diff escaping and computation-budget exhaustion
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

## Phase 8 checkpoint

Pass only when:

* Release-platform identity support is reliable.
* No-replace primitives are tested.
* Fuzzing produces no crashes.
* Resource limits are enforced.
* Recovery failpoints pass.
* Permission concurrency behavior passes.
* Workspace and control-directory security pass.
* Benchmark results are recorded.

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

Run at least 12 scenarios:

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
* At least 12 scenarios complete without manual repair.
* Every failure is categorized.

**Stop after the checkpoint.**

---

# Phase 10 — Release `v0.1.0`

Release only when:

* Phases 0–9 pass.
* No unresolved data-loss defect exists.
* No known workspace or manifest path escape exists.
* Physical identity works on minimum targets.
* No-replace backup primitives work on minimum targets.
* Single-target and multi-target transactions share one implementation.
* Preview wording and tests are portable.
* Expected-plan commits work.
* Plan-hash format is documented.
* Protocol-v1 schemas, stable errors, warnings, and exit categories are documented and golden-tested.
* Persistent lock bootstrap and absent-path equivalence work on every minimum target.
* Snapshot mutation during read is detected according to the bounded retry contract.
* Transaction manifest and state formats are documented.
* Normal and Durable guarantees, commit point, candidate identity, and partial-cleanup recovery are documented and tested.
* New-file permissions and arbitrary-byte diff behavior are documented and tested.
* Resource limits are documented.
* Pilot criteria pass.

Minimum release targets:

```text
Linux x86_64
macOS ARM64
Windows x86_64
```

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
