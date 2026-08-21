//! Exhaustive examples and property-like checks for snapshot position conversion.

use gen_lsp_types::{Position, Range};
use srcmv_core::{ByteRange, LineIndex};
use srcmv_lsp::capabilities::SupportedPositionEncoding;
use srcmv_lsp::position::{PositionConverter, PositionError, PositionLimits};

struct Snapshot {
    text: String,
    index: LineIndex,
}

impl Snapshot {
    fn new(text: impl Into<String>) -> Self {
        let text = text.into();
        let index = LineIndex::from_bytes_with_limits(text.as_bytes(), u64::MAX, u64::MAX)
            .expect("small test snapshot should index");
        Self { text, index }
    }

    fn converter(&self, encoding: SupportedPositionEncoding) -> PositionConverter<'_> {
        PositionConverter::new(&self.text, &self.index, encoding, PositionLimits::default())
            .expect("test index should describe its snapshot")
    }
}

#[test]
fn physical_lines_exclude_lf_crlf_and_lone_cr_from_character_counts() {
    let snapshot = Snapshot::new("a\r\nβ\rc\n🙂");
    let mut converter = snapshot.converter(SupportedPositionEncoding::Utf8);

    let positions = [
        (Position::new(0, 99), 1),
        (Position::new(1, 99), 5),
        (Position::new(2, 99), 7),
        (Position::new(3, 99), 12),
    ];
    for (position, expected) in positions {
        assert_eq!(converter.lsp_position_to_byte(position), Ok(expected));
    }
}

#[test]
fn blank_physical_lines_exist_but_final_terminator_has_no_phantom_line() {
    let snapshot = Snapshot::new("\n\r\n\r");
    let mut converter = snapshot.converter(SupportedPositionEncoding::Utf16);

    assert_eq!(converter.lsp_position_to_byte(Position::new(0, 7)), Ok(0));
    assert_eq!(converter.lsp_position_to_byte(Position::new(1, 7)), Ok(1));
    assert_eq!(converter.lsp_position_to_byte(Position::new(2, 7)), Ok(3));
    assert_eq!(
        converter.lsp_position_to_byte(Position::new(3, 0)),
        Err(PositionError::NonexistentLine)
    );
}

#[test]
fn encodings_count_their_respective_code_units_and_clamp_only_oversized_offsets() {
    let snapshot = Snapshot::new("é🙂z");
    let cases = [
        (SupportedPositionEncoding::Utf8, [0, 2, 6, 7]),
        (SupportedPositionEncoding::Utf16, [0, 1, 3, 4]),
        (SupportedPositionEncoding::Utf32, [0, 1, 2, 3]),
    ];

    for (encoding, characters) in cases {
        let mut converter = snapshot.converter(encoding);
        for (expected_byte, character) in [0_u64, 2, 6, 7].into_iter().zip(characters) {
            assert_eq!(
                converter.lsp_position_to_byte(Position::new(0, character)),
                Ok(expected_byte)
            );
        }
        assert_eq!(
            converter.lsp_position_to_byte(Position::new(0, u32::MAX)),
            Ok(7)
        );
    }
}

#[test]
fn lsp_positions_reject_split_utf8_and_utf16_units() {
    let snapshot = Snapshot::new("é🙂");

    let mut utf8 = snapshot.converter(SupportedPositionEncoding::Utf8);
    assert_eq!(
        utf8.lsp_position_to_byte(Position::new(0, 1)),
        Err(PositionError::CharacterSplitsCodeUnit)
    );

    let mut utf16 = snapshot.converter(SupportedPositionEncoding::Utf16);
    assert_eq!(
        utf16.lsp_position_to_byte(Position::new(0, 2)),
        Err(PositionError::CharacterSplitsCodeUnit)
    );
}

