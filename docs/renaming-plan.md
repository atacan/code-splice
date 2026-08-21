# srcmv → srcmv renaming plan

Status: proposed

This document defines the complete breaking rename of the srcmv product to
srcmv. It covers the main repository, Rust workspace, CLI, persistent state,
protocol metadata, schemas, agent skill, release automation, and Homebrew tap.

The rename intentionally removes compatibility aliases and migration support.
The first srcmv release is a new product identity, even though it is derived
from the current srcmv implementation.

srcmv has no users other than its maintainer. There are no external consumers
of the CLI, schemas, protocol, or Homebrew formula. This is what makes a hard
cutover acceptable, and it is why stale old-identity state can simply be
ignored rather than detected or migrated. The README and the first srcmv
release notes must still name srcmv as the former product identity,
because the historical GitHub releases under atacan/code-splice keep that name
forever.

## 1. Contract and decisions

### 1.1 Canonical names

Use these replacements for current source, automation, documentation, and
packaging:

| Existing identity | New identity |
| --- | --- |
| code-splice | srcmv |
| srcmv | srcmv |
| srcmv | srcmv |
| SRCMV | SRCMV |
| atacan/code-splice | atacan/srcmv |
| srcmv-* package/path prefix | srcmv-* |
| srcmv_* Rust crate prefix | srcmv_* |
| .srcmv | .srcmv |
| srcmv/config.toml | srcmv/config.toml |
| srcmv executable | srcmv executable |
| skills/srcmv | skills/srcmv |

The public product spelling is lowercase srcmv, including prose and command
examples. Rust package and module naming follows Cargo and Rust conventions:
srcmv-core and srcmv_core.

### 1.2 No compatibility layer

The new implementation must not:

- install or recognize a srcmv executable alias;
- accept old environment variable names as fallbacks;
- read SRCMV_CONFIG or srcmv/config.toml;
- read, migrate, or write .srcmv transaction state;
- accept old journal magic, hash domains, schema identifiers, or dispatch event
  names;
- retain old Cargo package or dependency aliases; or
- leave a second srcmv Homebrew formula as a supported installation path.

The old GitHub releases and their historical asset names may remain unchanged.
They are immutable historical artifacts, not current distribution surfaces.

### 1.3 Versioning

The current workspace is 0.3.0; that version and its release tag must not be
reused. The first renamed release is **0.4.0**. Create matching release notes;
the release helper must publish the new `v0.4.0` tag and must never move or
overwrite an existing tag.

Because the JSON grammar is unchanged, retain `protocol_version: 1` and
`selection_protocol_version: 1`. Document that srcmv is nevertheless a new,
incompatible namespace and does not accept old clients. Do not bump numeric
protocol versions as part of this rename; apply the new textual/hash domains
below instead.

### 1.4 Schema URL policy

Use the schema files committed in this repository as the canonical JSON Schema
namespace. Set every `$id` and absolute `$ref` under:

~~~text
https://raw.githubusercontent.com/atacan/srcmv/main/docs/schema/
~~~

For example, the v1 request schema ID is
`https://raw.githubusercontent.com/atacan/srcmv/main/docs/schema/v1/request.schema.json`.
The previous `https://srcmv.dev/schema/...` identifiers were never owned,
registered, or hosted; they were inert placeholders that no consumer could ever
resolve, so replacing them outright cannot break anyone and requires no
redirect or compatibility handling. When rewriting identifiers, replace the
whole `https://srcmv.dev/schema/` prefix with the base URL above so the
versioned path segments are preserved exactly once. These are direct GitHub
raw-content URLs, not a separately owned project domain; no DNS, HTTPS
hosting, or deployment workstream is required.

The schema directories are versioned contracts. Do not change the content of a
published schema incompatibly in place: add a new versioned schema path when
needed. Because the old `srcmv.dev` identifiers never resolved anywhere,
any URL that dereferences successfully is already using the new namespace, so
dereferencing is itself an identity check. Add a CI check that dereferences
every absolute `$id` and `$ref`, compares the response byte-for-byte with its
tracked schema file, and fails on any drift; this guards the first real
publication of the schemas. Run it after the source repository has been
renamed. Schema tests and validators must keep resolving references from the
tracked files during Phases 1–6, never over the network, so verification stays
green before the rename lands.

### 1.5 Historical documentation policy

