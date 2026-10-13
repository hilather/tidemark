//! Owned by: WFUZZ-seams
//! Fanotify bytes without descriptor ownership.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|bytes: &[u8]| {
    for record in tidemark_sys::fanotify::parse_event_buffer(bytes) {
        let _ = record;
    }
});
