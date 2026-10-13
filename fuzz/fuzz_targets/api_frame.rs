//! Owned by: WFUZZ-seams
//! Bounded daemon JSON-line request framing.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|bytes: &[u8]| {
    let mut input = bytes;
    let _ = tidemarkd::api::read_frame(&mut input, &mut tidemarkd::api::FrameDecoder::default());
});
