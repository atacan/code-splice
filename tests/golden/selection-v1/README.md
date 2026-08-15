# Semantic selection v1 golden vectors

These hand-authored files freeze representative standalone selection-v1 wire
shapes independently from the edit protocol-v1 golden vectors.

- `composition-selection.json` is a successful name query over
  `composition-workspace`.
- `success-position.json` covers normalized position queries, nullable server
  identity, UTF-16 negotiation, a nested symbol path, and the only warning
  allowed by selection v1.
- `error-not-found.json`, `error-ambiguous.json`, and `error-timeout.json` cover
  conflict, bounded candidate-summary, and retryable support errors.
- `error-registry.json` freezes every selection code's category, process exit
  status, and retryability.

The `request_source` at
`composition-selection.json#/matches/0/request_source` is structurally equal to
`composition-request-source.json` and to
`composition-edit-request.json#/operations/0/source`. The complete edit request
must continue to parse under `docs/schema/v1/request.schema.json` and the frozen
protocol-v1 implementation. Its source and destination digests match the files
under `composition-workspace`, and its selected payload is bytes `[0, 42)` of
`src/input.rs`.
