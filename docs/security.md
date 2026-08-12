# Security boundary

CodeSplice `v0.1.0` is a trusted-user pilot, not a hostile-filesystem security
boundary. It detects ordinary concurrent edits and refuses ambiguous recovery,
but it does not defend against a malicious process with workspace write access.

Operation paths are UTF-8, normalized, workspace-relative paths. Absolute paths,
empty components, `.`, `..`, NUL, symlink traversal, and the ASCII-case-insensitive
reserved first component `.codesplice` are rejected. Existing inputs must be
regular files. Existing path aliases are detected by POSIX device and inode.

The canonical workspace root, parents, inputs, absences, and target link counts
are revalidated before mutation. Changed existing files with multiple hard links
are rejected. All target backup, install, and restore renames require native
no-replace semantics.

The `.codesplice` control tree and transaction directories must be real objects
owned by the effective user and must not be group- or other-writable; transaction
directories are mode `0700`. The lock is a real regular file and is never repaired
silently. Mutation uses a nonblocking exclusive advisory lock, diagnostics use a
nonblocking shared lock, and ordinary replacement is detected by retained physical
identities before any future target mutation.

Linux x86_64/ext4 and macOS arm64/APFS local filesystems are the only pilot
configurations. Windows, network filesystems, hostile namespace-race resistance,
and power-loss durability are outside the `v0.1.0` claim.
