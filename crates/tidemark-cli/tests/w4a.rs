//! Owned by: W4-A
//! Process-isolated reporting-tool overrides and CLI n/a behavior.
#![allow(clippy::unwrap_used)]
#[path = "support/config.rs"]
mod config_support;
use std::os::unix::fs::PermissionsExt;
fn script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn command(owner: &std::path::Path) -> std::process::Command {
    {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"));
        cmd.arg("--config")
            .arg(config_support::direct_config(owner, ""));
        cmd
    }
}
#[test]
fn missing_tools_render_na_text_and_json() {
    for json in [false, true] {
        let command_owner = ::test_support::private_tempdir().unwrap();
        let mut cmd = command(command_owner.path());
        if json {
            cmd.arg("--json");
        }
        let output = cmd
            .args(["lvm"])
            .env("TIDEMARK_LVS_BIN", "/missing/tidemark-lvs")
            .env("TIDEMARK_VGS_BIN", "/missing/tidemark-vgs")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        if json {
            let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
            assert_eq!(value["data"]["status"], "n/a");
        } else {
            assert_eq!(output.stdout, b"LVM: n/a\n");
        }
    }
}
#[test]
fn fake_executables_are_collected_without_host_lvm() {
    let dir = ::test_support::private_tempdir().unwrap();
    let lvs = script(
        dir.path(),
        "lvs",
        "printf '%s' '{\"report\":[{\"lv\":[]}]}'",
    );
    let vgs = script(
        dir.path(),
        "vgs",
        "printf '%s' '{\"report\":[{\"vg\":[]}]}'",
    );
    let output = command(dir.path())
        .args(["--json", "lvm", "--mount"])
        .arg(dir.path())
        .env("TIDEMARK_LVS_BIN", lvs)
        .env("TIDEMARK_VGS_BIN", vgs)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["tracked"], serde_json::json!([]));
}
#[test]
fn probe_tolerates_absent_or_failed_lvm() {
    let dir = ::test_support::private_tempdir().unwrap();
    let failing = script(dir.path(), "fail", "exit 9");
    for path in [std::path::PathBuf::from("/missing/tidemark-lvs"), failing] {
        let output = command(dir.path())
            .args(["--json", "probe", "--root"])
            .arg(dir.path())
            .env("TIDEMARK_LVS_BIN", path)
            .env("TIDEMARK_VGS_BIN", "/missing/tidemark-vgs")
            .output()
            .unwrap();
        assert!(output.status.success());
        let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert!(value["data"]["roots"]
            .as_array()
            .unwrap()
            .iter()
            .any(|root| root["facts"]["total_bytes"]["value"].is_string()));
    }
}
#[test]
fn synthetic_report_maps_measured_fixture_device_in_cli_and_probe() {
    let dir = ::test_support::private_tempdir().unwrap();
    let raw = tidemark_core::path::RawPath::from_path(dir.path()).unwrap();
    let mounts = tidemark_fs::mount::read_mountinfo().unwrap();
    let mount = tidemark_fs::mount::mount_for_path(&mounts, &raw).unwrap();
    // Measure the temporary fixture's containing device; assume no host mount layout.
    let mut fixture: serde_json::Value =
        serde_json::from_slice(include_bytes!("../../test-support/fixtures/lvm/lvs.json")).unwrap();
    for lv in fixture["report"][0]["lv"].as_array_mut().unwrap() {
        lv["lv_kernel_major"] = serde_json::json!(if lv["lv_name"] == "data" {
            mount.major.to_string()
        } else {
            "-1".into()
        });
        lv["lv_kernel_minor"] = serde_json::json!(if lv["lv_name"] == "data" {
            mount.minor.to_string()
        } else {
            "-1".into()
        });
    }
    let lvs = script(
        dir.path(),
        "lvs",
        &format!("printf '%s' '{}'", serde_json::to_string(&fixture).unwrap()),
    );
    let vgs = script(
        dir.path(),
        "vgs",
        &format!(
            "printf '%s' '{}'",
            String::from_utf8_lossy(include_bytes!("../../test-support/fixtures/lvm/vgs.json"))
        ),
    );
    let output = command(dir.path())
        .args(["--json", "lvm", "--mount"])
        .arg(dir.path())
        .env("TIDEMARK_LVS_BIN", &lvs)
        .env("TIDEMARK_VGS_BIN", &vgs)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(value["data"]["tracked"][0]["current"]["data_pct"], 25.5);
    assert_eq!(value["data"]["tracked"][0]["segtype"], "thin");
    let output = command(dir.path())
        .args(["--json", "probe", "--root"])
        .arg(dir.path())
        .env("TIDEMARK_LVS_BIN", &lvs)
        .env("TIDEMARK_VGS_BIN", &vgs)
        .output()
        .unwrap();
    assert!(output.status.success());
    let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let root = value["data"]["roots"].as_array().unwrap().last().unwrap();
    assert!(root["facts"]["lvm"]["value"]
        .as_str()
        .unwrap()
        .contains("LV vg-with-hyphens/data; thin pool pool; VG free 4294967296 bytes"));
}

#[cfg(feature = "privileged")]
mod privileged {
    use std::{io::Write, time::Duration};
    use test_support::{
        lvm::{LoopVg, LvLayout},
        xfs::LoopXfs,
    };
    use tidemark_core::{
        config::{AlertKind, AlertRule, LvmConfig},
        live::{AlertState, FsSample, HotEntry, LvmNow},
        path::RawPath,
    };
    use tidemark_fs::{lvm, mount};
    use tidemark_live::alerts::{AlertEngine, AlertInputs, AlertSource, ScanAge};