Current instructions, examples, automation documentation, and release procedures
must use srcmv. Historical release notes under docs/release-v*.md may retain
the old product and asset names when needed to accurately describe releases
that were actually published.

Preserve historical release-note wording and exclude only that documented
historical set from the current-identity audit. Do not rewrite the release notes
in a way that makes their published asset names inaccurate.

#### Audit allowlist

Record the final allowlist as tracked, machine-readable path-and-reason entries
in `docs/renaming-allowlist.txt`. Before the final audit, it must contain only:

- `docs/renaming-plan.md` for the explicit before/after mappings in this plan;
- `docs/release-v*.md` for preserved historical release wording and for the
  0.4.0 release notes' required references to the former product name;
- `README.md` for exactly one formerly-known-as mention of srcmv; and
- the exact source file and test fixture that implement legacy-artifact
  rejection, if Section 1.6 requires an old literal.

The audit command must parse this file and fail for every match outside it. An
allowlist entry must name a path, matching literal or regular expression, and a
short reason; broad directory entries are forbidden.

### 1.6 Old persistent state

There is no migration path from .srcmv to .srcmv. A user upgrading in the
middle of a transaction cannot recover that transaction with srcmv.

Old user-level configuration follows the same hard-cut policy without a guard:
srcmv resolves configuration only from `SRCMV_CONFIG` and
`srcmv/config.toml` under the platform configuration directory. A leftover
user-level `srcmv/config.toml` from the old tool is ignored; it is not
read, guarded, migrated, or deleted, because configuration carries no
transaction-safety risk. The release notes must state the new configuration
locations.

The release notes and documentation must require users to complete or clean up
all old transactions before upgrading. Before an operation opens, creates,
recovers, or mutates workspace control state, srcmv must inspect only the
workspace root for a first component equal to `.srcmv`, ignoring ASCII
case. If present, it must fail with a documented `srcmv:` error directing the
user to finish or clean up with the old tool; it must not enumerate, parse,
lock, migrate, or mutate that tree. This legacy-artifact rejection is a safety
check, not a compatibility alias or migration layer. Its exact source literal
and test fixture must be in the audit allowlist.

Add integration coverage that creates a legacy control tree with a sentinel
record and restrictive permissions, invokes each control-state operation, and
asserts the documented rejection plus unchanged sentinel contents and metadata.

## 2. Rename inventory

The current repository contains old identities in all of these surfaces:

- GitHub repository metadata and the local Git remote;
- workspace members, package names, dependency keys, paths, and Rust imports;
- the CLI binary, Clap command name, version output, errors, and test harness
  environment variables;
- .srcmv control paths, configuration paths, environment variables, lock
  handling, recovery, and path validation;
- journal magic, physical-identity hash domains, plan-hash domains, and golden
  bytes;
- JSON Schema URLs, $ref values, titles, and error-registry annotations;
- examples, fixtures, LSP fake-server names, transcripts, and scripts;
- the skills/srcmv directory, metadata, prompt, and references;
- the CI workflow as well as the GitHub release workflow, release scripts,
  archive roots, asset names, and release titles;
- recorded artifacts such as docs/performance-baseline.json;
- Homebrew formula, updater, workflow, dispatch payload, branch names, and
  README instructions; and
- GitHub repository settings metadata and third-party services keyed to the
  repository name, such as the DeepWiki badge.

The rename must cover both text and filesystem names. A text-only replacement
will not update Cargo package paths, generated CARGO_BIN_EXE_* names, or
Homebrew formula discovery.

## 3. Execution phases

### Human/agent handoff checkpoints

The agent may make local repository changes, regenerate artifacts, and run the
verification commands in Phases 0–6 without stopping for confirmation. It must
not make an external or irreversible change listed below until the user has
completed the manual action or given an explicit approval for that exact gate.

