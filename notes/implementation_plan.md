# CodeSplice Technical Implementation Plan

**Status:** Revised after second technical review
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
    pub files: BTreeMap<WorkspacePath, FileSnapshot>,
    pub absent_paths: BTreeSet<WorkspacePath>,
}

pub struct FileSnapshot {
    pub path: WorkspacePath,
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

Freeze a versioned binary encoding.

Plan hash version 1 must define:

```text
magic:
  fixed ASCII bytes "CODESPLICE-PLAN\0"

plan-hash version:
  unsigned 32-bit big-endian integer

integer encoding:
  unsigned integers encoded as fixed-width big-endian values
  unless a field explicitly uses a bounded one-byte enum tag

strings:
  UTF-8
  unsigned 32-bit big-endian byte length
  followed by exact bytes

paths:
  normalized workspace-relative UTF-8
  "/" as separator
  no "." or ".." components
  no trailing separator
  case is not rewritten by the core

digests:
  raw 32-byte SHA-256 values

identities:
  identity-kind tag
  length-prefixed canonical identity bytes

absent files:
  explicit absent-state tag

operations:
  retained in request order

input file table:
  sorted by normalized path

output file table:
  sorted by normalized path

segments:
  emitted in final output order

warnings:
  excluded

human messages:
  excluded

tool version:
  excluded

timestamps:
  excluded

temporary paths:
  excluded

transaction ID:
  excluded
```

Golden plan-hash tests must use synthetic identity tokens rather than live inodes or Windows file indexes.

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
docs/adr/0001-exact-content-model.md
docs/adr/0002-coordinate-semantics.md
docs/adr/0003-workspace-root-model.md
docs/adr/0004-path-identity-policy.md
docs/adr/0005-conflict-semantics.md
docs/adr/0006-new-file-policy.md
docs/adr/0007-plan-hash-format.md
docs/adr/0008-transaction-guarantees.md
docs/adr/0009-permission-preservation.md
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

## 0.5 Cross-operation conflicts

All sources and anchors resolve before composition.

Rules:

* Move source ranges may not overlap.
* Copy ranges may overlap other copy ranges.
* Copy ranges may overlap moved source ranges.
* Multiple insertions at one offset retain request order.
* An insertion at the start boundary of a deleted range is valid.
* An insertion at the end boundary of a deleted range is valid.
* An insertion strictly inside any deleted range is invalid.
* Conflicting preconditions for one physical file are invalid.
* Distinct paths resolving to one physical file are invalid.

The specification must define a complete total order for deletion boundaries and insertions.

## 0.6 Workspace and path rules

Freeze all workspace-root and `.codesplice` rules from Sections 7 and 8.

Operation paths:

* Are workspace-relative.
* Cannot be absolute.
* Cannot contain `..`.
* Cannot target `.codesplice`.
* Cannot traverse symlinks, junctions, or unsupported reparse points.
* Cannot refer to directories or special files.
* Cannot escape the root.
* Cannot rely on parent-directory creation.

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

## 0.12 Required fixtures

Create at least 40 fixtures, including:

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

## Phase 0 checkpoint

Pass only when:

* Crate ownership is cycle-free.
* Workspace-root behavior is frozen.
* `.codesplice` behavior is frozen.
* Plan-hash encoding is frozen.
* Single-target and multi-target commits share one transaction model.
* Preview wording does not promise stable access time.
* Permission concurrency behavior is frozen.
* Backup no-replace behavior is specified.
* Manifest target records are fully specified.
* Orphan handling is frozen.
* At least 40 fixtures exist.
* No production editing engine exists.

### Demonstration

Present:

1. A same-file no-op move.
2. A cross-file move with expected plan.
3. A permission change between preview and commit.
4. A workspace-root replacement conflict.
5. A transaction directory created before candidate generation.
6. A target backup collision rejected through no-replace rename.

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

Define the initial `move` and `copy` protocol.

## Required protocol behavior

* `workspace_root` must equal `"."`.
* Unknown operations fail explicitly.
* Unknown selectors fail explicitly.
* Unknown anchors fail explicitly.
* Unknown semantic fields fail explicitly.
* Source preconditions must be SHA-256.
* Destination preconditions may be SHA-256 or `must_not_exist`.
* Resource limits are validated before large allocation.

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

### Demonstration

Deserialize:

1. Valid move.
2. Valid new-file copy.
3. Invalid workspace root.
4. Unknown selector.
5. Oversized request.
6. Commit options with expected plan.

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
* Read each physical file once.
* Convert platform identity into core identity tokens.
* Populate core `WorkspaceSnapshot`.
* Populate filesystem `CommitContext`.
* Enforce snapshot memory limits.
* Detect path aliases.
* Detect precondition conflicts.
* Represent absent destinations explicitly.

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
* No filesystem writes occur.

### Demonstration

Show:

* Valid snapshot acquisition.
* Hard-link alias rejection.
* Workspace-root symlink rejection.
* `.codesplice` symlink rejection.
* Stale digest rejection.
* Snapshot limit rejection.

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
        operation_index: usize,
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
* No filesystem writes occur.

### Demonstration

Show one plan containing:

* A real move.
* A copy.
* A no-op move.
* A new destination.
* Its canonical plan hash.

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

While holding the mutation lock:

1. Revalidate workspace identity.
2. Recompute the snapshot and plan.
3. Verify `--expect-plan` when provided.
4. Verify the plan changes at least one file.
5. Securely create `.codesplice` if absent.
6. Securely create `.codesplice/transactions`.
7. Generate a validated transaction ID.
8. Exclusively create the transaction directory.
9. Determine bounded candidate and backup basenames.
10. Write a versioned immutable manifest before candidate creation.
11. Write manifest checksum.
12. Write initial state `Preparing`.
13. Write state checksum.
14. Only then create candidate files.

This prevents adjacent candidate files from existing without a durable transaction record.

## 6.2 Bounded reserved names

Do not append an unbounded suffix to the original filename.

Use bounded basenames such as:

```text
.cs-<short-transaction-id>-<target-hash>-c
.cs-<short-transaction-id>-<target-hash>-b
```

Requirements:

* Fixed maximum length.
* Single path component.
* No user-controlled path separators.
* Target hash derived from normalized target path.
* Candidate creation uses exclusive create.
* Backup installation uses no-replace rename.
* Reserved-name collision causes safe transaction failure.

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
2. Stream plan segments into it.
3. Hash while writing.
4. Verify digest and length.
5. Flush.
6. Synchronize in durable mode.
7. Update transaction state to `Prepared`.

## 6.5 Existing-target commit

1. Revalidate root and parent identity.
2. Revalidate target path type and identity.
3. Move target to backup with no-replace semantics.
4. Hash and identify the object actually moved.
5. On mismatch, restore it and report conflict.
6. Read its current ordinary permission bits.
7. Apply those bits to the candidate.
8. Install candidate with safe platform semantics.
9. Verify final type, identity, length, and digest.
10. Record commit progress.
11. Mark transaction committed.
12. Clean backup and transaction artifacts.

## 6.6 New-target commit

1. Revalidate root and parent identity.
2. Revalidate complete absence.
3. Install candidate without replacement.
4. Verify final type, identity, length, and digest.
5. Record progress.
6. Mark committed.
7. Clean transaction artifacts.

## 6.7 Manifest target record

Every target entry includes:

```text
deterministic commit index
normalized target path
normalized target parent path
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

## 6.9 Orphan rules

Because the manifest is written before candidate creation, adjacent candidates should always have a transaction record.

Potential orphan states:

### Transaction directory without a manifest

Safe behavior:

* List as `incomplete_transaction_record`.
* Do not inspect or delete unrelated adjacent files.
* Allow explicit cleanup only after verifying the directory is inside the secure control directory and contains no valid manifest references.

### Valid manifest with missing candidates

Recovery uses manifest state and actual filesystem classification.

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

### Demonstration

Inject crashes:

1. After transaction directory creation.
2. After manifest write.
3. After candidate creation.
4. After target-to-backup move.
5. After candidate installation.
6. Before cleanup.

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

Targets use deterministic normalized-path order.

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
* Transaction manifest and state formats are documented.
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
