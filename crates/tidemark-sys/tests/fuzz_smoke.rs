//! Owned by: WFUZZ-seams
//! Non-owning fanotify decoder generated properties and scaling guards.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
#[path = "../../../fuzz/support/seams.rs"]
mod seams;
use tidemark_sys::fanotify::parse_event_buffer;
const SEED: &[u8] = include_bytes!("../../../fuzz/corpus/fanotify_events/sample");
fn exercise(bytes: &[u8]) {
    let mut previous = 0;
    let mut records = parse_event_buffer(bytes);
    let mut count = 0;
    while let Some(result) = records.next() {
        count += 1;
        assert!(count <= bytes.len() / 24 + 1);
        match result {
            Ok(record) => {
                assert_eq!(record.version, 3);
                assert!(record.information.start >= previous + 24);
                assert!(record.information.end <= bytes.len());
                previous += record.event_len as usize;
                assert_eq!(record.information.end, previous);
            }
            Err(_) => assert!(records.next().is_none()),
        }
    }
    assert!(records.next().is_none());
}
#[test]
fn arbitrary_bytes_and_mutated_valid_records() {
    seams::generated(
        exercise,
        &[
            SEED,
            include_bytes!("../../../fuzz/corpus/fanotify_events/fid"),
            include_bytes!("../../../fuzz/corpus/fanotify_events/dfid_name"),
        ],
    );
}
#[test]
fn valid_record_round_trip_and_refusals() {
    let record = parse_event_buffer(SEED).next().unwrap().unwrap();
    assert_eq!(record.event_len, 24);
    assert_eq!(record.metadata_len, 24);
    assert_eq!(record.mask, 8);
    assert_eq!(record.raw_fd, 123456);
    assert_eq!(record.pid, 42);
    assert_eq!(record.information, 24..24);
    for len in [0u32, 23, 25, 32, u32::MAX] {
        let mut bytes = SEED.to_vec();
        bytes[..4].copy_from_slice(&len.to_ne_bytes());
        assert!(parse_event_buffer(&bytes).next().unwrap().is_err());
    }
    for meta in [0u16, 23, 25, 32, u16::MAX] {
        let mut bytes = SEED.to_vec();
        bytes[6..8].copy_from_slice(&meta.to_ne_bytes());
        assert!(parse_event_buffer(&bytes).next().unwrap().is_err());
    }
}
#[test]
fn large_inputs_and_scaling() {
    seams::scaling(SEED, exercise);
    for byte in [0, 255] {
        seams::timed(&vec![byte; 1024 * 1024], exercise);
    }
}

#[test]
fn pure_parser_never_closes_a_real_descriptor() {
    use std::os::fd::AsRawFd;
    let file = std::fs::File::open("/").unwrap();
    let mut bytes = SEED.to_vec();
    bytes[16..20].copy_from_slice(&file.as_raw_fd().to_ne_bytes());
    seams::generated(exercise, &[&bytes]);
    assert!(file.metadata().unwrap().is_dir());
}
