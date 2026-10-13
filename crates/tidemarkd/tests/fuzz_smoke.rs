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
        assert_not_quadratic(start, bytes.len());
    }
    let deep = format!("{}0{}", "[".repeat(512), "]".repeat(512));
    exercise(deep.as_bytes());
}

fn assert_not_quadratic(start: Instant, bytes: usize) {
    // Catch accidental quadratic blowups, with generous wall-clock slack for loaded CI hosts.
    let elapsed = start.elapsed();
    assert!(
        elapsed < Duration::from_secs(30),
        "{bytes} bytes took {elapsed:?}"
    );
}

#[test]
fn repeated_seed_scales_without_quadratic_blowup() {
    let seed = b" ".as_slice();
    let minimum = |size: usize| {
        let mut bytes = seed.repeat(size.div_ceil(seed.len()));
        bytes.truncate(size);
        let mut minimum = Duration::MAX;
        for _ in 0..3 {
            let start = Instant::now();
            let _: Result<tidemarkd::firstboot::FirstbootReport, _> =
                serde_json::from_slice(std::hint::black_box(&bytes));
            let elapsed = start.elapsed();
            assert_not_quadratic(start, bytes.len());
            minimum = minimum.min(elapsed);
        }
        minimum
    };
    let small = minimum(64 * 1024);
    let large = minimum(1024 * 1024);
    // Linear growth is 16x; quadratic growth is 256x. Ignore sub-millisecond noise.
    if small >= Duration::from_millis(1) {
        assert!(
            large < small * 200,
            "64 KiB took {small:?}; 1 MiB took {large:?}"
        );
    }
}
