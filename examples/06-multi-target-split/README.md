# 06 — Split one file into three targets

Two moves select adjacent lines from one initial source and create two new files.
The single commit changes three targets: the source plus both destinations.

```bash
examples/run.sh 06-multi-target-split
```

CodeSplice prepares every candidate before the first target changes and commits
targets in normalized path order. Visibility across several files is recoverable,
not atomic: an unrelated reader may briefly observe mixed old/new files.

