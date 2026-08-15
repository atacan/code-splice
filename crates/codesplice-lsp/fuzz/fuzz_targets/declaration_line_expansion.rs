#![no_main]

use codesplice_core::ByteRange;
use codesplice_lsp::symbols::{SelectionExtent, apply_extent};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let snapshot = String::from_utf8_lossy(data);
    let Some(range) = nonempty_char_boundary_range(&snapshot, data) else {
        return;
    };

    let Ok(expanded) = apply_extent(&snapshot, range, SelectionExtent::DeclarationLines) else {
        return;
    };
    assert!(expanded.start <= range.start);
    assert!(expanded.end >= range.end);
    assert!(expanded.end <= snapshot.len() as u64);
    assert!(snapshot.is_char_boundary(expanded.start as usize));
    assert!(snapshot.is_char_boundary(expanded.end as usize));
    assert_eq!(
        apply_extent(&snapshot, expanded, SelectionExtent::Symbol),
        Ok(expanded)
    );
});

fn nonempty_char_boundary_range(snapshot: &str, data: &[u8]) -> Option<ByteRange> {
    if snapshot.is_empty() {
        return None;
    }
    let length = snapshot.len();
    let mut start = read_usize(data, 0) % length;
    while start < length && !snapshot.is_char_boundary(start) {
        start += 1;
    }
    if start == length {
        start = 0;
    }

    let remaining = length - start;
    let mut end = start + 1 + read_usize(data, 8) % remaining;
    while end < length && !snapshot.is_char_boundary(end) {
        end += 1;
    }
    Some(ByteRange {
        start: start as u64,
        end: end as u64,
    })
}

fn read_usize(data: &[u8], offset: usize) -> usize {
    let mut bytes = [0_u8; size_of::<usize>()];
    let available = data.get(offset..).unwrap_or_default();
    let count = available.len().min(bytes.len());
    bytes[..count].copy_from_slice(&available[..count]);
    usize::from_le_bytes(bytes)
}
