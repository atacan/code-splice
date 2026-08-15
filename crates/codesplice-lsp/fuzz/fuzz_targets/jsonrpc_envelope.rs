#![no_main]

use codesplice_lsp::jsonrpc::{EnvelopeLimits, decode_body};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let _ = decode_body(data, EnvelopeLimits::default());
});
