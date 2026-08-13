# CodeSplice

**Exact, byte-preserving code movement for developers and coding agents.**

CodeSplice is a Rust command-line tool that moves or copies code already present
in a workspace. It selects line or byte ranges from an immutable snapshot and
inserts those exact bytes elsewhere—without parsing the language, reformatting
the code, normalizing line endings, or asking an agent to reproduce the text.

That makes CodeSplice useful for refactors where textual fidelity matters:
moving a function to another file, copying a declaration, reordering blocks in
one file, splitting one source file across several destinations, and preserving
CRLF, mixed line endings, or non-UTF-8 data.

CodeSplice `v0.1.1` is a deliberately bounded pilot. It is qualified only for
Linux x86_64 workspaces on local ext4 and macOS arm64 workspaces on local APFS.

## Why CodeSplice?

Coding agents are good at deciding *what* should move, but regenerating an
existing block can introduce incidental whitespace, encoding, or line-ending
changes. CodeSplice separates those responsibilities: the caller chooses a
source range and destination; the CLI verifies preconditions, previews a
deterministic plan, and transfers the original bytes.

Core features include:

- exact `move` and `copy` operations using line or half-open byte ranges;
- insertion at file start/end, before/after a line, or at a byte offset;
- same-file reordering, explicit no-op detection, and new-file destinations;
- multi-operation and multi-target plans from one immutable workspace snapshot;
- read-only inspection with file digests, byte lengths, and line counts;
- bounded text or binary previews with a deterministic `sha256:` plan digest;
- optimistic file preconditions and `commit --expect-plan` protection against a
  stale source, destination, or preview;
- persistent transaction records plus explicit completion or rollback; and
- strict protocol-v1 JSON schemas, stable error codes, and machine-readable
  reports suitable for automation.

Explore the runnable, before-and-after walkthroughs in [`examples/`](examples/).
They cover the user-facing feature set and are the easiest way to try the CLI
without modifying a real project.

## Install

CodeSplice requires Rust 1.97 or newer when building from source.

### From this checkout

```bash
git clone https://github.com/atacan/code-splice.git
cd code-splice
cargo install --locked --path crates/codesplice-cli
codesplice --version
```

Reinstall after making local changes with:

```bash
cargo install --locked --force --path crates/codesplice-cli
```

### Prebuilt binaries and Homebrew

The repository contains release packaging for these two qualified targets:

- `x86_64-unknown-linux-gnu`
- `aarch64-apple-darwin`

Native archives and checksums are published with each current GitHub Release.
Configuring the personal Homebrew tap is still forthcoming. Once available, the
intended Homebrew command is:

```bash
brew install atacan/tap/codesplice
```

Until the tap is live, download a supported archive from GitHub Releases or
install from this checkout.

## Safe quickstart

Every automated mutation should follow the same three stages:

```text
inspect -> preview -> commit --expect-plan
```

The following example moves line 2 from `source.rs` to the end of
`destination.rs`. It requires `jq` and creates a fresh disposable directory:

```bash
DEMO_DIR=$(mktemp -d "${TMPDIR:-/tmp}/codesplice-demo.XXXXXX")
mkdir "$DEMO_DIR/workspace"
printf 'fn stay() {}\nfn move_me() {}\n' > "$DEMO_DIR/workspace/source.rs"
printf 'fn destination() {}\n' > "$DEMO_DIR/workspace/destination.rs"

codesplice --workspace "$DEMO_DIR/workspace" inspect \
  --path source.rs --path destination.rs --json > "$DEMO_DIR/inspection.json"

SOURCE_SHA=$(jq -r '.paths[] | select(.path == "source.rs") | .sha256' "$DEMO_DIR/inspection.json")
DESTINATION_SHA=$(jq -r '.paths[] | select(.path == "destination.rs") | .sha256' "$DEMO_DIR/inspection.json")

jq -n --arg source_sha "$SOURCE_SHA" --arg destination_sha "$DESTINATION_SHA" '{
  protocol_version: 1,
  operations: [{
    kind: "move",
    source: {
      path: "source.rs",
      selector: {kind: "lines", start: 2, end: 2},
      precondition: {kind: "sha256", value: $source_sha}
    },
    destination: {
      path: "destination.rs",
      anchor: {kind: "file_end"},
      precondition: {kind: "sha256", value: $destination_sha}
    }
  }]
}' > "$DEMO_DIR/request.json"

codesplice --workspace "$DEMO_DIR/workspace" apply \
  --request "$DEMO_DIR/request.json" --preview --json > "$DEMO_DIR/preview.json"

PLAN_SHA=$(jq -r '.plan_sha256' "$DEMO_DIR/preview.json")

codesplice --workspace "$DEMO_DIR/workspace" apply \
  --request "$DEMO_DIR/request.json" --commit --expect-plan "$PLAN_SHA" --json
```

