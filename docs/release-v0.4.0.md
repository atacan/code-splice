# srcmv v0.4.0 release notes

This release renames CodeSplice to srcmv. It is a breaking rename with no
migration support, and it starts a new version line at 0.4.0 so that no version
previously published under the old product identity is reused.

CodeSplice is the former product identity of srcmv. The historical releases
published as `atacan/code-splice` keep their names, tags, and asset names and
remain unchanged; they are not current distribution surfaces. This repository
is now `atacan/srcmv`.

## Breaking rename

- The executable is `srcmv`. No `codesplice` alias is installed or recognized.
- Workspace control state lives in `.srcmv/` instead of `.codesplice/`.
- User-level configuration resolves only from the `SRCMV_CONFIG` environment
  variable or `<configuration directory>/srcmv/config.toml`. A leftover
  user-level `codesplice/config.toml` from the old tool is ignored: it is not
  read, guarded, migrated, or deleted.
- Environment variables change from `CODESPLICE_*` to `SRCMV_*` (for example
  `SRCMV_TEST_*`, `SRCMV_PILOT_*`, `SRCMV_BIN`, `SRCMV_QUALIFICATION_*`,
  `SRCMV_SWIFT_LSP`).
- Journal magic, physical-identity and plan-hash hash domains, JSON Schema
  identifiers, error-registry annotations, and the release dispatch event move
  to the `SRCMV` namespace (`SRCMV-MANIFEST`, `SRCMV-STATE`, `SRCMV-PLAN-V1`,
  `srcmv-physical-identity-v1`, `srcmv_release_published`).
- Rust packages, paths, and imports rename from `codesplice-*`/`codesplice_*`
  to `srcmv-*`/`srcmv_*`, and the agent skill moves from `skills/codesplice/`
  to `skills/srcmv/`.

## No migration path

There is no migration path from `.codesplice` to `.srcmv`. Complete or clean up
all unfinished transactions with the old tool before upgrading; a transaction
started with CodeSplice cannot be recovered with srcmv.

Before an operation opens, creates, recovers, or mutates workspace control
state, srcmv checks only the workspace root for a first path component named
`.codesplice` (ASCII case-insensitive). If one is present, the operation fails
with a documented `srcmv:` error directing you to finish or remove that tree
with the old tool first. srcmv does not enumerate, parse, lock, migrate, or
modify that tree.

## Unchanged protocol numbers, new namespace

The JSON grammar is unchanged, so protocol v1 requests keep
`protocol_version: 1` and selection responses keep
`selection_protocol_version: 1`. srcmv is nevertheless a new, incompatible
namespace: it does not accept old clients, journal records, hash domains,
schema identifiers, or dispatch events from CodeSplice. Plan digests differ
from CodeSplice's for identical payloads because the hashed domain prefixes
changed. Canonical JSON Schemas now live under
`https://raw.githubusercontent.com/atacan/srcmv/main/docs/schema/`.

## Homebrew switch

Uninstall the old formula, then install the renamed one:

```console
brew uninstall atacan/tap/codesplice || true
brew install atacan/tap/srcmv
```

Skip the uninstall step if the old formula was never installed on this machine.

The supported rows remain Linux x86_64 on local ext4 and macOS arm64 on local
APFS. Publishing an archive does not extend support beyond those platforms.

The release contains exactly these assets:

- `srcmv-v0.4.0-aarch64-apple-darwin.tar.gz`
- `srcmv-v0.4.0-x86_64-unknown-linux-gnu.tar.gz`
- `SHA256SUMS`

The two archives are built and tested on their matching qualified native GitHub
runner.