| Gate | Agent stops after | User action or approval | Agent resumes with |
| --- | --- | --- | --- |
| A — cutover preflight | recording the baseline and checking available local state | Confirm that real workspaces have no unfinished srcmv transactions, choose a release window, and confirm that no old-identity release or tap automation is active. | Phase 1 local implementation. |
| B — implementation review | completing Phases 1–6 and reporting the complete diff plus verification results | Review the rename change and approve pushing the source and workflow-only tap branches; approve merging the workflow-only tap change after its external review. | Push the review branches; address review feedback. |
| C — repository rename | source and tap workflow-only changes are reviewed and ready to merge | Manually rename the GitHub repository to `atacan/srcmv` in GitHub, then confirm completion. This action may instead be delegated only through explicit approval to perform that exact repository rename. | Update local remotes, push/merge the source branch, and verify the new raw schema URLs. |
| D — public source release | the merged `atacan/srcmv` main branch passes release verification for `0.4.0` | Explicitly approve creating and pushing annotated tag `v0.4.0` and publishing the public GitHub release, or perform that publication manually. | Obtain and independently verify the published archives and `SHA256SUMS`. |
| E — Homebrew publication | the formula-only tap change has verified release checksums and passes local checks | Review and approve merging `Formula/srcmv.rb`; explicitly approve the recovery workflow run if it is required. | Run the recovery workflow, then complete Homebrew install/test verification. |
| F — completion | all automated verification and release evidence are collected | Review the evidence and confirm the cutover is complete. Local checkout-directory renaming remains optional. | Record final status; no further publication actions. |

The agent must report a failed precondition or verification result immediately;
it must not work around a failed gate by changing a public release, moving a
tag, guessing a checksum, or silently broadening the rename scope.

### Phase 0: prepare and freeze the cutover

1. Create a dedicated branch in the main repository and separate branches in
   the Homebrew tap. Do not discard unrelated working-tree changes.
2. Record the baseline commit, current release tags and GitHub releases, current
   Homebrew formula version, and the selected `0.4.0` release version. Check
   every package registry used by this project as well; do not proceed if 0.4.0
   was published under the old identity on any public distribution surface.
3. Confirm that atacan/srcmv is available for the GitHub rename and that the
   srcmv command and formula names are acceptable. Confirm that `srcmv` does
   not collide with a homebrew-core formula name. Decide whether any srcmv_*
   crate will ever publish to crates.io; if so, verify and reserve those
   names first. No package is currently published to crates.io, and this plan
   adds none.
4. Confirm that the final repository name is `atacan/srcmv` and that the
   GitHub raw-content schema URL policy in Section 1.4 is acceptable.
5. Ensure no real workspace has an unfinished transaction before testing the
   new implementation. Do not use the rename as a recovery or cleanup tool.
6. Stop or account for any release, Homebrew, or automation runs that could
   publish using the old identity during the cutover.
7. Add or update the new release notes before the release tag is created.
   State clearly that this is a breaking rename with no migration support;
   name srcmv as the former product identity; list the new persistent
   state and configuration locations (.srcmv, srcmv/config.toml,
   SRCMV_CONFIG); and give the Homebrew switch commands (uninstall the old
   formula, then install atacan/tap/srcmv).

### Phase 1: rename tracked filesystem paths

Use git mv so Git can preserve rename history.

In the main repository, rename:

~~~text
crates/srcmv-cli          → crates/srcmv-cli
crates/srcmv-core         → crates/srcmv-core
crates/srcmv-fs           → crates/srcmv-fs
crates/srcmv-lsp          → crates/srcmv-lsp
crates/srcmv-protocol     → crates/srcmv-protocol
crates/srcmv-test-support → crates/srcmv-test-support
crates/srcmv-test-support/src/bin/srcmv-fake-lsp.rs
  → crates/srcmv-test-support/src/bin/srcmv-fake-lsp.rs
skills/srcmv              → skills/srcmv
~~~

Also rename any remaining tracked file or directory whose name contains an old
identity, including nested fuzz paths and agent-skill metadata. Do not edit
.git logs, reflogs, or historical Git objects.

In the Homebrew tap, rename:

~~~text
Formula/srcmv.rb
  → Formula/srcmv.rb
scripts/update-srcmv-formula.rb
  → scripts/update-srcmv-formula.rb
.github/workflows/update-srcmv.yml
  → .github/workflows/update-srcmv.yml
~~~

### Phase 2: update the Rust workspace identity

Update the root Cargo.toml:

- workspace member paths to crates/srcmv-*;
- workspace repository to https://github.com/atacan/srcmv;
- the selected new release version; and
- workspace-level descriptions or metadata that mention srcmv.

Update each package manifest:

| New package | Required changes |
| --- | --- |
| srcmv-cli | package name, binary name srcmv, descriptions, dependencies, dev-dependencies, and test target references |
| srcmv-core | package name, description, and all consumers |
| srcmv-fs | package name, description, dependency path/key, and all consumers |
| srcmv-lsp | package name, description, dependency path/key, and all consumers |
| srcmv-protocol | package name, description, dependency path/key, and all consumers |
| srcmv-test-support | package name, dependency references, and fake-LSP binary name |