#[test]
fn lsp_ranges_normalize_endpoints_then_reject_empty_reversed_and_missing_lines() {
    let snapshot = Snapshot::new("abc\r\ndef");
    let mut converter = snapshot.converter(SupportedPositionEncoding::Utf8);

    assert_eq!(
        converter.lsp_range_to_byte_range(Range::new(Position::new(0, 1), Position::new(1, 2),)),
        Ok(ByteRange { start: 1, end: 7 })
    );
    assert_eq!(
        converter.lsp_range_to_byte_range(Range::new(
            Position::new(0, u32::MAX),
            Position::new(0, u32::MAX),
        )),
        Err(PositionError::InvalidRange)
    );
    assert_eq!(
        converter.lsp_range_to_byte_range(Range::new(Position::new(1, 1), Position::new(0, 1),)),
        Err(PositionError::InvalidRange)
    );
    assert_eq!(
        converter.lsp_range_to_byte_range(Range::new(Position::new(0, 0), Position::new(2, 0),)),
        Err(PositionError::NonexistentLine)
    );
}

#[test]
fn user_coordinates_are_one_based_exact_and_never_clamped() {
    let snapshot = Snapshot::new("é🙂\r\nx");
    let mut converter = snapshot.converter(SupportedPositionEncoding::Utf16);

    assert_eq!(converter.user_line_scalar_to_byte(1, 1), Ok(0));
    assert_eq!(converter.user_line_scalar_to_byte(1, 2), Ok(2));
    assert_eq!(converter.user_line_scalar_to_byte(1, 3), Ok(6));
    assert_eq!(converter.user_line_scalar_to_byte(2, 1), Ok(8));
    for (line, column) in [(0, 1), (1, 0), (1, 4), (3, 1)] {
        assert_eq!(
            converter.user_line_scalar_to_byte(line, column),
            Err(PositionError::InvalidUserPosition)
        );
    }

    assert_eq!(converter.validate_user_byte(9), Ok(9));
    assert_eq!(
        converter.validate_user_byte(1),
        Err(PositionError::InvalidUserPosition)
    );
    assert_eq!(
        converter.validate_user_byte(10),
        Err(PositionError::InvalidUserPosition)
    );
}

#[test]
fn byte_to_lsp_rejects_terminator_interiors_and_unrepresentable_document_edges() {
    let terminated = Snapshot::new("a\r\n");
    let mut converter = terminated.converter(SupportedPositionEncoding::Utf8);

    assert_eq!(converter.byte_to_lsp_position(1), Ok(Position::new(0, 1)));
    assert_eq!(
        converter.byte_to_lsp_position(2),
        Err(PositionError::ByteNotRepresentable)
    );
    assert_eq!(
        converter.byte_to_lsp_position(3),
        Err(PositionError::ByteNotRepresentable)
    );

    let empty = Snapshot::new("");
    let mut converter = empty.converter(SupportedPositionEncoding::Utf8);
    assert_eq!(converter.validate_user_byte(0), Ok(0));
    assert_eq!(
        converter.byte_to_lsp_position(0),
        Err(PositionError::ByteNotRepresentable)
    );
}

#[test]
fn every_content_boundary_round_trips_for_all_encodings_and_terminators() {
    let content_variants = ["", "a", "é", "🙂", "aé🙂"];
    let terminators = ["", "\n", "\r", "\r\n"];

    for first in content_variants {
        for first_terminator in terminators {
            for second in content_variants {
                let text = format!("{first}{first_terminator}{second}");
                let snapshot = Snapshot::new(text);
                for encoding in [
                    SupportedPositionEncoding::Utf8,
                    SupportedPositionEncoding::Utf16,
                    SupportedPositionEncoding::Utf32,
                ] {
                    let mut converter = snapshot.converter(encoding);
                    for offset in representable_content_boundaries(&snapshot.text) {
                        let position = converter
                            .byte_to_lsp_position(offset)
                            .expect("content boundary should convert to LSP");
                        assert_eq!(converter.lsp_position_to_byte(position), Ok(offset));
                    }
                }
            }
        }
    }
}

