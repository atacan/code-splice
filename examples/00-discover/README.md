# 00 — Discover the installed CLI

Start here to confirm which binary is running and ask it for its frozen protocol
surface. These queries do not require `--workspace` and do not touch files.

```bash
srcmv --version
srcmv capabilities --json
srcmv protocol-version --json
```

Run all three and retain their output under `examples/.work/00-discover/reports/`:

```bash
examples/run.sh 00-discover
```

For `v0.1.0`, capabilities report protocol version 1, implementation phase 10,
both operations, both selectors, all five anchors, both preconditions, and all
five feature flags enabled.
