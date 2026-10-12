//! Owned by: W5-D
//! Snapshot fixtures for complete and degraded live reports.
use std::time::Duration;
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
    modes: &[(String, String, u32)],
    degraded: &[String],
) -> tidemark_core::live::NowReport {
    build_now(
        &NowInputs {
            host: "db-01",
            mounts: Some(mounts),
            lvm: Some(&[]),
            hot_dirs_5m: Some(&[]),
            writers_5m: Some(&[]),
            phantom: Some(&[]),
            open_writers: Some(&[]),
            alerts: Some(&[]),
            scans: Some(&[]),
            watch_modes: Some(modes),
            unevaluable: &[],
            degraded,
        },
        100,
    )
}
#[test]
fn healthy() {
    let r = report(
        &[mount("/data", false)],
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
fn degraded() {
    let r = report(
        &[mount("/data", true)],
        &[("/data".into(), "fd/mount".into(), 2)],
        &["procscan: unavailable".into()],
    );
    insta::assert_snapshot!(render_now_text(&r, 80, false));
}
#[test]
fn multi_mount() {
    let r = report(
        &[mount("/data", true), mount("/", false)],
        &[],
        &["daemon parts unavailable".into()],
    );
    assert_eq!(r.mounts[0].mount, "/");
    insta::assert_snapshot!(render_now_text(&r, 60, false));
}
#[test]
fn narrow_and_controls() {
    let mut r = report(&[], &[], &[]);
    r.host = "a\n\x1b".into();
    let text = render_now_text(&r, 1, false);
    assert!(!text.contains('\x1b'));
    assert!(text.lines().all(|l| l.chars().count() <= 1));
}
#[test]
fn byte_names_and_escaped_paths_are_distinct() {
    let mut r = report(&[mount("/data", false)], &[], &[]);
    for key in [b"proc\xff".to_vec(), b"proc\xfe".to_vec()] {
        r.mounts[0].writers_5m.push(tidemark_core::live::HotEntry {
            key,
            events: 1,
            files: 1,
            bytes_delta: 1,
            top_unit: None,
            lossy: false,
        });
    }
    r.mounts[0].mount = "/data\n".into();
    let a = render_now_text(&r, 1000, false);
    assert!(a.contains("proc\\xFF"));
    assert!(a.contains("proc\\xFE"));
    r.mounts[0].mount = "/data\\n".into();
    let b = render_now_text(&r, 1000, false);
    assert_ne!(a, b);
    assert!(b.contains("/data\\x5Cn"));
}
#[test]
fn missing_sections_and_alert_diagnostics_are_visible() {
    let diagnostic = [tidemark_live::alerts::AlertDiagnostic {
        rule: "stale".into(),
        reason: "effective scan cadence unavailable".into(),
    }];
    let mut inputs = NowInputs {
        host: "host",
        mounts: Some(&[]),
        lvm: Some(&[]),
        hot_dirs_5m: Some(&[]),
        writers_5m: Some(&[]),
        phantom: Some(&[]),
        open_writers: Some(&[]),
        alerts: Some(&[]),
        scans: Some(&[]),
        watch_modes: Some(&[]),
        unevaluable: &diagnostic,
        degraded: &[],
    };
    let measured = build_now(&inputs, 0);
    assert_eq!(measured.degraded.len(), 1);
    inputs.phantom = None;
    let missing = build_now(&inputs, 0);
    assert!(missing
        .degraded
        .iter()
        .any(|s| s == "phantom: not measured"));
    assert!(render_now_text(&missing, 1000, false)
        .contains("alert stale: not evaluable: effective scan cadence unavailable"));
    inputs.mounts = None;
    inputs.lvm = None;
    inputs.hot_dirs_5m = None;
    inputs.writers_5m = None;
    inputs.open_writers = None;
    inputs.alerts = None;
    inputs.scans = None;
    inputs.watch_modes = None;
    assert_eq!(build_now(&inputs, 0).degraded.len(), 10);
}

#[test]
fn missing_phantom_is_not_rendered_as_zero() {
    let mounts = [mount("/data", false)];
    let inputs = NowInputs {
        host: "host",
        mounts: Some(&mounts),
        lvm: None,
        hot_dirs_5m: None,
        writers_5m: None,
        phantom: None,
        open_writers: None,
        alerts: None,
        scans: None,
        watch_modes: None,
        unevaluable: &[],
        degraded: &[],
    };
    let text = render_now_text(&build_now(&inputs, 0), 1000, false);
    assert!(text.contains("deleted-but-open not measured"));
    assert!(!text.contains("deleted-but-open 0 B"));
    assert!(text.contains("lvm not measured"));
}
