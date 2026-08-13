#![no_main]

use codesplice_lsp::jsonrpc::{EnvelopeLimits, FrameDecoder, FramingLimits, decode_body};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    exercise_decoder(data, data.len().max(1));

    // Exercise terminators and body boundaries split across arbitrary reads.
    let fragmented_chunk_size = data.first().map_or(1, |byte| usize::from(*byte % 31) + 1);
    exercise_decoder(data, fragmented_chunk_size);
});

fn exercise_decoder(data: &[u8], chunk_size: usize) {
    let mut decoder = FrameDecoder::new(FramingLimits::default());
    for chunk in data.chunks(chunk_size) {
        let Ok(bodies) = decoder.push(chunk) else {
            return;
        };
        for body in bodies {
            let _ = decode_body(&body, EnvelopeLimits::default());
        }
    }
    let _ = decoder.finish();
}
