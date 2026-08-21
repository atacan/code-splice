#![no_main]

use srcmv_core::LineIndex;
use srcmv_lsp::capabilities::SupportedPositionEncoding;
use srcmv_lsp::position::{PositionConverter, PositionLimits};
use gen_lsp_types::{Position, Range};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let snapshot = String::from_utf8_lossy(data);
    let Ok(index) = LineIndex::from_bytes_with_limits(snapshot.as_bytes(), u64::MAX, u64::MAX)
    else {
        return;
    };
    let encoding = match data.first().copied().unwrap_or_default() % 3 {
        0 => SupportedPositionEncoding::Utf8,
        1 => SupportedPositionEncoding::Utf16,
        _ => SupportedPositionEncoding::Utf32,
    };
    let Ok(mut converter) =
        PositionConverter::new(&snapshot, &index, encoding, PositionLimits::default())
    else {
        return;
    };

    let start = Position::new(read_u32(data, 1), read_u32(data, 5));
    let end = Position::new(read_u32(data, 9), read_u32(data, 13));
    if let Ok(byte) = converter.lsp_position_to_byte(start) {
        assert!(byte <= snapshot.len() as u64);
        assert!(snapshot.is_char_boundary(byte as usize));
        if let Ok(round_trip_position) = converter.byte_to_lsp_position(byte) {
            assert_eq!(
                converter.lsp_position_to_byte(round_trip_position),
                Ok(byte)
            );
        }
    }
    let _ = converter.lsp_range_to_byte_range(Range::new(start, end));

    let user_byte = u64::from(read_u32(data, 17));
    let _ = converter.validate_user_byte(user_byte);
    let _ = converter.user_byte_to_lsp_position(user_byte);
    let _ = converter
        .user_line_scalar_to_byte(u64::from(read_u32(data, 21)), u64::from(read_u32(data, 25)));
    let _ = converter.user_line_scalar_to_lsp_position(
        u64::from(read_u32(data, 29)),
        u64::from(read_u32(data, 33)),
    );
});

fn read_u32(data: &[u8], offset: usize) -> u32 {
    let mut bytes = [0_u8; 4];
    let available = data.get(offset..).unwrap_or_default();
    let count = available.len().min(bytes.len());
    bytes[..count].copy_from_slice(&available[..count]);
    u32::from_le_bytes(bytes)
}
