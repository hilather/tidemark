//! Owned by: W7-C3-fuzz
//! Arbitrary-input parser target.
#![no_main]
#[path = "../support/firstboot.rs"]
mod support;
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    support::exercise(data);
});
