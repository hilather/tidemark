//! Owned by: W7-C3-fuzz
//! Deterministic state-reader smoke coverage (this crate lacks proptest).
#![forbid(unsafe_code)]
use std::time::{Duration, Instant};
#[path = "../../../fuzz/support/firstboot.rs"]
mod support;
use support::exercise;
#[test]
fn mutated_reports_and_large_inputs() {
    let seed = br#"{"version":"test","config_sha256":"abc","daemon_started_at":1,"components":[],"watch_modes":[],"first_fs_sample":null,"first_lvm_sample":null,"next_scans":[],"booted_at":0,"healthy":false,"written_at":2}"#;
    exercise(seed);
    for cut in 0..seed.len() {
        exercise(&seed[..cut]);
    }
    for i in 0..seed.len() {
        let mut bytes = seed.to_vec();
        bytes[i] ^= 255;
        exercise(&bytes);
    }
    for byte in [0, 255, b'9', b' '] {
        let bytes = vec![byte; 1024 * 1024 + 1];
        let start = Instant::now();
        exercise(&bytes);
        assert!(start.elapsed() < Duration::from_secs(2));
    }
    let deep = format!("{}0{}", "[".repeat(512), "]".repeat(512));
    exercise(deep.as_bytes());
}
