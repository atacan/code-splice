# Phase 10 checkpoint

Status: PASS
Commit: not committed

## Delivered

- A repeatable real-repository Codex pilot that uses the release binary and the
  required `inspect -> preview -> commit --expect-plan` workflow. The harness
  rejects any subprocess argument containing `--accept-current-plan`.
- All 15 mandated scenarios over copies of real files from this repository,
  including exact text and binary movement, single- and multi-target changes,
  stale-plan rejection, crash recovery, mode preservation, path confinement,
  cross-device rejection, and no-replace collision handling.
- Qualified pilot runners for Linux x86_64/ext4 and macOS arm64/APFS. Each runner
  provisions a real second device for the cross-device scenario and refuses any
  operating-system, architecture, or filesystem row outside the release matrix.
- The protocol-1 release capability response with every Phase 1-10 feature
  enabled and `implementation_phase` set to 10.
- A frozen `v0.1.0` contract covering schemas, plan-hash and record versions,
  error and warning registries, limits, support matrix, metadata exclusions, and
  threat-model claims. No protocol, platform, or security claim was expanded.
- A package builder that accepts only `x86_64-unknown-linux-gnu` and
  `aarch64-apple-darwin` and includes the binary plus the frozen license,
  protocol, agent-workflow, and platform-support documents.
- Retained pilot and package evidence under `notes/evidence/phase-10`.

## Baseline and prior checkpoints

- Phase 9 baseline `4ec465cb2c5e8be0974b884a623fa2204c09fdbc` was the initial
  `HEAD`, resolved to a commit object, and remained the ancestor and repository
  content source used by both pilots.
- `notes/checkpoints/phase-1.md` through `phase-9.md` each report `Status: PASS`.
- The verified Phase 1-9 checkpoint commits are, in order:
  `592e639ac99de3ca400498d16cf4cb3ba3c76caf`,
  `8c8f52be2407806dd5289b320c4dd15ece567be9`,
  `25df2b04f3c80642f6a0b5a684c7059a69db1bbb`,
  `c8bf440d424b52b9fad90e8aa4f95942168a460f`,
  `4c5fa8c8bc1609158344d0ddd6fd2a908737f71c`,
  `410366e4b974d69cb82315a159716f6c57be4ae6`,
  `8d359031577d5250d862a03ed9adf200c216eb7f`,
  `6f1ee999f86cd00a65ea8302cb8187dc66526044`, and
  `4ec465cb2c5e8be0974b884a623fa2204c09fdbc`.

## Pilot evidence

Both rows ran `scripts/run-codex-pilot.sh` on 2026-08-12. Linux used a local
QEMU-emulated x86_64 Docker runtime with the checkout and pilot workspaces on an
ext4 volume; macOS used the local Apple Silicon host and APFS workspaces. The raw
runner evidence hashes are recorded in `notes/evidence/phase-10/pilot-results.json`.

| # | Scenario | Linux x86_64/ext4 | macOS arm64/APFS |
|---:|---|---|---|
| 1 | Move a function to an existing file | PASS | PASS |
| 2 | Move a function to a new file | PASS | PASS |
| 3 | Copy a declaration | PASS | PASS |
| 4 | Reorder code in one file | PASS | PASS |
| 5 | Execute a same-file no-op | PASS | PASS |
| 6 | Split one file into two outputs | PASS | PASS |
| 7 | Split one file into three outputs | PASS | PASS |
| 8 | Preserve CRLF and mixed terminators | PASS | PASS |
| 9 | Move non-UTF-8 payload bytes | PASS | PASS |
| 10 | Reject a stale source digest | PASS | PASS |
| 11 | Reject an expected-plan mismatch | PASS | PASS |
| 12 | Recover an interrupted multi-file commit by completion | PASS | PASS |
| 13 | Recover an interrupted multi-file commit by rollback | PASS | PASS |
| 14 | Preserve a permission change after preview | PASS | PASS |
| 15 | Reject unsafe paths, cross-device mutation, and external collision | PASS | PASS |

Every successful mutation used the previewed `sha256:` plan digest with
`--expect-plan`. Negative scenarios stopped at the expected rejection or explicit
recovery action. Neither platform used `--accept-current-plan`.

## Release acceptance verification

- `cargo fmt --all --check` — pass
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — pass
  on both qualified rows
- `cargo test --workspace --all-features` — pass; the qualified pilot is
  intentionally ignored by the ordinary suite and was executed separately
- `cargo build --workspace --all-features` — pass on both qualified rows
- `scripts/qualify-platform.sh` — pass on macOS arm64/APFS, including ordinary,
  crash, platform, fuzz, unsafe, AddressSanitizer, and allocator checks
- Equivalent shared qualification sequence — pass on Linux x86_64/ext4; the
  allocator fuzz command passed in a serial focused rerun in the same retained
  checkout after QEMU killed one concurrent run for resource pressure
- `scripts/run-codex-pilot.sh` — 15/15 pass on Linux x86_64/ext4 and 15/15 pass
  on macOS arm64/APFS, both with real second-device mounts
- `scripts/audit-unsafe.sh` — pass through both qualification sequences
- `bash -n scripts/audit-unsafe.sh scripts/qualify-platform.sh
  scripts/run-fuzz-regressions.sh scripts/run-codex-pilot.sh
  scripts/package-release.sh` — pass
- `git diff --check` — pass
- Both packaged executables report protocol version 1, implementation phase 10,
  and all release features enabled.

Review of these results found no unresolved exactness, overwrite, rollback,
path-escape, or record-corruption defect.

## Release packages

Only the two qualified target archives were built:

| Target | SHA-256 |
|---|---|
| `x86_64-unknown-linux-gnu` | `fa0cb4bffe7122633e37484475af3ea79f31252434c3237e3e435858dc3893d8` |
| `aarch64-apple-darwin` | `617699833a36a136fc9878b192033729c510e958b198a9bd574fb42f0efe5956` |

Archive listings contain only the target directory, native `codesplice` binary,
`LICENSE`, `README.md`, `protocol.md`, `agent-integration.md`, and
`platform-support.md`. The binaries were verified as ELF x86-64 and Mach-O arm64,
respectively. Exact sizes and digests are retained in
`notes/evidence/phase-10/release-packages.json`.

## Decisions made within phase authority

- Keep the release support and packaging matrix exact: Linux x86_64/ext4 and
  macOS arm64/APFS only.
- Close protocol version 1 at `v0.1.0`; breaking wire changes require a new
  protocol version.
- Preserve the existing metadata and trusted-user threat-model boundaries
  verbatim rather than treating pilot success as broader evidence.
- Keep the real-device pilot as an explicit ignored integration test invoked by
  its qualified runner, because ordinary unit-test hosts cannot safely fabricate
  the required cross-device condition.

## Deviations or concerns

- The qualified Linux environment is the same local QEMU-emulated x86_64/ext4
  row documented in Phase 9, not physical x86_64 hardware. QEMU killed one
  concurrent allocator-diagnostic process under resource pressure; the exact
  diagnostic passed serially with all 512 property cases in the same checkout.
- macOS emitted the previously documented sandboxed Xcode cache warnings during
  qualification; all compilation, sanitizer, allocator, and test stages passed.

## Release readiness

- PASS. Phase 10 is complete and ready for the release commit and annotated
  `v0.1.0` tag. No work beyond that commit and tag is authorized by this phase.
