//! Owned by: W4-A
//! Report rendering and optional history tests.
use super::*;
fn fixture() -> (lvm::LvmReport, Vec<lvm::MountLv>) {
    let report = lvm::LvmReport {
        lvs: lvm::parse_lvs_json(include_bytes!(
            "../../../test-support/fixtures/lvm/lvs.json"
        ))
        .unwrap(),
        vgs: lvm::parse_vgs_json(include_bytes!(
            "../../../test-support/fixtures/lvm/vgs.json"
        ))
        .unwrap(),
        sampled_at: 86400,
    };
    let mapped = vec![lvm::MountLv {
        mount: tidemark_core::path::RawPath::new(b"/data".to_vec()).unwrap(),
        vg: "vg-with-hyphens".into(),
        lv: "data".into(),
        thin_pool: Some("pool".into()),
    }];
    (report, mapped)
}
#[test]
fn renders_text_json_and_absent_history() {
    let (r, m) = fixture();
    let report = compose(&r, &m, None, 86400);
    let mut text = Vec::new();
    crate::render::lvm::write(&report, false, &mut text).unwrap();
    let text = String::from_utf8(text).unwrap();
    assert!(text.contains("data=25.50%"));
    assert!(text.contains("24h trend: n/a"));
    assert!(text.contains("headroom for lvextend + xfs_growfs"));
    assert!(text.contains("other LV vg-with-hyphens/pool_tdata"));
    let mut json = Vec::new();
    crate::render::lvm::write(&report, true, &mut json).unwrap();
    let json: serde_json::Value = serde_json::from_slice(&json).unwrap();
    assert_eq!(json["schema"], "tidemark.lvm.v1");
    assert_eq!(json["data"]["tracked"][0]["mount"]["path"], "/data");
}
#[test]
fn reads_history_and_computes_24h_trend() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("timeseries.db");
    let db = tidemark_store::timeseries::TimeSeries::open(&path, true).unwrap();
    let (r, m) = fixture();
    let samples: Vec<_> = (0..3)
        .map(|i| tidemark_core::live::LvmSample {
            ts: i * 43200,
            vg: m[0].vg.clone(),
            lv: m[0].lv.clone(),
            lv_size: 1024,
            vg_free: 512,
            data_pct: Some(10.0 + i as f64),
            meta_pct: None,
        })
        .collect();
    db.insert_lvm(&samples).unwrap();
    drop(db);
    let db = tidemark_store::timeseries::TimeSeries::open(&path, false).unwrap();
    let output = compose(&r, &m, Some(&db), 86400);
    let trend = output.tracked[0].trend.as_ref().unwrap();
    assert_eq!(trend.data_pp_per_day, Some(2.0));
    assert_eq!(trend.meta_pp_per_day, None);
    assert_eq!(slope(&samples[..2], |s| s.data_pct), None);
}
