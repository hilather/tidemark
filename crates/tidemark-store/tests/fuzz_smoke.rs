//! Owned by: W7-C3-fuzz
//! Deterministic adversarial parser smoke tests.
#![forbid(unsafe_code)]
use proptest::prelude::*;
use proptest::test_runner::{Config as TestConfig, RngSeed};
use std::time::{Duration, Instant};
fn config() -> TestConfig {
    TestConfig {
        cases: 48,
        rng_seed: RngSeed::Fixed(0x57374333),
        max_shrink_iters: 1024,
        ..TestConfig::default()
    }
}
fn mutated(seed: &[u8], cut: usize, byte: u8, mode: u8) -> Vec<u8> {
    let mut out = seed.to_vec();
    match mode % 5 {
        0 => out.truncate(cut % (out.len() + 1)),
        1 => {
            if !out.is_empty() {
                let i = cut % out.len();
                out[i] ^= byte;
            }
        }
        2 => out.extend_from_slice(seed),
        3 => {
            let i = cut % (out.len() + 1);
            out.splice(i..i, std::iter::repeat_n(byte, cut % 4096));
        }
        _ => {
            let i = cut % (out.len() + 1);
            out.splice(i..i, [0, 255, byte]);
        }
    }
    out
}
#[path = "support/catalog.rs"]
mod catalog;
fn exercise(bytes: &[u8]) {
    catalog::exercise(bytes);
}
const SEEDS: &[&[u8]] = &[
    b"snapshots/root/test.db",
    b"/old/state/snapshots/root/test.db",
    b"../escape.db",
    b"./snapshots/a/../b",
    b"/",
    b"nul\0bad",
    b"invalid\xff",
];
proptest! {
    #![proptest_config(config())]
    #[test]
    fn arbitrary_bytes(bytes in prop::collection::vec(any::<u8>(), 0..8192)) { exercise(&bytes); }
    #[test]
    fn near_valid(index in 0usize..SEEDS.len(), cut in any::<usize>(), byte in any::<u8>(), mode in any::<u8>()) {
        exercise(&mutated(SEEDS[index], cut, byte, mode));
    }
}
#[test]
fn large_and_deep_inputs_finish_without_quadratic_blowup() {
    let mut cases = vec![
        vec![b' '; 1024 * 1024],
        vec![b'9'; 1024 * 1024],
        vec![0; 1024 * 1024],
        vec![255; 1024 * 1024],
    ];
    let mut deep = vec![b'['; 512];
    deep.extend(std::iter::repeat_n(b']', 512));
    cases.push(deep);
    cases.push(format!("deep = {}0{}", "[".repeat(512), "]".repeat(512)).into_bytes());
    for seed in SEEDS {
        cases.push(seed.repeat((1024 * 1024 / seed.len()).max(1)));
    }
    for bytes in cases {
        let start = Instant::now();
        exercise(&bytes);
        assert_not_quadratic(start, bytes.len());
    }
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
    let seed = b"a/".as_slice();
    let minimum = |size: usize| {
        let mut bytes = seed.repeat(size.div_ceil(seed.len()));
        bytes.truncate(size);
        let mut minimum = Duration::MAX;
        for _ in 0..3 {
            let start = Instant::now();
            exercise(std::hint::black_box(&bytes));
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
