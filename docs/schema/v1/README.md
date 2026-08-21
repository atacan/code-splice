# Protocol version 1 schemas

`request.schema.json` and `response.schema.json` are the normative JSON Schema
Draft 2020-12 contracts for srcmv protocol version 1 during the v0.1.0
implementation phases. Runtime validation additionally enforces byte-count limits,
duplicate-key rejection, selector ordering, and the maximum nesting depth because
those constraints are not portably expressible in JSON Schema.

The opt-in `--summary` review metadata uses the response schema's existing open
`diff.summary` extension point. Its independently versioned nested contract is
[`review-summary-v1`](../review-summary-v1/README.md); these protocol-v1 schemas
and all default response shapes remain unchanged.
