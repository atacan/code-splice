# Platform support

The `v0.1.0` pilot targets only:

| Operating system | Architecture | Local filesystem | Status |
|---|---|---|---|
| Linux | x86_64 | ext4 | Intended pilot; qualification occurs in Phase 9 |
| macOS | arm64 | APFS | Intended pilot; qualification occurs in Phase 9 |

Commit requires the workspace control directory and all changed-target parents to
be on one device. Network, overlay, virtual, cross-device, unqualified local, and
Windows filesystems are rejected for commit. A new supported row requires the
complete Phase 9 qualification suite and an explicit documentation amendment.

The required no-replace operations are `renameat2(RENAME_NOREPLACE)` on Linux and
`renamex_np(RENAME_EXCL)` on macOS. There is no weaker fallback. The durability
policy flushes records and candidates and syncs affected directories where the
platform supports it; recovery after abrupt process termination is the claim,
not power-loss durability.
