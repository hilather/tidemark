//! Owned by: WFUZZ-seams
//! fdinfo mount identity parser.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|bytes: &[u8]| {
    let _ = tidemark_live::fuzzing::parse_mount_id(&String::from_utf8_lossy(bytes));
});
