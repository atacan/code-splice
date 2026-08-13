# Resource limits

Version `0.1.0` uses the following maximum defaults. Trusted local configuration
may lower them but may not raise them. Every layer charges the limit when it first
knows the value and uses checked arithmetic before allocation or I/O.

| Resource | Limit |
|---|---:|
| JSON request bytes | 4 MiB |
| JSON nesting depth | 64 |
| Serialized JSON response | 16 MiB |
| Operations per batch | 1,000 |
| Distinct operation paths | 1,000 |
| UTF-8 relative path | 4,096 bytes |
| Individual snapshot file | 256 MiB |
| Total snapshot bytes | 1 GiB |
| Total line count | 5,000,000 |
| Line-index memory | 256 MiB |
| Total charged planning memory | 2 GiB |
| Resulting bytes per output | 512 MiB |
| Total planned output bytes | 1 GiB |
| Segments per output | 100,000 |
| Total segments | 250,000 |
| Changed transaction targets | 100 |
| Projected transaction disk use | 3 GiB |
| Manifest or state record | 16 MiB |
| State records per transaction | 512 |
| Cumulative state-record bytes | 128 MiB |
| Transaction directories scanned | 100 |
| Recovery bytes read per command | 256 MiB |
| Human or JSON diff bytes | 4 MiB |

Detailed diff input is limited to 8 MiB per side and 10,000,000 explicitly
counted input-and-render work units. The linear-memory text comparison removes a
common exact prefix and suffix and reports the changed middle with original
terminator labels; it never allocates a quadratic matrix. Both human text and the
JSON-encoded diff string are capped at 4 MiB. Diff truncation never changes the
plan digest or commit.

An opt-in review summary is complete or preview fails; its rows are never
truncated. Indexed selected-range metrics and composable output-segment metrics
avoid rescanning or materializing output bytes solely for line counts. The
planner's conservative response projection charges 1,024 bytes per operation,
512 bytes per output, and 512 bytes per output segment, covering the
nonduplicative summary supplements and insertion groups. Summary metadata is
reserved within the 16 MiB serialized-response limit before the remaining budget
is assigned to detailed diff text. A response-driven reduction uses the existing
`DIFF_TRUNCATED` warning with reason `response_budget`.

Phase 4 charges every resulting output byte and retained segment. Changed-target
count uses byte classification, so an effectful but byte-identical output is not a
target. Planning memory excludes immutable snapshot storage and charges retained
operation/output records, segment enums, and owned path bytes. The projected
response charge is a conservative structural bound of 1,024 base bytes, 1,024
bytes plus paths per resolved operation, 512 bytes plus path per output, and 512
bytes per segment; Phase 6 additionally enforces the exact serialized response
limit.

Phase 5 bounds the complete binary envelope for each manifest or state record at
16 MiB, validates no more than 100 targets and 512 contiguous state snapshots,
and caps cumulative state records at 128 MiB. A diagnostic scan visits at most 100
active-plus-completed transaction directories and reads at most 256 MiB across
records and authorized artifacts. Checked arithmetic precedes every cumulative
charge. Persisted data beyond a structural bound is rejected rather than partially
interpreted or guessed through.

## Boundary verification

Every row above has below/at/above coverage using lowerable accounting limits or
numeric accounting test doubles, so the suite does not allocate the release
maximum merely to prove comparison behavior:

| Layer | Covered resources | Regression test |
|---|---|---|
| Protocol | request bytes, depth, operations, paths, path bytes | `request_resource_boundaries_should_fail_at_the_first_exceeded_limit` |
| CLI | serialized response bytes | `phase9_serialized_response_limit_covers_below_at_and_above_boundaries` |
| Snapshot | file/total bytes, identities, lines, index memory, path bytes | `phase9_snapshot_limits_cover_below_at_and_above_every_boundary` |
| Planner | output/total bytes, per-output/total segments, targets, response projection, planning memory | `planner_enforces_every_phase_four_resource_boundary` |
| Transaction | record bytes, targets, state count/bytes, directories, recovery bytes, projected disk | `journal_limits_should_pass_at_and_reject_below_each_known_schema_boundary` and `journal_scan_limits_should_reject_below_directory_recovery_and_state_byte_usage` |
| Diff | detailed input, work units, human/JSON output bytes | `phase9_diff_limits_cover_below_at_and_above_boundaries` |
