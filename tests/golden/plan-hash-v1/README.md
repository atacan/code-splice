# Plan-hash version 1 golden vector

`all-discriminants.cbor.hex` is the exact deterministic CBOR payload hashed after
the ASCII domain prefix `CODESPLICE-PLAN-V1\0`. Whitespace and `#` comments are
ignored by the test decoder.

The fixture is intentionally semantic-edge data for the encoder rather than a
planner-valid edit. It diagnoses encoding drift across:

- top-level fields `[version, protocol, workspace, inputs, operations, outputs]`;
- absent state `0` and existing state `1`, sorted as `a.new` then `z.bin`;
- move `1`, copy `2`, changed `1`, and no-op `2` operation values;
- line selector `1`, byte selector `2`, every anchor `1..=5`, and both
  precondition records;
- unchanged `1`, modified `2`, created `3`, and emptied `4` outputs;
- original segment `1` and payload segment `2`; and
- CBOR unsigned-integer boundary widths at 23, 24, 255, 256, 65,535, 65,536,
  2^32, and `u64::MAX`.

The existing file contains `ff 00 80`, proving that file content need not be
UTF-8. Operation 3 is a same-file no-op, while operations 0 and 1 retain the same
destination offset so their request order remains byte-visible in this vector.