Review `$DEMO_DIR/preview.json` before committing. If the workspace or plan
changes, CodeSplice rejects the commit; inspect and preview again. Coding agents
should never bypass this check with `--accept-current-plan`.

For interrupted work, inspect persistent transactions before choosing an
explicit action:

```bash
codesplice --workspace "$DEMO_DIR/workspace" recover --list --json
codesplice --workspace "$DEMO_DIR/workspace" recover TRANSACTION_ID --status --json
codesplice --workspace "$DEMO_DIR/workspace" recover TRANSACTION_ID --complete --json
# Or: codesplice --workspace "$DEMO_DIR/workspace" recover TRANSACTION_ID --rollback --json
```

## Coding-agent skill

The repository includes a progressively disclosed agent skill under
[`skills/codesplice/`](skills/codesplice/). Its short `SKILL.md` routes agents to
focused references only when a task needs them.

After the repository is publicly available, install it for Codex with the open
[Skills CLI](https://skills.sh/docs/cli):

```bash
npx skills add atacan/code-splice --skill codesplice -g -a codex
```

From a local checkout, omit `-g` to install it for the current project:

```bash
npx skills add . --skill codesplice -a codex
```

The command is `npx skills`, not `mpx skills`.

## Guarantees and boundaries

CodeSplice guarantees the equality of selected and inserted content bytes for
effectful exact-mode operations. It preserves the POSIX permission bits of an
existing changed target and assigns a new target according to the startup umask.

It does **not** parse code, update imports, format output, normalize newlines, or
create parent directories. It also does not preserve ownership, ACLs, extended
attributes, resource forks, timestamps, platform flags, or hard-link
relationships. Changed files with multiple hard links are rejected.

Multi-target commit is **recoverable, not atomically visible**: unrelated readers
may temporarily observe a mixture of old and new files. Recovery after abrupt
process termination is supported; power-loss durability is not claimed.

The `v0.1.0` threat model assumes a trusted local user. CodeSplice rejects
absolute or escaping paths, symlink traversal, unsupported file types and
filesystems, cross-device transactions, and detected concurrent edits, but it is
not a sandbox or a defense against a malicious same-user process racing the
workspace.

See the frozen contracts for exact details:

- [editing semantics](docs/specification.md)
- [protocol and error registry](docs/protocol.md)
- [agent workflow](docs/agent-integration.md)
- [transaction and recovery model](docs/transaction-model.md)
- [resource limits](docs/resource-limits.md)
- [metadata contract](docs/metadata.md)
- [security boundary](docs/security.md)
- [qualified platforms](docs/platform-support.md)
- [`v0.1.0` release contract](docs/release-v0.1.0.md)
- [`v0.1.1` release notes](docs/release-v0.1.1.md)
- [release automation and Homebrew handoff](docs/releasing.md)

Protocol version 1, plan-hash version 1, and transaction-record version 1 are
frozen at the `v0.1.0` tag. Breaking wire-format changes require a new protocol
version.

## Repository layout

- `crates/codesplice-core`: immutable domain model and pure planning.
- `crates/codesplice-fs`: workspace snapshots, transactions, and recovery.
- `crates/codesplice-protocol`: strict JSON protocol and reports.
- `crates/codesplice-cli`: argument parsing, orchestration, and rendering.
- `crates/codesplice-test-support`: test-only fixtures and helpers.
- `examples/`: runnable user-facing demonstrations.
- `skills/codesplice/`: reusable instructions for coding agents.
- `docs/`: public behavior, support, and release contracts.

The detailed implementation record is
[`notes/implementation_plan.md`](notes/implementation_plan.md).

## Development

```bash
cargo fmt --all --check
cargo clippy --workspace --all-targets --all-features --locked -- \
  -D warnings -D clippy::perf
cargo test --workspace --all-features
cargo build --workspace --all-features
```

Run the complete platform qualification suite only on a qualified host and
filesystem:

```bash
scripts/qualify-platform.sh
```

CodeSplice is licensed under [Apache-2.0](LICENSE).
