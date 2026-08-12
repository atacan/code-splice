# Platform support

The `v0.1.0` pilot targets only:

| Operating system | Architecture | Local filesystem | Status |
|---|---|---|---|
| Linux | x86_64 | ext4 | Phase 9 qualified |
| macOS | arm64 | APFS | Phase 9 qualified |

Commit requires the workspace control directory and all changed-target parents to
be on one device. Network, overlay, virtual, cross-device, unqualified local, and
Windows filesystems are rejected for commit. A new supported row requires the
complete Phase 9 qualification suite and an explicit documentation amendment.

The required no-replace operations are `renameat2(RENAME_NOREPLACE)` on Linux and
`renamex_np(RENAME_EXCL)` on macOS. There is no weaker fallback. The durability
policy flushes records and candidates and syncs affected directories where the
platform supports it; recovery after abrupt process termination is the claim,
not power-loss durability.

The shared `scripts/qualify-platform.sh` suite was exercised on 2026-08-12 on a
local macOS arm64/APFS host and on a local QEMU-emulated Linux x86_64 runtime whose
test checkout and temporary workspaces were stored on ext4. The same script is
also required by both platform rows in CI. Detection tests reject representative
NFS, SMB, WebDAV, overlay, tmpfs, and device filesystems; runtime commit remains
allowlist-based, so an unlisted local filesystem is rejected as well.
