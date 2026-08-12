---
name: codesplice
description: Safely inspect, preview, move, copy, reorder, or split exact bytes already present in workspace files with the CodeSplice CLI. Use when a coding task asks to relocate existing code or data within or across files, preserve selected bytes exactly, construct protocol-v1 requests, commit an expected preview plan, diagnose CodeSplice conflicts, or inspect and recover an interrupted CodeSplice transaction.
---

# CodeSplice

Use CodeSplice for exact relocation or duplication of bytes that already exist in a workspace. Keep generated content, import changes, formatting, and semantic refactors outside the CodeSplice operation.

## Enforce the agent safety gate

For every mutation, follow this sequence without shortcuts:

1. Inspect every source and destination.
2. Build the request from the observed digests and coordinates.
3. Preview the request and review the resolved operations, outputs, diff, warnings, and `plan_sha256`.
4. Commit the unchanged request with `--expect-plan` set to that previewed digest.
5. Verify the commit response and resulting workspace.

Never use `--accept-current-plan`. If a digest, file, or plan changed, inspect and preview again. Never reproduce the selected bytes by hand as a fallback after CodeSplice rejects an operation.

Read [references/workflow.md](references/workflow.md) before executing any mutating task. It contains the commands, review gates, and retry rules.

## Keep the operation in scope

CodeSplice moves or copies byte ranges selected by line or byte coordinates. It does not parse languages, update imports, format code, normalize line endings, create parent directories, or provide atomic multi-file visibility. Treat follow-up semantic edits as separate work and test the final workspace.

Before use, confirm `codesplice` is available and query `capabilities --json` when version or feature support is uncertain. See [references/cli-protocol.md](references/cli-protocol.md) for command grammar, response fields, protocol version, and error categories.

## Load only the needed detail

- Read [references/request-construction.md](references/request-construction.md) when selecting lines or bytes, choosing anchors, creating a new destination, composing multiple operations, or interpreting a no-op.
- Read [references/safety-and-exactness.md](references/safety-and-exactness.md) before multi-file work, binary or mixed-line-ending work, metadata-sensitive changes, or when a path, platform, alias, resource, or precondition check fails.
- Read [references/recovery.md](references/recovery.md) only when recovery is requested or a transaction is unfinished, busy, interrupted, or corrupt. Do not choose completion versus rollback without explicit user intent.

## Preserve evidence

Keep the request and preview response available through commit. Report the previewed plan digest, transaction state, files changed, warnings, and any separate follow-up edits. For a no-op, verify that `transaction_id` is `null` and `transaction_state` is `no_op` rather than claiming a mutation occurred.
