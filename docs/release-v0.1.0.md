# CodeSplice v0.1.0 release contract

The `v0.1.0` tag freezes the pilot contract without expanding its scope.

## Frozen versions and registries

- request/response protocol: version 1, with the schemas under `docs/schema/v1`;
- deterministic plan hash: version 1 and its checked-in CBOR golden vectors;
- manifest/state record envelope and payload schemas: version 1;
- errors and exit categories: the registry in `docs/protocol.md`;
- warnings: `OBSERVATION_MAY_BE_STALE`, `METADATA_NOT_PRESERVED`, and
  `DIFF_TRUNCATED`;
- limits: every row in `docs/resource-limits.md`;
- support matrix: Linux x86_64/ext4 and macOS arm64/APFS only; and
- metadata and threat model: the exclusions in `docs/metadata.md` and the
  trusted-user boundary in `docs/security.md`.

Breaking protocol changes require a new protocol version. Adding a platform or
filesystem requires the complete qualification suite and a documentation
amendment. The release makes no Windows, network-filesystem, hostile-writer,
power-loss durability, atomic multi-file visibility, ACL, xattr, ownership,
resource-fork, timestamp, platform-flag, or hard-link-preservation claim.

## Pilot and defect gate

`scripts/run-codex-pilot.sh` runs the 15 real-repository scenarios through the
agent inspect, preview, and `commit --expect-plan` workflow. It rejects an
unqualified host row, requires a real second-device mount for the cross-device
case, and asserts that no subprocess argument contains `--accept-current-plan`.
The retained Phase 10 evidence records 15/15 passing scenarios on both qualified
rows. Release acceptance found no unresolved exactness, overwrite, rollback,
path-escape, or record-corruption defect.

## Packages

`scripts/package-release.sh` accepts only these archive targets:

- `codesplice-v0.1.0-x86_64-unknown-linux-gnu.tar.gz`
- `codesplice-v0.1.0-aarch64-apple-darwin.tar.gz`

Each archive contains the native `codesplice` binary, Apache-2.0 license, README,
protocol contract, agent workflow, and platform support contract. Phase 10 records
the generated archive SHA-256 values in its checkpoint after building on the
matching native target.
