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
counted work units. Diff truncation never changes the plan digest or commit.

Phase 4 charges every resulting output byte and retained segment. Changed-target
count uses byte classification, so an effectful but byte-identical output is not a
target. Planning memory excludes immutable snapshot storage and charges retained
operation/output records, segment enums, and owned path bytes. The projected
response charge is a conservative structural bound of 1,024 base bytes, 1,024
bytes plus paths per resolved operation, 512 bytes plus path per output, and 512
bytes per segment; Phase 6 additionally enforces the exact serialized response
limit.
