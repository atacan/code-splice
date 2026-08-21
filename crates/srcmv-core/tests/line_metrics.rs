//! Property coverage for indexed and streaming line metrics.

use srcmv_core::{ByteRange, LineIndex, LineMetrics};
use proptest::prelude::*;

fn reference_line_count(bytes: &[u8]) -> u64 {
    let mut terminators = 0_u64;
    let mut offset = 0;
    while offset < bytes.len() {
        match bytes[offset] {
            b'\r' if bytes.get(offset + 1) == Some(&b'\n') => {
                terminators += 1;
                offset += 2;
            }
            b'\r' | b'\n' => {
                terminators += 1;
                offset += 1;
            }
            _ => offset += 1,
        }
    }
    if bytes
        .last()
        .is_some_and(|byte| !matches!(byte, b'\r' | b'\n'))
    {
        terminators += 1;
    }
    terminators
}

proptest! {
    #[test]
    fn indexed_range_metrics_match_a_standalone_reference_scan_for_every_subrange(
        bytes in prop::collection::vec(any::<u8>(), 0..64),
    ) {
        let index = LineIndex::from_bytes_with_limits(&bytes, u64::MAX, u64::MAX)
            .expect("bounded generated input should index");

        for start in 0..=bytes.len() {
            for end in start..=bytes.len() {
                let actual = index
                    .metrics_for_range(
                        &bytes,
                        ByteRange {
                            start: start as u64,
                            end: end as u64,
                        },
                    )
                    .expect("every generated subrange should be valid");
                let selected = &bytes[start..end];

                prop_assert_eq!(actual.byte_count(), selected.len() as u64);
                prop_assert_eq!(actual.line_count(), reference_line_count(selected));
            }
        }
    }

    #[test]
    fn two_and_three_chunk_composition_match_every_split_of_the_concatenated_bytes(
        bytes in prop::collection::vec(any::<u8>(), 0..48),
    ) {
        let expected = LineMetrics::try_from_bytes(&bytes)
            .expect("bounded generated input should be representable");
        let index = LineIndex::from_bytes_with_limits(&bytes, u64::MAX, u64::MAX)
            .expect("bounded generated input should index");

        for first_split in 0..=bytes.len() {
            let mut two_chunks = LineMetrics::default();
            two_chunks
                .try_append_bytes(&bytes[..first_split])
                .expect("first generated chunk should append");
            two_chunks
                .try_append_bytes(b"")
                .expect("empty chunk should append");
            two_chunks
                .try_append_bytes(&bytes[first_split..])
                .expect("second generated chunk should append");
            prop_assert_eq!(two_chunks, expected);

            for second_split in first_split..=bytes.len() {
                let ranges = [
                    ByteRange {
                        start: 0,
                        end: first_split as u64,
                    },
                    ByteRange {
                        start: first_split as u64,
                        end: second_split as u64,
                    },
                    ByteRange {
                        start: second_split as u64,
                        end: bytes.len() as u64,
                    },
                ];
                let mut indexed_chunks = LineMetrics::default();
                for range in ranges {
                    indexed_chunks
                        .try_append(
                            index
                                .metrics_for_range(&bytes, range)
                                .expect("generated chunk range should be valid"),
                        )
                        .expect("generated range summary should append");
                    indexed_chunks
                        .try_append(LineMetrics::default())
                        .expect("empty summary should append");
                }
                prop_assert_eq!(indexed_chunks, expected);
            }
        }
    }

    #[test]
    fn independently_indexed_segment_summaries_match_materialized_output(
        chunks in prop::collection::vec(
            prop::collection::vec(any::<u8>(), 0..32),
            0..8,
        ),
    ) {
        let materialized = chunks.concat();
        let expected = LineMetrics::try_from_bytes(&materialized)
            .expect("bounded materialized output should be representable");
        let mut actual = LineMetrics::default();

        for chunk in &chunks {
            let index = LineIndex::from_bytes_with_limits(chunk, u64::MAX, u64::MAX)
                .expect("bounded generated segment should index");
            let summary = index
                .metrics_for_range(
                    chunk,
                    ByteRange {
                        start: 0,
                        end: chunk.len() as u64,
                    },
                )
                .expect("complete generated segment range should be valid");
            actual
                .try_append(summary)
                .expect("generated segment summary should append");
        }

        prop_assert_eq!(actual, expected);
        prop_assert_eq!(actual.line_count(), reference_line_count(&materialized));
    }
}
