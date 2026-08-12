# Phase 9 qualification

The complete local qualification entry point is:

```bash
scripts/qualify-platform.sh
```

It rejects any host outside Linux x86_64/ext4 and macOS arm64/APFS before
running all integration tests, the 4,096-case bounded fuzz/property campaign,
the single- and multi-target subprocess crash matrices, the unsafe-code audit,
AddressSanitizer, and platform heap diagnostics. The pinned stable compiler runs
the sanitizer instrumentation with `RUSTC_BOOTSTRAP=1`; this is qualification
tooling and does not affect shipped builds. Linux also uses glibc `MALLOC_CHECK_=3` and
`MALLOC_PERTURB_=165`; macOS uses `MallocScribble`, `MallocPreScribble`, and
`MallocGuardEdges`. These are the platform equivalents used for a codebase that
forbids direct unsafe Rust in every workspace crate.

The bounded campaign exercises:

- arbitrary JSON bytes and terminal/bidirectional escaping;
- arbitrary line bytes plus selector, anchor, and shared-offset event recipes;
- deterministic CBOR encoding and a strict bounded decoder;
- arbitrary manifest/state record bytes;
- arbitrary state transitions and recovery observations; and
- arbitrary text-diff bytes.

Inputs are capped between 512 bytes and 8 KiB by surface. The campaign has no
time-dependent oracle, network input, or unbounded allocation. Permanent cases
are in `tests/fuzz-regressions`, and future minimized proptest failures are
checked in beside their owning test.

The crash matrix launches a fresh process at both sides of manifest/state record
publication, candidate creation/write/sync/verification, every target backup and
install index, final verification, reverse rollback steps, terminal rename, and
cleanup. Each fixture must resolve to all-original, all-planned, or an explicit
conflict without overwriting an external entry.

Performance methodology and observed release-profile measurements are recorded
in `performance-baseline.json`. Phase 9 establishes no timing threshold.
