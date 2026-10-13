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
fn exercise(bytes: &[u8]) {
    let text = String::from_utf8_lossy(bytes);
    let _: Result<_, tidemark_fs::ParseError> = tidemark_fs::mount::parse_mountinfo(bytes);
    let _: Option<_> = tidemark_fs::blockdev::parse_disk_stat(&text);
    let _: Option<_> = tidemark_fs::procfs::parse_cgroup_unit(&text);
    let _: Option<_> = tidemark_fs::procfs::parse_fdinfo_flags(&text);
    let _: Option<_> = tidemark_fs::procfs::parse_stat_starttime(&text);
    let _: Result<_, tidemark_fs::LvmError> = tidemark_fs::lvm::parse_lvs_json(bytes);
    let _: Result<_, tidemark_fs::LvmError> = tidemark_fs::lvm::parse_vgs_json(bytes);
}
const SEEDS: &[&[u8]] = &[
    include_bytes!("w1a/data/mountinfo"),
    include_bytes!("w1d/data/cgroup-v1.txt"),
    include_bytes!("w1d/data/cgroup-v2.txt"),
    include_bytes!("w1d/data/fdinfo.txt"),
    include_bytes!("w1d/data/stat.txt"),
    include_bytes!("../../test-support/fixtures/lvm/lvs.json"),
    include_bytes!("../../test-support/fixtures/lvm/vgs.json"),
    b"1 2 3 4 5 6 7 8 9 10 11",
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
#[test]
fn documented_size_limits_refuse_without_parsing() {
    let bytes = vec![b' '; 16 * 1024 * 1024 + 1];
    let start = Instant::now();
    assert!(tidemark_fs::mount::parse_mountinfo(&bytes).is_err());
    assert!(tidemark_fs::lvm::parse_lvs_json(&bytes).is_err());
    assert!(tidemark_fs::lvm::parse_vgs_json(&bytes).is_err());
    assert!(tidemark_fs::blockdev::parse_disk_stat(&" ".repeat(65537)).is_none());
    assert_not_quadratic(start, bytes.len());
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
    let seed = include_bytes!("w1a/data/mountinfo").as_slice();
    let minimum = |size: usize| {
        let mut bytes = seed.repeat(size.div_ceil(seed.len()));
        bytes.truncate(size);
        let mut minimum = Duration::MAX;
        for _ in 0..3 {
            let start = Instant::now();
            let _ = tidemark_fs::mount::parse_mountinfo(std::hint::black_box(&bytes));
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