Replace all Rust imports and fully qualified paths:

~~~text
srcmv_core         → srcmv_core
srcmv_fs           → srcmv_fs
srcmv_lsp          → srcmv_lsp
srcmv_protocol     → srcmv_protocol
srcmv_cli          → srcmv_cli
srcmv_test_support → srcmv_test_support
~~~

Update the nested fuzz manifest and lockfile under crates/srcmv-lsp/fuzz.
Regenerate, do not hand-edit, both the root Cargo.lock and nested fuzz
Cargo.lock, then inspect the resulting diff for stale package names or paths.

Validate this phase before continuing:

~~~bash
cargo metadata --locked --no-deps --format-version 1
cargo check --locked --workspace --all-targets --all-features
~~~

### Phase 3: update CLI identity

In the CLI manifest and implementation:

- set [[bin]].name to srcmv;
- change the Clap command identity to srcmv;
- rename srcmv_cli::run() to srcmv_cli::run();
- update CARGO_BIN_EXE_srcmv to CARGO_BIN_EXE_srcmv;
- update srcmv version output and all srcmv: error prefixes;
- update help text, usage text, command dispatch, and examples; and
- update every srcmv --version, cargo -p srcmv-*, and cargo run
  -p srcmv-* invocation.

The test-support binary must be exposed as srcmv-fake-lsp. Update its
transcripts, server metadata, fixture documentation, and diagnostics as well
as its filename.

### Phase 4: update persistent state and protocol identifiers

Apply the new names consistently to implementation, tests, fixtures, and
documentation.

#### Workspace control and configuration

~~~text
.srcmv/               → .srcmv/
srcmv/config.toml     → srcmv/config.toml
SRCMV_CONFIG          → SRCMV_CONFIG
SRCMV_TEST_*          → SRCMV_TEST_*
SRCMV_PILOT_*         → SRCMV_PILOT_*
SRCMV_BIN             → SRCMV_BIN
SRCMV_QUALIFICATION_* → SRCMV_QUALIFICATION_*
SRCMV_SWIFT_LSP       → SRCMV_SWIFT_LSP
~~~

Update control-tree constants, default configuration resolution, environment
variable reads, temporary paths, lock paths, recovery classification, cleanup,
path validation, and all tests that assert the absence or presence of control
state. The config.toml mapping applies to the user-level configuration
directory as well: the resolved default path becomes
config_dir/srcmv/config.toml.

The first path component .srcmv, case-insensitively, must be reserved in the
same places where .srcmv was previously reserved. If the safety guard for
an existing legacy .srcmv tree is retained, it must reject without reading
or mutating that tree.

#### Journal, hash, and identity domains

Change the persisted and protocol identifiers exactly as selected by the
contract:

~~~text
SRCMV-MANIFEST            → SRCMV-MANIFEST
SRCMV-STATE               → SRCMV-STATE
SRCMV-PLAN-V1             → SRCMV-PLAN-V1
srcmv-physical-identity-v1 → srcmv-physical-identity-v1
~~~

Update:

- journal magic constants and decoders;
- record writers, readers, version checks, and corruption errors;
- physical-identity hashing;
- plan-hash domain prefixes and advertised hash versions;
- lock and recovery logic;
- journal and plan-hash golden fixtures; and
- all protocol documentation describing byte-level contracts.

Changing a domain prefix changes the resulting digest even when the serialized
payload is identical. Recompute every affected digest and inspect the actual
bytes. Do not perform a textual replacement inside a hexadecimal fixture and
assume that the resulting checksum is correct.

#### Schemas and protocol metadata

Update schema URLs and registry annotations:

~~~text
https://srcmv.dev/schema/ → https://raw.githubusercontent.com/atacan/srcmv/main/docs/schema/
x-srcmv-error-registry   → x-srcmv-error-registry
~~~

Update all schema $id and absolute $ref values, schema titles, tests that
assert schema identities, and documentation that describes the registry.
Keep relative $ref values intact unless their target path changes.

Review every versioned contract separately:

- edit protocol request/response schemas;
- selection protocol schemas and error registry;
- transaction manifest/state schemas;
- review-summary schema;
- protocol and selection capability fixtures; and
- plan-hash and transaction golden vectors.

