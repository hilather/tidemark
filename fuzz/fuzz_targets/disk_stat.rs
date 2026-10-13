//! Owned by: W7-C3-fuzz
//! Arbitrary-input parser target.
#![no_main]
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let _ = tidemark_fs::blockdev::parse_disk_stat(&String::from_utf8_lossy(data));
});
