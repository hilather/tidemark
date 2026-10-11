//! Owned by: W5-C
//! Pure CLI rendering coverage.
#![allow(clippy::unwrap_used)]
use super::*;
use tidemark_core::live::{PgNow, PgSample, PgSlot};
fn state() -> PgNow {
    PgNow {
        sampled_at: Some(100),
        unreachable_since: None,
        server_version_num: Some(160000),
        databases: vec![("db".into(), 1024)],
        wal_dir_bytes: Some(2048),
        slots: vec![PgSlot {
            name: "slot".into(),
            slot_type: "physical".into(),
            active: false,
            retained_bytes: None,
            wal_status: None,
        }],
        archiver_failing: true,
        archiver_last_failed: Some(90),
        temp_bytes_rate: None,
        top_relations: vec![("db.public.table".into(), 4096)],
    }
}
#[test]
fn text_lists_database_wal_slots_archiver_temp_and_relations() {
    let mut output = Vec::new();
    render(
        &mut output,
        false,
        &state(),
        &[PgSample {
            ts: 100,
            kind: "temp_bytes_total".into(),
            name: "db".into(),
            value: 42,
        }],
    )
    .unwrap();
    let output = String::from_utf8(output).unwrap();
    for expected in [
        "db",
        "WAL directory: 2.00 KiB",
        "Slot slot (physical) active=false retained=unknown",
        "Archiver: failing",
        "temp_bytes_total db: 42",
        "Relation db.public.table: 4.00 KiB",
    ] {
        assert!(output.contains(expected), "{output}");
    }
}
#[test]
fn json_is_versioned_and_keeps_null_retention() {
    let mut output = Vec::new();
    render(&mut output, true, &state(), &[]).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output).unwrap();
    assert_eq!(value["schema"], "tidemark.pg.v1");
    assert_eq!(
        value["data"]["now"]["slots"][0]["retained_bytes"],
        serde_json::Value::Null
    );
    assert_eq!(value["data"]["now"]["databases"][0][0], "db");
}
