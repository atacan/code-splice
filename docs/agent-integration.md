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

Through Phase 7, this workflow is available for plans with exactly one changed
target. Cross-file moves and other multi-target plans remain unavailable until
Phase 8.
