# Releasing CodeSplice

CodeSplice publishes native archives as GitHub Release assets. A personal
Homebrew tap can use those immutable, versioned URLs and the matching values in
`SHA256SUMS` without maintaining another binary host.

The release archives do not broaden the qualification contract. CodeSplice is
qualified only for Linux x86_64 on ext4 and macOS arm64 on APFS. A binary may
download or start on another filesystem, but that is not a supported row.

## One-time repository setup

1. Merge `.github/workflows/release.yml` into the default branch before creating
   or pushing a version tag. The tagged commit must contain the workflow.
2. Keep the workflow's default `contents: read` permission. Its publish job is
   the only job granted `contents: write`.
3. Protect tags matching `v*` with a GitHub ruleset. If the repository offers
   immutable releases, enable that setting as defense in depth after testing the
   first release.
4. Do not move or reuse a published version tag. Correct a bad release with a
   new patch version.

The existing `v0.1.0` tag predates this workflow. Do not move that tag merely to
trigger automation. If it has not been published, either upload its previously
qualified Phase 10 archives manually or publish the next patch release after
the workflow is present in that release's tagged commit.

## Release procedure

1. Update the workspace version in `Cargo.toml`, regenerate `Cargo.lock` if it
   changes, and update version-specific release notes and contracts.
2. Run the normal qualification and release acceptance checks.
3. Commit and merge the release candidate to the default branch.
4. Create an annotated tag whose version exactly matches the `codesplice-cli`
   package version, then push it:

   ```console
   git tag -a v0.1.1 -m "CodeSplice v0.1.1"
   git push origin v0.1.1
   ```

Only stable tags of the form `vMAJOR.MINOR.PATCH` pass the workflow gate. The
workflow also requires the tag to be annotated and resolves it back to the exact
GitHub event commit before building and immediately before publishing.

For each tag, GitHub-hosted native runners:

- test the locked workspace;
- execute every checked-in user-facing example;
- confirm the OS, CPU, Rust host target, and qualified filesystem;
- build with the repository's pinned Rust toolchain and `Cargo.lock`;
- verify the executable format and reported version; and
- package only `x86_64-unknown-linux-gnu` and `aarch64-apple-darwin`.

The publish job waits for both builders, downloads their short-lived workflow
artifacts, creates a stable `SHA256SUMS`, and uploads all three files to a draft
GitHub Release. It verifies the exact asset set and tag again before publishing
the draft. A failed upload therefore does not expose a partial public release.
A rerun replaces an abandoned draft but refuses to replace an already published
release.

## Homebrew consumption

GitHub Release assets are the right first distribution source for the personal
tap. They provide stable versioned URLs, checksums, access logs, and one place to
retain release notes. Each tap update should copy the target archive digest from
the release's `SHA256SUMS`; never point a formula at a branch archive or `latest`
URL.

For example, the tap can choose the native URL and digest inside `on_macos` and
`on_linux` blocks and install `codesplice` from the archive's `bin.install`
equivalent. Keep the same two-row OS/architecture restrictions in the formula.

The more Homebrew-native refinement is to build bottles from a source formula
and publish the bottle metadata and archives from the tap's own workflow. That
adds reproducible formula testing and `brew`-managed bottle selection, but also
adds a second release pipeline. Start with the upstream GitHub Release assets;
adopt bottles once the formula and release cadence are stable.

## Failure recovery

- If either native build fails, no GitHub Release is created.
- If release creation or verification fails, the release remains a private
  draft. Rerun the workflow for the same tag after fixing transient service
  issues; it will replace only that draft.
- If the release is already public, the workflow stops. Do not delete or mutate
  it to make a rerun pass.
- If the tag or packaged content is wrong, increment the version, qualify a new
  commit, and publish a new tag.
