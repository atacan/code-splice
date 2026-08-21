# Review summary format version 1

`schema.json` defines the opt-in review metadata introduced by srcmv
`v0.2.0`. Protocol-v1 previews include this complete object at
`diff.summary.review` only when `apply --preview --summary` is requested. The
normative protocol-v1 response schema remains unchanged because `diff.summary`
is its frozen presentation-extension point.

Operation indices join `resolved_operations` in request order. Output indices
join `outputs` in normalized UTF-8 path order. The complete preview response,
not this nonduplicative supplement alone, is the machine review record.
