# CodeSplice v0.1.1 release notes

CodeSplice `v0.1.1` is a compatible distribution and onboarding release. It
publishes the native GitHub Release assets needed by the planned personal
Homebrew tap and includes the executable examples, progressively disclosed
coding-agent skill, and improved developer and agent documentation added after
`v0.1.0`.

This patch release does not change protocol version 1, plan-hash version 1,
transaction-record version 1, editing behavior, limits, error or warning
registries, qualified platforms, metadata guarantees, or threat model. The
frozen [`v0.1.0` release contract](release-v0.1.0.md) remains authoritative for
those guarantees and boundaries.

The release contains exactly these assets:

- `codesplice-v0.1.1-x86_64-unknown-linux-gnu.tar.gz`
- `codesplice-v0.1.1-aarch64-apple-darwin.tar.gz`
- `SHA256SUMS`

The two archives are built and tested on their matching qualified native GitHub
runner. Publishing an archive does not extend support beyond Linux x86_64 on
local ext4 and macOS arm64 on local APFS.
