# Safety and exactness boundaries

## Exactness contract

CodeSplice edits bytes, not syntax. For each effectful exact operation, the selected-byte SHA-256 equals the inserted-byte SHA-256. It does not:

- Parse a programming language or infer declaration boundaries.
- Update imports, references, module declarations, or build files.
- Format output or normalize line endings.
- Create missing parent directories.

Select only the intended original bytes, review the preview, then make any semantic follow-up changes separately. Never reconstruct the selected bytes manually when the tool rejects an operation; doing so discards its exactness and concurrency checks.

## Workspace and path boundary

Use UTF-8 workspace-relative operation paths. Absolute paths, empty components, `.`, `..`, NUL, symlink traversal, and a first component equal to `.codesplice` ignoring ASCII case are rejected. Existing inputs must be regular files.

The supported v0.1.0 commit environments are:

- Linux x86_64 on local ext4.
- macOS arm64 on local APFS.

Windows, network filesystems, overlay filesystems, unlisted local filesystems, and cross-device transactions are outside the supported release. All changed targets and the control directory must be on one device.

## Aliases, links, and concurrency

CodeSplice detects existing paths that physically alias by device and inode. It rejects changing an existing target with multiple hard links and rejects symlink traversal. Commit revalidates input bytes, absent destinations, path identities, parents, link counts, and workspace identity before mutation.

Treat conflict errors as evidence that the inspected snapshot is stale or the plan is invalid. Re-inspect and preview; never substitute a weaker commit policy.

The trusted-user pilot detects ordinary concurrent edits but is not a sandbox or hostile-filesystem security boundary. A malicious same-user writer can race filesystem checks.

## Multi-file visibility

Multi-target commits are recoverable but not atomically visible. All candidates are prepared before mutation, yet unrelated readers can observe a mixture of original and planned files while commit or rollback is in progress. Avoid running dependent readers concurrently and run repository checks after completion.

## Metadata

Content bytes are the exactness guarantee. CodeSplice preserves POSIX permission bits for an existing changed target. A new target receives `0666 & !startup_umask`.

It does not preserve hard-link relationships, ownership, ACLs, extended attributes, resource forks, timestamps, or platform flags. A changing result reports `METADATA_NOT_PRESERVED`; surface that warning when metadata matters.

## Important release limits

The default release caps include 1,000 operations, 1,000 distinct operation paths, 256 MiB per snapshot file, 1 GiB total snapshot bytes, 512 MiB per resulting output, 1 GiB total planned output, and 100 changed transaction targets. Do not split a logically transactional change merely to bypass a limit. Reduce scope explicitly or use a different workflow after informing the user.

## Failure decisions

| Signal | Required response |
|---|---|
| Stale digest or changed file | Re-inspect every involved path and preview again |
| Expected-plan mismatch | Discard the old digest and review a fresh preview |
| Edit conflict | Fix selectors, anchors, overlaps, aliases, or preconditions; preview again |
| Unsupported path/platform/filesystem/link/device | Stop; do not bypass the safety check |
| Resource limit | Reduce the requested scope without weakening configured maxima |
| Transaction busy or recovery required | Stop mutation and use the recovery workflow |
| Corrupt control or transaction record | Stop; do not repair or delete records ad hoc |

Warnings do not alter the plan digest. Review `OBSERVATION_MAY_BE_STALE`, `METADATA_NOT_PRESERVED`, and `DIFF_TRUNCATED` explicitly rather than treating a successful exit as the only acceptance signal.
