# 02 — Copy bytes to a new file

This copies the half-open byte range `[7, 15)` (`COPY_ME` plus its LF) to a new file. The
source must exist at its digest; the destination must not exist. A new file may
use `file_start`, `file_end`, or `byte_offset(0)`—all mean offset zero.

The runner sends the checked-in JSON through standard input to demonstrate
`--request -`:

```bash
codesplice --workspace examples/.work/02-copy-bytes-new-file/workspace apply \
  --request - --preview --json < examples/02-copy-bytes-new-file/request.json
```

It then commits with the returned plan digest and verifies the unchanged source
plus the created destination:

```bash
examples/run.sh 02-copy-bytes-new-file
```

CodeSplice creates the file, not its parent directory. The parent must already
exist. A new file receives `0666 & !startup_umask`.
