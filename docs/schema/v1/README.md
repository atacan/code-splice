# Protocol version 1 schemas

`request.schema.json` and `response.schema.json` are the normative JSON Schema
Draft 2020-12 contracts for CodeSplice protocol version 1 during the v0.1.0
implementation phases. Runtime validation additionally enforces byte-count limits,
duplicate-key rejection, selector ordering, and the maximum nesting depth because
those constraints are not portably expressible in JSON Schema.
