# CodeSplice v0.2.1 release notes

CodeSplice `v0.2.1` is a compatible distribution-automation release. After a
stable release is published and its native assets are verified, the release
workflow now notifies `atacan/homebrew-tap`. The tap independently verifies the
annotated tag, public release state, exact asset set, API digests,
`SHA256SUMS`, both archives, and native binary version before auditing,
installing, and testing an updated formula and opening a reviewable pull
request.

The notification is deliberately downstream of publication. A Homebrew
notification failure cannot roll back or mutate the public release or its
immutable tag, and it has a separate manual recovery path. The tap automation
never commits to its default branch and never approves or merges its own pull
request.

This patch release does not change the CodeSplice executable behavior,
protocol version 1, plan-hash version 1, transaction-record version 1, editing
semantics, limits, qualified platforms, metadata guarantees, or threat model.
The `v0.2.0` release notes remain authoritative for the lock-contention and
opt-in concise-preview additions.

The release contains exactly these assets:

- `codesplice-v0.2.1-x86_64-unknown-linux-gnu.tar.gz`
- `codesplice-v0.2.1-aarch64-apple-darwin.tar.gz`
- `SHA256SUMS`

The two archives are built and tested on their matching qualified native GitHub
runner. Publishing an archive does not extend support beyond Linux x86_64 on
local ext4 and macOS arm64 on local APFS.
