//! Owned by: W7-C3-fuzz
//! Arbitrary-input parser target.
#![no_main]
#[path = "../support/deploy.rs"]
mod support;
use libfuzzer_sys::fuzz_target;
fuzz_target!(|data: &[u8]| {
    let _ = serde_json::from_slice::<tidemark_cli::deploy::StagedRecord>(data);
    support::exercise(data);
});
