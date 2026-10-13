//! Owned by: WFUZZ-seams
//! Bounded daemon JSON-line request framing.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|mut bytes: &[u8]| {
    let _ = tidemarkd::api::read_frame(
        &mut bytes,
        &mut tidemarkd::api::FrameDecoder::default(),
    );
});