    // Only LVM measurements are needed by this rule.
    struct PoolSource(LvmNow);
    impl AlertSource for PoolSource {
        fn fs_sample(&self, _: &str) -> Option<FsSample> {
            None
        }
        fn fs_rate(&self, _: &str, _: Duration) -> Option<f64> {
            None
        }
        fn hot_dirs(&self, _: &str, _: Duration) -> Option<Vec<HotEntry>> {
            None
        }
        fn watch_overflows(&self, _: &str, _: Duration) -> Option<u64> {
            None
        }
        fn phantom_bytes(&self, _: &str) -> Option<u64> {
            None
        }
        fn lvm(&self) -> Option<Vec<LvmNow>> {
            Some(vec![self.0.clone()])
        }
        fn dir_growth(&self, _: &str, _: u32) -> Option<Vec<(RawPath, i64)>> {
            None
        }
        fn scan_age(&self, _: &str) -> Option<ScanAge> {
            None
        }
    }
    fn installed_config() -> LvmConfig {
        LvmConfig {
            enabled: true,
            ..Default::default()
        }
    }
    #[test]
    fn lvm_thin_pool_usage_tracks_writes() {
        // VM runs use installed tools even if the parent supplied local-check overrides.
        std::env::set_var("TIDEMARK_LVS_BIN", "/usr/sbin/lvs");
        std::env::set_var("TIDEMARK_VGS_BIN", "/usr/sbin/vgs");
        let vg = LoopVg::new(
            "w4athin",
            2 * 1024 * 1024 * 1024,
            LvLayout {
                linear: None,
                thin_pool: Some((512 * 1024 * 1024, 1024 * 1024 * 1024)),
            },
        );
        let before = lvm::collect(&installed_config(), 1).unwrap();
        let mounts = mount::read_mountinfo().unwrap();
        let mapped = lvm::map_mounts(&before, &mounts);
        let m = mapped
            .iter()
            .find(|m| m.vg == vg.name() && m.lv == "thin")
            .unwrap();
        assert_eq!(m.thin_pool.as_deref(), Some("pool"));
        let initial = lvm::lvm_now(&before, m).data_pct.unwrap();
        let mut file = std::fs::File::create(vg.mounts[0].mountpoint.join("payload")).unwrap();
        let data = vec![42; 1024 * 1024];
        for _ in 0..32 {
            file.write_all(&data).unwrap();
        }
        file.sync_all().unwrap();
        let after = lvm::collect(&installed_config(), 2).unwrap();
        let current = lvm::lvm_now(&after, m);
        assert!(current.data_pct.unwrap() > initial);
        // Separate plain numeric lvs oracle, selecting the exact pool rather than a thin LV.
        let oracle = test_support::oracle::lvs_bytes(&format!("{}/pool", vg.name())).unwrap();
        assert_eq!(oracle.len(), 1);
        assert!((current.data_pct.unwrap() - oracle[0][1]).abs() <= 0.01);
        assert!((current.meta_pct.unwrap() - oracle[0][2]).abs() <= 0.01);
        let rule = AlertRule {
            name: "thin_pool_pct".into(),
            kind: AlertKind::ThinPoolPct,
            vg: Some(vg.name().into()),
            pool: Some("pool".into()),
            data_above: Some(initial),
            meta_above: Some(100.0),
            for_: Duration::from_secs(1),
            ..Default::default()
        };
        let mut engine = AlertEngine::new(&[rule], Duration::from_secs(60)).unwrap();
        let source = PoolSource(current);
        let inputs = AlertInputs {
            source: &source,
            state_space_refused: false,
        };
        assert!(engine.evaluate(10, &inputs).is_empty());
        let events = engine.evaluate(11, &inputs);
        assert!(events.iter().any(|e| e.rule == "thin_pool_pct"
            && e.subject == format!("{}/pool", vg.name())
            && e.state == AlertState::Firing));
        assert!(engine.diagnostics().is_empty());
    }
    #[test]
    fn lvm_absent_is_na() {
        // Include an actual LVM mount, so an unrelated VG cannot classify the loop mount.
        std::env::set_var("TIDEMARK_LVS_BIN", "/usr/sbin/lvs");
        std::env::set_var("TIDEMARK_VGS_BIN", "/usr/sbin/vgs");
        let _vg = LoopVg::new(
            "w4aabsent",
            1024 * 1024 * 1024,
            LvLayout {
                linear: Some(512 * 1024 * 1024),
                thin_pool: None,
            },
        );
        let fs = LoopXfs::new(512 * 1024 * 1024, &[], "");
        let report = lvm::collect(&installed_config(), 42).unwrap();
        let mounts = mount::read_mountinfo().unwrap();
        let raw = RawPath::from_path(&fs.mountpoint).unwrap();
        let selected = mount::mount_for_path(&mounts, &raw).unwrap();
        assert!(lvm::map_mounts(&report, std::slice::from_ref(selected)).is_empty());
        let dir = ::test_support::private_tempdir().unwrap();
        let config = super::config_support::direct_config(dir.path(), "[lvm]\nenabled = true\n");
        for json in [false, true] {
            let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"));
            cmd.arg("--config").arg(&config);
            if json {
                cmd.arg("--json");
            }
            let output = cmd
                .args(["lvm", "--mount"])
                .arg(&fs.mountpoint)
                .env("TIDEMARK_LVS_BIN", "/usr/sbin/lvs")
                .env("TIDEMARK_VGS_BIN", "/usr/sbin/vgs")
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "{}",
                String::from_utf8_lossy(&output.stderr)
            );
            if json {
                let value: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
                assert_eq!(value["data"]["status"], "n/a");
                assert_eq!(value["data"]["tracked"], serde_json::json!([]));
            } else {
                assert_eq!(output.stdout, b"LVM: n/a\n");
            }
        }
    }
}