Do not change a numeric protocol version merely because a string was renamed. If
the numeric-version policy changes, update implementation constants, schema
directories, golden directories, capability responses, error cases, and docs as
one atomic protocol change.

### Phase 5: update documentation, examples, and the skill

Update all current documentation and runnable material:

- README.md, including title, badges, repository links, clone commands,
  installation commands, Homebrew commands, binary invocations, package paths,
  environment variables, and skill installation instructions. Add exactly one
  formerly-known-as sentence naming srcmv so historical releases stay
  discoverable; that sentence is allowlisted;
- every non-historical page under docs/, including protocol.md,
  specification.md, security.md, transaction-model.md, agent-integration.md,
  releasing.md, metadata.md, qualification.md, resource-limits.md,
  unsafe-audit.md, and platform/support documentation;
- all non-historical examples, shell scripts, fixtures, transcripts, and
  expected output;
- notes and implementation plans that describe current behavior;
- schema READMEs and golden-fixture READMEs; and
- current release automation guidance.

Add `scripts/audit-identity.sh` to perform the final tracked-path and content
audit against `docs/renaming-allowlist.txt`. Use the equivalent checked-in
script and allowlist in the Homebrew tap; do not rely on a reviewer manually
interpreting raw search output.

Rename and update the skill:

~~~text
skills/srcmv/SKILL.md
skills/srcmv/agents/openai.yaml
skills/srcmv/references/*
~~~

The skill metadata must use name: srcmv, display name srcmv, and the $srcmv
invocation. Update all command examples and installation commands, including:

~~~bash
npx skills add https://github.com/atacan/srcmv --skill srcmv -g -a codex
npx skills add . --skill srcmv -a codex
~~~

Preserve historical release-note wording as required by Section 1.5; do not
rewrite those files during the current-identity documentation update.

### Phase 6: update main-repository release automation

Update .github/workflows/ci.yml as well: package selections, binary paths,
version assertions, and artifact or cache keys that embed the old identity.

Update scripts/package-release.sh:

- expected version output to srcmv VERSION;
- archive basename to srcmv-vVERSION-TARGET.tar.gz;
- staging directory root to srcmv-vVERSION-TARGET;
- installed binary to srcmv; and
- archive contents, permissions, checksum output, and comments.

Update scripts/release.sh:

- Homebrew workflow recovery commands to update-srcmv.yml;
- expected asset names to srcmv-v...tar.gz;
- release text and recovery messages to srcmv; and
- repository or formula references to atacan/srcmv.

Update .github/workflows/release.yml:

- package lookup from srcmv-cli to srcmv-cli;
- build package and binary paths to srcmv-cli and srcmv;
- version assertions to srcmv VERSION;
- archive and archive-root names to srcmv-v...;
- release title to srcmv TAG;
- expected asset lists and checksum validation;
- repository URLs and hard-coded tap references; and
- dispatch event type from srcmv_release_published to
  srcmv_release_published.

Update release notes and release documentation for the selected new version.
Do not alter the names of already-published historical GitHub assets.

The release workflow must continue to preserve these invariants:

- only the publish job has contents: write;
- tags are annotated and immutable;
- the tagged source commit is verified before building and publishing;
- the exact asset set is checked; and
- a failed Homebrew notification never rolls back a public GitHub release.

### Phase 7: rename repository metadata and local maintainer setup

At Gate C, after the source rename branch is prepared and reviewed, the user
renames the GitHub repository to `atacan/srcmv` before the agent merges the
branch or publishes the first srcmv release. GitHub may retain redirects and
historical release URLs; those are external historical behavior and are not
compatibility features implemented by the source.

Update the local remote explicitly:

~~~bash
git remote set-url origin https://github.com/atacan/srcmv.git
git remote get-url origin
~~~

Push and merge the prepared branch through the new remote. Do not modify
`.git/config` with a bulk text replacement; update the remote with Git and
verify the result.

After GitHub completes the rename, update the repository settings metadata to
the new identity: description, topics, and website. Re-check third-party
services keyed to the repository name, such as the DeepWiki badge, and confirm
the README badge renders against atacan/srcmv.

Renaming an individual maintainer's local checkout directory is optional
post-cutover housekeeping, not a repository or release acceptance condition.
Do it from the parent directory only after operations that depend on the old
working directory have finished, then reopen shells, IDEs, and configured agent
workspaces at the new path.

### Phase 8: update Homebrew atomically

In /Users/atacan/Developer/Repositories/homebrew-tap:

The tap also contains unrelated formulas such as record.rb and translate.rb.
This phase must not modify them, and the tap identity audit must pass with
them untouched.

#### Formula

Only after the first new release is published and its checksums are verified,
create `Formula/srcmv.rb` and:

- class Codesplice to Srcmv;
- homepage to https://github.com/atacan/srcmv;
- release URLs to the new repository and srcmv-v... archives;
- bin.install "srcmv";
- all test commands and expected version output; and
- description, comments, and README references.

Do not guess checksums. After the new native archives are published, copy the
SHA-256 values from that release's SHA256SUMS and verify them independently.

#### Formula updater

Update scripts/update-srcmv-formula.rb:

- default formula path to Formula/srcmv.rb;
- SRCMV_FORMULA environment variable;
- upstream repository to atacan/srcmv;
- archive pattern and URL matching to srcmv-v...;
- diagnostics and idempotency messages; and
- formula class/name assumptions.

#### Workflow and tap documentation

Update .github/workflows/update-srcmv.yml:

- workflow name and filename;
- dispatch type srcmv_release_published;
- upstream guard atacan/srcmv;
- archive names, archive roots, installed binary, and version assertions;
- formula path and updater command;
- SRCMV_FORMULA and other environment variables;
- automation branch names;
- expected changed-file checks;
- Homebrew audit/install/test commands; and
- commit, PR, recovery, and manual-run titles/messages.

Update the tap README and manual recovery commands. Exclude .git logs and
reflogs from the content audit; old branch names in Git history do not affect
the current tap.

#### First-release cutover

The first renamed archive does not exist until the new source release is
published, so the tap formula cannot receive its final checksums beforehand.
Coordinate the cutover as follows:

1. At Gate B, review and approve landing a tap workflow-only change: it must recognize
   `srcmv_release_published`, use the renamed updater, and document manual
   recovery, but must not add a formula with guessed or placeholder checksums.
   Confirm whether the updater needs `Formula/srcmv.rb` to exist; if it does,
   make its missing-formula failure explicit and recoverable.
2. Complete Phase 7: rename the GitHub repository, update remotes and guards,
   and merge the reviewed source rename branch through `atacan/srcmv`.
3. Build and publish the first new srcmv release at the selected new version.
4. Verify the published archives and obtain their actual checksums.
5. At Gate E, review and approve merging the formula-only tap change containing
   `Formula/srcmv.rb` with those verified checksums.
6. After Gate E approval, run the renamed tap workflow manually for recovery;
   verify that it is idempotent.
7. Verify brew install atacan/tap/srcmv and brew test atacan/tap/srcmv.

A failed one-time notification is acceptable because the source release is
immutable and remains public; do not delete or mutate that release to make the
tap automation pass.

## 4. Generated and golden artifacts

The following artifacts require regeneration or byte-level review rather than a
blind string replacement:

- root Cargo.lock;
- crates/srcmv-lsp/fuzz/Cargo.lock;
- plan-hash golden digest(s);
- transaction manifest/state encoded records and checksums;
- schema-derived or schema-validated fixtures;
- CLI version/error output fixtures;
- recorded baselines such as docs/performance-baseline.json;
- LSP transcripts and fake-server metadata;
- release archive names and SHA256SUMS; and
- Homebrew formula checksums.

For each regenerated artifact:

1. regenerate it with the project tool or test helper;
2. inspect the complete diff;
3. confirm that only the intended namespace/version changes occurred; and
4. run the test that consumes the artifact.

Do not change binary or hexadecimal fixture content solely because a textual
search did not find the old name. Validate the decoded structure, prefix, and
checksum.

## 5. Verification matrix

### 5.1 Static identity audit

Run from the repository root after all current files are renamed:

~~~bash
scripts/audit-identity.sh docs/renaming-allowlist.txt
~~~

The expected result is exactly the entries in
`docs/renaming-allowlist.txt`; the audit must fail for an unlisted match or an
allowlist entry that matches nothing. Run the same audit in the Homebrew tap,
excluding its `.git` directory and using an equivalent tracked allowlist. The
audit script must inspect both tracked filenames and file contents, while
excluding `.git/**` and generated `target/**` content.

Run positive checks too:

~~~bash
rg --hidden --glob '!.git/**' --glob '!target/**' -n 'srcmv|Srcmv|SRCMV' .
cargo metadata --locked --no-deps --format-version 1
~~~

Verify that all tracked paths, package names, binary names, and skill paths use
the new identity.

### 5.2 Rust checks

~~~bash
git diff --check
cargo fmt --all --check
cargo clippy --locked --workspace --all-targets --all-features -- -D warnings -D clippy::perf
cargo test --locked --workspace --all-features
cargo doc --locked --workspace --no-deps
~~~

Run the nested fuzz package independently because it is its own Cargo workspace:

~~~bash
cargo check --locked --manifest-path crates/srcmv-lsp/fuzz/Cargo.toml --all-targets
scripts/run-fuzz-regressions.sh
~~~

### 5.3 Repository qualification and examples

~~~bash
scripts/check-examples.sh
scripts/audit-unsafe.sh
scripts/qualify-platform.sh
scripts/qualify-lsp.sh
~~~

The LSP qualification script may skip real language-server smoke tests when no
supported server is installed; the fake-server integration tests remain
authoritative.

### 5.4 CLI and protocol checks

Build and verify the renamed executable:

~~~bash
cargo build --locked --release --package srcmv-cli
target/release/srcmv --version
target/release/srcmv capabilities --json
target/release/srcmv protocol-version --json
~~~

Confirm that:

- version output is exactly srcmv VERSION;
- errors begin with srcmv:;
- CARGO_BIN_EXE_srcmv integration tests execute the new binary;
- .srcmv is created and recovered correctly;
- a case-insensitive legacy `.srcmv` tree causes the documented failure
  without enumeration, parsing, locking, migration, or mutation;
- new configuration is resolved only from SRCMV_CONFIG or srcmv/config.toml
  according to the selected contract;
- a leftover user-level srcmv/config.toml is ignored, never read, and
  never removed;
- plan and identity hashes match regenerated golden values; and
- schema validators resolve the final canonical $id and $ref values.

### 5.5 Release packaging checks

For each supported target, build the release binary and run:

~~~bash
scripts/package-release.sh TARGET-TRIPLE target/TARGET-TRIPLE/release/srcmv dist
~~~

Verify the archive contains exactly the expected
srcmv-vVERSION-TARGET root, srcmv binary, license, README, and documented
Markdown files. Verify the binary's version output, executable mode, archive
digest, and archive name.

The GitHub release workflow must then verify:

- both supported native architectures;
- exact archive names and asset set;
- SHA256SUMS values;
- annotated tag/source-commit identity; and
- release title and Homebrew dispatch payload.

### 5.6 Homebrew checks

After the formula PR is merged with checksums from the actual new release:

~~~bash
brew style Formula/srcmv.rb
brew audit --strict --online atacan/tap/srcmv
brew install atacan/tap/srcmv
brew test atacan/tap/srcmv
srcmv --version
~~~

Also exercise the updater twice with the same version and checksums to verify
idempotency, and verify that a downgrade or checksum mismatch is rejected.

## 6. Acceptance criteria

The rename is complete only when all of the following are true:

- the main repository is atacan/srcmv and the local remote points there;
- GitHub repository settings metadata (description, topics, website) uses the
  new identity;
- all tracked source paths use srcmv; local checkout-directory renaming, if
  desired, has been completed as maintainer housekeeping;
- all six Rust packages, imports, dependencies, locks, and generated binary
  names use the new namespace;
- the only installed CLI command is srcmv;
- persistent state uses .srcmv and srcmv/config.toml;
- all new environment variables use SRCMV_*;
- journal, plan-hash, identity, schema, and dispatch identifiers use the
  selected new contract;
- no old compatibility alias or migration path exists;
- generated fixtures and checksums have been regenerated and reviewed;
- current documentation, examples, skill metadata, and automation contain no
  stale old identity outside the documented allowlist;
- the new release has a version never previously published under the old
  product identity on any public distribution surface used by the project;
- the GitHub release asset set and checksums are verified; and
- the renamed Homebrew formula installs, audits, tests, and reports the new
  executable identity.

The final change should leave both repositories clean, with release evidence
recorded for the exact source commit, archive checksums, Homebrew formula
revision, and any intentionally retained historical or legacy literals.
