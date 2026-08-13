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

The trusted-user boundary means a same-user process intentionally racing path
replacement can defeat assumptions between individual checks. CodeSplice is not
a sandbox, privilege boundary, malware defense, or guarantee against power-loss
reordering. It fails closed for detected ordinary edits, identity changes,
unsupported filesystems, cross-device layouts, and ambiguous recovery state.

## Semantic-selection boundary

`codesplice select` is read-only with respect to CodeSplice's edit and
transaction engines, but it starts a separately installed language-server
executable. That executable is inside the trusted-user boundary. It runs with the
invoking user's privileges, inherits the process environment, receives canonical
workspace and source file URIs plus the exact UTF-8 source snapshot, and may read
other project files while indexing. CodeSplice is not a sandbox for the server
and cannot prevent a malicious or compromised server from reading, changing, or
exfiltrating files using its own process privileges.

CodeSplice itself sends only `initialize`, `initialized`, optional
`workspace/didChangeConfiguration`, `textDocument/didOpen`,
`textDocument/documentSymbol`, `textDocument/didClose`, `shutdown`, and `exit`,
plus bounded responses to server-initiated requests needed for selection. It
advertises `workspace.applyEdit: false`, answers
`workspace/applyEdit` with `applied: false`, and rejects unsupported capability
or malformed range responses. It does not send `didChange`, `willSave`,
`didSave`, rename, formatting, code-action, or edit requests. This protects the
selection workflow from accidental LSP edits; it is not a defense against an
executable that performs filesystem writes directly.

Server programs and arguments are launched directly, without a shell. Repeated
`--server-arg` values are literal: no interpolation, globbing, pipelines, or
command substitution occurs. Automatic and built-in discovery uses only
absolute `PATH` components, ignores empty and relative components, canonicalizes
regular executable candidates, and refuses executables inside the workspace.
This prevents common `PATH=.` and workspace `bin` poisoning during automatic
selection.

The exceptions are deliberate trust decisions. An explicit
`--server-program PROGRAM --language-id ID` may name a relative or
workspace-local executable. A user descriptor in the trusted configuration may
do the same only with `allow_workspace_program = true`. Review either source
before use. `CODESPLICE_CONFIG` names the trusted configuration file exactly;
CodeSplice never reads configuration from the workspace implicitly.

The server is placed in a dedicated process group. On failure or timeout,
CodeSplice terminates the group, reaps the direct child, and joins its bounded
stdio workers. This covers ordinary wrapper-spawned helpers but cannot guarantee
cleanup of a malicious process that independently daemonizes or escapes the
group.

### Immutable snapshot and mixed-time observation

Selection acquires the existing shared diagnostic lock when one exists, scans
recovery state, captures one immutable source snapshot and its digest, then
releases the lock before starting the external server. A slow or indexing server
therefore does not hold the CodeSplice lock and does not block an unrelated
commit after capture.

The selected file content sent with `didOpen` is the captured snapshot, and all
LSP ranges are converted only against those bytes. The server may observe other
project files later, after the lock has been released. Its semantic answer can
therefore reflect a mixed-time project view. Selection makes no whole-project
consistency claim. The selected source's SHA-256 is embedded in every
`request_source`; if that source changes before ordinary preview or commit, the
unchanged protocol-v1 precondition rejects the edit. Changes to other project
files can affect semantic interpretation without invalidating that source
precondition, so callers should rerun selection when project context has changed
materially.

Only valid UTF-8 source is sent to LSP. Raw source text, absolute paths, server
command lines and arguments, environment values, settings, initialization
options, and stderr are omitted from selection JSON and structured error
contexts. Human output visibly escapes terminal control and bidirectional
formatting characters.

Semantic selection is supported on the same two qualified release rows as the
CLI: Linux x86_64/ext4 and macOS arm64/APFS. Language-server behavior remains an
external compatibility boundary; the real-server qualification script exercises
installed `clangd` and `rust-analyzer` when present, while bounded fake-server
tests remain authoritative for the protocol and failure behavior.
