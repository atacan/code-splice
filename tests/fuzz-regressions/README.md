# Phase 9 fuzz regressions

These seeds are permanent regression cases discovered or selected during the
bounded Phase 9 campaigns. The crate-local `hardening` tests also run generated
property cases against JSON decoding, line indexing, coordinate resolution,
event composition, deterministic CBOR validation, transaction-record decoding,
state folding, recovery classification, diff decoding, and terminal escaping.

Run the checked-in regression set with:

```bash
cargo test --workspace --all-features fuzz_regression
```

Run the bounded generated campaign with:

```bash
PROPTEST_CASES=4096 cargo test --workspace --all-features fuzz_regression
```

The campaign is deterministic enough for regression use through proptest's
checked-in source cases, has bounded input sizes, and persists any future minimal
failure in the relevant crate's `proptest-regressions` file.
