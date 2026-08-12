# Agent integration

Agents follow an inspect, preview, commit sequence:

```bash
codesplice --workspace /path/to/repo inspect \
  --path src/source.rs --path src/destination.rs --json

codesplice --workspace /path/to/repo apply \
  --request split.json --preview --json

codesplice --workspace /path/to/repo apply \
  --request split.json --commit \
  --expect-plan sha256:PREVIEWED_PLAN --json
```

An agent never uses `--accept-current-plan`. If the expected plan changes, the
agent inspects and previews again rather than bypassing the digest precondition.
It treats recovery-required and conflict outcomes as failures needing explicit
inspection; it does not reproduce selected source bytes by hand as a fallback.

In `v0.1.0`, this workflow is available for plans with up to 100 changed
targets. Every candidate is prepared before mutation, targets commit in normalized
path order, and explicit recovery completes forward or rolls back in reverse order.
The commit is recoverable but not atomically visible across files.

The Phase 10 qualification pilot executed all 15 release scenarios with this
workflow on Linux x86_64/ext4 and macOS arm64/APFS. Negative path, stale-input,
and mismatch scenarios stopped at their documented rejection boundary; no pilot
invocation used `--accept-current-plan`.
