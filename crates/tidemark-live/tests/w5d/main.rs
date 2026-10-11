//! Owned by: W5-D
//! Snapshot fixtures for complete and degraded live reports.
use std::time::Duration;
use tidemark_core::live::PgNow;
use tidemark_live::{
    now::{build_now, render_now_text, NowInputs},
    rates::MountRates,
};
fn mount(name: &str, filling: bool) -> MountRates {
    MountRates {
        mount: name.into(),
        size: 1000,
        used: if filling { 950 } else { 500 },
        avail: if filling { 50 } else { 500 },
        rate_15m: Some(if filling { 2.0 } else { 0.0 }),
        rate_1h: Some(if filling { 1.0 } else { 0.0 }),
        ttf: if filling {
            Some(Duration::from_secs(50))
        } else {
            None
        },
    }
}
fn report(
    mounts: &[MountRates],
    pg: Option<&PgNow>,
    modes: &[(String, String, u32)],
    degraded: &[String],
) -> tidemark_core::live::NowReport {
    build_now(
        &NowInputs {
            host: "db-01",
            mounts,
            lvm: &[],
            hot_dirs_5m: &[],
            writers_5m: &[],
            phantom: &[],
            open_writers: &[],
            pg,
            alerts: &[],
            scans: &[],
            watch_modes: modes,
            degraded,
        },
        100,
    )
}
#[test]
fn healthy() {
    let r = report(
        &[mount("/data", false)],
        None,
        &[("/data".into(), "dfid-name/filesystem".into(), 0)],
        &[],
    );
    let mut r = r;
    r.mounts[0].lvm = Some(tidemark_core::live::LvmNow {
        vg: "vg00".into(),
        lv: "data".into(),
        lv_size: 1000,
        vg_free: 0,
        thin_pool: Some("pool0".into()),
        data_pct: Some(81.0),
        meta_pct: Some(12.0),
        sampled_at: 100,
    });
    r.mounts[0].hot_dirs_5m.push(tidemark_core::live::HotEntry {
        key: b"/data/reports".to_vec(),
        events: 12,
        files: 10,
        bytes_delta: 2048,
        top_unit: Some("reports.service".into()),
        lossy: false,
    });
    r.mounts[0].phantom.total_bytes = 1024;
    r.mounts[0].phantom.files = 1;
    r.mounts[0]
        .phantom
        .holders
        .push(tidemark_core::live::PhantomHolder {
            pid: 42,
            comm: "java".into(),
            unit: Some("app.service".into()),
            path: b"/data/logs/app.log".to_vec(),
            dev: 1,
            ino: 1,
            bytes: 1024,
        });
    insta::assert_snapshot!(render_now_text(&r, 100, false));
}
#[test]
fn filling() {
    let r = report(
        &[mount("/data", true)],
        None,
        &[("/data".into(), "dfid-name/mount".into(), 0)],
        &[],
    );
    insta::assert_snapshot!(render_now_text(&r, 100, false));
}
#[test]
fn degraded() {
    let pg = PgNow {
        sampled_at: Some(50),
        unreachable_since: Some(60),
        server_version_num: None,
        databases: vec![],
        wal_dir_bytes: None,
        slots: vec![],
        archiver_failing: false,
        archiver_last_failed: None,
        temp_bytes_rate: None,
        top_relations: vec![],
    };
    let r = report(
        &[mount("/data", true)],
        Some(&pg),
        &[("/data".into(), "fd/mount".into(), 2)],
        &["procscan: unavailable".into()],
    );
    insta::assert_snapshot!(render_now_text(&r, 80, false));
}
#[test]
fn multi_mount() {
    let r = report(
        &[mount("/data", true), mount("/", false)],
        None,
        &[],
        &["daemon parts unavailable".into()],
    );
    assert_eq!(r.mounts[0].mount, "/");
    insta::assert_snapshot!(render_now_text(&r, 60, false));
}
#[test]
fn narrow_and_controls() {
    let mut r = report(&[], None, &[], &[]);
    r.host = "a\n\x1b".into();
    let text = render_now_text(&r, 1, false);
    assert!(!text.contains('\x1b'));
    assert!(text.lines().all(|l| l.chars().count() <= 1));
}
