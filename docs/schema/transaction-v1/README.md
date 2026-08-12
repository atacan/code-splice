# Transaction record format version 1

`manifest.schema.json` and `state.schema.json` are the private persisted-payload
schemas frozen at the Phase 5 checkpoint. Both use Draft 2020-12 JSON Schema,
reject unknown fields, and are stored as UTF-8 JSON inside the checksummed binary
record envelope documented in `docs/transaction-model.md`.

The golden payloads and complete envelope bytes are under
`tests/golden/transaction-v1/`. JSON object member ordering is not semantic; the
envelope checksum covers the exact published bytes.

Limits enforced before allocation or publication are 16 MiB per complete record,
100 targets, 512 state records, 128 MiB of cumulative state records, 100 scanned
transaction directories, and 256 MiB read by one recovery command.
