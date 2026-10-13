//! Owned by: WFUZZ-seams
//! fdinfo mount identity generated properties and scaling guards.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used)]
#[path = "../../../fuzz/support/seams.rs"]
mod seams;
use tidemark_live::fuzzing::parse_mount_id;
const SEED: &[u8] = include_bytes!("../../../fuzz/corpus/mount_id/sample");
fn exercise(bytes: &[u8]) {
    let _ = parse_mount_id(&String::from_utf8_lossy(bytes));
}
#[test]
fn arbitrary_bytes_and_mutated_valid_fdinfo() {
    seams::generated(exercise, &[SEED, b"mnt_id:\t18446744073709551615\n"]);
}
#[test]
fn valid_ids_round_trip_and_invalid_ids_refuse() {
    assert_eq!(parse_mount_id(&String::from_utf8_lossy(SEED)).unwrap(), 42);
    for id in [0, 1, 42, u64::MAX] {
        assert_eq!(parse_mount_id(&format!("mnt_id:\t{id}\n")).unwrap(), id);
    }
    for text in [
        "",
        "mnt_id:",
        "mnt_id: -1",
        "mnt_id: 18446744073709551616",
        "mnt_id: 4\0",
        " mnt_id: 42",
    ] {
        assert_eq!(
            parse_mount_id(text).unwrap_err().kind(),
            std::io::ErrorKind::InvalidData
        );
    }
    // Preserve the original first-match behavior, including refusal of an invalid first ID.
    assert!(parse_mount_id("mnt_id: bad\nmnt_id: 42").is_err());
    assert_eq!(parse_mount_id("mnt_id: 42\nmnt_id: 99").unwrap(), 42);
}
#[test]
fn large_inputs_and_scaling() {
    // Missing IDs force a scan of the whole buffer rather than an early match.
    seams::scaling(b"flags:\t0100000\n", exercise);
    for byte in [b'9', 0, 255] {
        seams::timed(&vec![byte; 1024 * 1024], exercise);
    }
}
