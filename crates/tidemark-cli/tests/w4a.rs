//! Owned by: W4-A
//! Process-isolated reporting-tool overrides and CLI n/a behavior.
#![allow(clippy::unwrap_used)]
use std::os::unix::fs::PermissionsExt;
fn script(dir: &std::path::Path, name: &str, body: &str) -> std::path::PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    path
}
fn command() -> std::process::Command {
    {
        let mut cmd = std::process::Command::new(env!("CARGO_BIN_EXE_tidemark"));
        cmd.args(["--config", "/missing/tidemark-w4a-config"]);
        cmd
    }
}
#[test]
fn missing_tools_render_na_text_and_json() {
    for json in [false, true] {
        let mut cmd = command();
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
    let dir = tempfile::tempdir().unwrap();
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
    let output = command()
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
    let dir = tempfile::tempdir().unwrap();
    let failing = script(dir.path(), "fail", "exit 9");
    for path in [std::path::PathBuf::from("/missing/tidemark-lvs"), failing] {
        let output = command()
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
    let dir = tempfile::tempdir().unwrap();
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
    let output = command()
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
    let output = command()
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
