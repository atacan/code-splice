# CodeSplice v0.2.0 release notes

CodeSplice `v0.2.0` adds clearer lock-contention guidance and an opt-in concise
preview for reviewing multi-file plans. It also introduces a shared internal
line-metrics primitive so exact line accounting is consistent across indexed
source ranges and composed output segments.

Highlights:

- `TRANSACTION_BUSY` errors now report stable `lock_state`,
  `recovery_required`, and `safe_next_action` context so callers can wait and
  retry without mistaking contention for proven recovery work.
- `apply --preview --summary` adds complete, typed `review-summary-v1` metadata
  under `diff.summary.review` while retaining the bounded diff.
- `apply --preview --summary --no-diff` is the concise review mode for operation
  order, output changes, insertion groups, and logical-line metrics.
- Existing preview output remains unchanged when `--summary` is omitted.
- Line metrics correctly compose LF, CRLF, lone CR, unterminated lines, and
  segment boundaries without rescanning assembled outputs.

This minor release does not change protocol version 1, plan-hash version 1,
transaction-record version 1, editing behavior, limits, qualified platforms,
metadata guarantees, or threat model. The frozen `v0.1.0` release contract
remains authoritative for those guarantees and boundaries. The nested
`review-summary-v1` format uses protocol v1's existing open summary extension
point and has its own versioned schema.

The release contains exactly these assets:

- `codesplice-v0.2.0-x86_64-unknown-linux-gnu.tar.gz`
- `codesplice-v0.2.0-aarch64-apple-darwin.tar.gz`
- `SHA256SUMS`

The two archives are built and tested on their matching qualified native GitHub
runner. Publishing an archive does not extend support beyond Linux x86_64 on
local ext4 and macOS arm64 on local APFS.