#[test]
fn oversized_lsp_characters_canonicalize_to_content_end_for_all_line_endings() {
    for terminator in ["", "\n", "\r", "\r\n"] {
        let snapshot = Snapshot::new(format!("é🙂{terminator}"));
        for encoding in [
            SupportedPositionEncoding::Utf8,
            SupportedPositionEncoding::Utf16,
            SupportedPositionEncoding::Utf32,
        ] {
            let mut converter = snapshot.converter(encoding);
            let byte = converter
                .lsp_position_to_byte(Position::new(0, u32::MAX))
                .expect("oversized character should clamp on an existing line");
            assert_eq!(byte, 6);
            assert_eq!(
                converter
                    .byte_to_lsp_position(byte)
                    .and_then(|position| converter.lsp_position_to_byte(position)),
                Ok(6)
            );
        }
    }
}

#[test]
fn conversion_work_is_cumulative_and_fail_closed() {
    let snapshot = Snapshot::new("abcdef");
    let mut at_limit = PositionConverter::new(
        &snapshot.text,
        &snapshot.index,
        SupportedPositionEncoding::Utf32,
        PositionLimits {
            maximum_code_points_scanned: 2,
        },
    )
    .expect("test index should describe its snapshot");
    assert_eq!(at_limit.lsp_position_to_byte(Position::new(0, 2)), Ok(2));
    assert_eq!(at_limit.code_points_scanned(), 2);

    let mut converter = PositionConverter::new(
        &snapshot.text,
        &snapshot.index,
        SupportedPositionEncoding::Utf16,
        PositionLimits {
            maximum_code_points_scanned: 2,
        },
    )
    .expect("test index should describe its snapshot");

    assert_eq!(converter.lsp_position_to_byte(Position::new(0, 1)), Ok(1));
    assert_eq!(converter.code_points_scanned(), 1);
    assert_eq!(
        converter.lsp_position_to_byte(Position::new(0, 2)),
        Err(PositionError::WorkLimitExceeded)
    );
    assert_eq!(converter.code_points_scanned(), 2);

    let mut utf8 = PositionConverter::new(
        &snapshot.text,
        &snapshot.index,
        SupportedPositionEncoding::Utf8,
        PositionLimits {
            maximum_code_points_scanned: 0,
        },
    )
    .expect("test index should describe its snapshot");
    assert_eq!(utf8.lsp_position_to_byte(Position::new(0, 6)), Ok(6));
    assert_eq!(utf8.code_points_scanned(), 0);
}

#[test]
fn converter_rejects_an_index_with_a_different_snapshot_length() {
    let index = LineIndex::from_bytes_with_limits(b"ab", u64::MAX, u64::MAX)
        .expect("small test snapshot should index");
    let result = PositionConverter::new(
        "abc",
        &index,
        SupportedPositionEncoding::Utf8,
        PositionLimits::default(),
    );

    assert!(matches!(result, Err(PositionError::InvalidSnapshotIndex)));
}

fn representable_content_boundaries(text: &str) -> Vec<u64> {
    let mut boundaries = Vec::new();
    let mut offset = 0_usize;
    while offset < text.len() {
        boundaries.push(offset as u64);
        match text.as_bytes()[offset] {
            b'\r' if text.as_bytes().get(offset + 1) == Some(&b'\n') => offset += 2,
            b'\r' | b'\n' => offset += 1,
            _ => {
                let scalar = text[offset..]
                    .chars()
                    .next()
                    .expect("offset should start a scalar");
                offset += scalar.len_utf8();
                boundaries.push(offset as u64);
            }
        }
    }
    boundaries.sort_unstable();
    boundaries.dedup();

    if text.is_empty() || text.ends_with(['\r', '\n']) {
        boundaries.retain(|offset| *offset != text.len() as u64);
    } else {
        boundaries.push(text.len() as u64);
        boundaries.sort_unstable();
        boundaries.dedup();
    }
    boundaries
}
