//! Owned by: WFUZZ-seams
//! Dependency-free deterministic input generation and generous timing guards.
#![allow(dead_code)]
use std::time::{Duration, Instant};

/// Exercise deterministic arbitrary bytes and exhaustive mutations of supplied seeds.
pub fn generated(mut exercise: impl FnMut(&[u8]), seeds: &[&[u8]]) {
    let mut state = 0x46555a5a_u64;
    for length in (0..256).chain([512, 1024, 4096, 8192]) {
        let bytes: Vec<_> = (0..length)
            .map(|_| {
                state ^= state << 13;
                state ^= state >> 7;
                state ^= state << 17;
                state as u8
            })
            .collect();
        exercise(&bytes);
    }
    for seed in seeds {
        exercise(seed);
        for cut in 0..seed.len() {
            exercise(&seed[..cut]);
        }
        for at in 0..seed.len() {
            for byte in [0, 1, 127, 255] {
                let mut bytes = seed.to_vec();
                bytes[at] ^= byte;
                exercise(&bytes);
            }
        }
        exercise(&seed.repeat(2));
    }
}

/// Observe one parser call with generous CI slack, returning its elapsed duration.
pub fn timed(bytes: &[u8], mut exercise: impl FnMut(&[u8])) -> Duration {
    let start = Instant::now();
    exercise(std::hint::black_box(bytes));
    let elapsed = start.elapsed();
    // Generous slack for loaded CI; this is a regression guard, not a benchmark.
    assert!(
        elapsed < Duration::from_secs(30),
        "{} bytes took {elapsed:?}",
        bytes.len()
    );
    elapsed
}

/// Compare minimum elapsed times for 64 KiB and 1 MiB repeated seed inputs.
pub fn scaling(seed: &[u8], mut exercise: impl FnMut(&[u8])) {
    let mut minimum = |size: usize| {
        let mut bytes = seed.repeat(size.div_ceil(seed.len()));
        bytes.truncate(size);
        (0..3)
            .map(|_| timed(&bytes, &mut exercise))
            .min()
            .unwrap_or(Duration::ZERO)
    };
    let small = minimum(64 * 1024);
    let large = minimum(1024 * 1024);
    if small >= Duration::from_millis(1) {
        assert!(
            large < small * 200,
            "64 KiB took {small:?}; 1 MiB took {large:?}"
        );
    }
}
