# Phase 9 checkpoint

Status: PASS
Commit: not committed

## Delivered

- A documented, bounded 4,096-case property campaign over JSON and record
  decoders, line indexing, selector/anchor planning and shared-offset event
  composition, deterministic CBOR, state folding, recovery classification,
  text-diff decoding, and terminal/bidirectional escaping. Fixed regression
  inputs are checked in under `tests/fuzz-regressions`.
- Systematic fresh-process crash coverage on both sides of record publication,
  candidate preparation, every forward target index, final verification, every
  reverse rollback index, terminal publication, and cleanup. Each fixture
  resolves to all-original, all-planned, or an explicit conflict.
- Below/at/above tests for every documented request, response, snapshot,
  planning, transaction, and diff resource limit, using lowerable accounting
  limits where allocating the release maximum would be wasteful.
- Runtime filesystem qualification restricted to Linux x86_64/ext4 and macOS
  arm64/APFS, including representative network/virtual filesystem rejection,
  workspace/control/target device checks, and a native no-replace collision test
  that proves neither entry is overwritten.
- A zero-direct-unsafe audit with a portable `rg`/`grep` source scan and
  `RUSTFLAGS=-Funsafe-code` compilation. The no-replace boundary remains rustix's
  reviewed safe wrapper; no direct unsafe shim was introduced.
- One shared platform qualification script required by both CI matrix rows. It
  runs integration and crash suites, fuzz regressions, the unsafe audit,
  AddressSanitizer, and macOS or glibc allocator diagnostics with temporary
  workspaces pinned to the qualified filesystem.
- Reproducible release-profile performance tooling and measured 1 MiB, 100 MiB,
  1/10/100-target, 1/1,000/100,000-segment, and limit-rejection results in
  `docs/performance-baseline.json`, with no invented timing thresholds.
- Explicit platform, qualification, unsafe-audit, threat-model, metadata-loss,
  and resource-boundary documentation. The support table names only the two
  configurations actually exercised.

## Verification

- Phase 8 base `6f1ee999f86cd00a65ea8302cb8187dc66526044` — verified as the
  initial `HEAD`, a valid commit object, and an ancestor of the Phase 9 work.
- `cargo fmt --all --check` — pass
- `cargo clippy --workspace --all-targets --all-features -- -D warnings` — pass
- `cargo test --workspace --all-features` — pass (155 tests)
- `cargo build --workspace --all-features` — pass
- `scripts/run-fuzz-regressions.sh` — pass with `PROPTEST_CASES=4096`
- `scripts/audit-unsafe.sh` — pass; zero direct unsafe findings and all targets
  accepted `-Funsafe-code`
- `scripts/qualify-platform.sh` on macOS arm64/APFS — pass, including the full
  test/crash sequence, AddressSanitizer, and allocator diagnostics
- Linux x86_64/ext4 qualification in a local QEMU-emulated Docker runtime — all
  shared-script functional, platform, fuzz, crash, unsafe, and AddressSanitizer
  commands passed; the glibc heap-diagnostic command passed in a focused run
  against the same retained ext4 checkout after stopping a stale QEMU process
- `RUSTC_BOOTSTRAP=1 RUSTFLAGS='-Zsanitizer=address' PROPTEST_CASES=512 cargo
  test --workspace --all-features fuzz_regression --target
  aarch64-apple-darwin` — pass through the macOS qualification script
- Equivalent `x86_64-unknown-linux-gnu` AddressSanitizer campaign — pass through
  the Linux qualification sequence
- `cargo run --release -p codesplice-cli --example phase9_baseline` — pass and
  measurements recorded
- `bash -n scripts/audit-unsafe.sh scripts/qualify-platform.sh
  scripts/run-fuzz-regressions.sh` — pass

## Demonstrated behavior

- Generated selections always inserted the exact selected bytes and retained the
  selected digest. Repeated planning from one snapshot/request produced identical
  plans, plan digests, and deterministic CBOR bytes.
- Arbitrary line bytes matched an independent boundary model; shared-offset
  insertions matched a request-order reference composition; malformed JSON, CBOR,
  manifest, and state inputs never panicked.
- Preview/read-only regressions left workspaces unchanged, expected-plan
  rejection remained pre-transaction, commits matched every planned length and
  digest, and interrupted recovery remained all-old, all-new, or explicit conflict.
- Native collision tests preserved both source and destination. Replacing a
  candidate with equal bytes but a distinct identity was rejected on both APFS
  and ext4; the fixture was made inode-reuse-independent after ext4 exposed that
  test portability defect.
- Measured macOS arm64/APFS medians were 3.499 ms for 1 MiB planning and 358.400
  ms for 100 MiB; 1/10/100-target planning measured 0.002/0.013/0.132 ms;
  1/1,000/100,000-segment encoding measured 0.000/0.007/0.650 ms. These values
  are informational observations, not gates.

## Decisions made within phase authority

- Keep the mutation allowlist exact: ext4 on Linux and APFS on macOS. Detection
  remains fail-closed for every other filesystem rather than inferring safety
  from a local/network label.
- Retain zero direct unsafe Rust by using rustix's safe native no-replace API; the
  audit documents dependency internals as outside the direct-source boundary.
- Prebuild the ordinary fuzz binaries after the sanitizer run and before enabling
  allocator diagnostics, so compiler subprocesses are not themselves perturbed.
- Treat the first performance observations as a reproducible baseline only; Phase
  9 adds no aspirational latency threshold.

## Deviations or concerns

- The local Linux row was an x86_64 QEMU runtime backed by ext4 rather than
  physical x86_64 hardware. One redundant end-to-end rerun stalled in QEMU while
  Cargo rebuilt an instrumented dependency and was stopped; every qualification
  component passed either earlier in the shared sequence or in the focused rerun,
  and native Linux CI continues to run the unchanged shared script.
- macOS's sandboxed Xcode SDK discovery emitted FSEvents/cache-directory warnings;
  compilation, sanitizers, and tests all completed successfully.

## Next phase readiness

- Ready. Phase 10 has not been started.
