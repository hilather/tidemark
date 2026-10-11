//! Owned by: W4-A
//! Pure parser/mapping and bounded reporting-process tests.
use super::*;
use std::{
    os::unix::fs::PermissionsExt,
    time::{Duration, Instant},
};
const LVS: &[u8] = include_bytes!("../../../test-support/fixtures/lvm/lvs.json");
const VGS: &[u8] = include_bytes!("../../../test-support/fixtures/lvm/vgs.json");
fn report() -> LvmReport {
    LvmReport {
        lvs: parse_lvs_json(LVS).unwrap(),
        vgs: parse_vgs_json(VGS).unwrap(),
        sampled_at: 42,
    }
}
#[test]
fn parser_fixture_types_and_empty_fields() {
    let r = report();
    assert_eq!(r.lvs.len(), 6);
    let hidden = r.lvs.iter().find(|v| v.lv == "[pool_tdata]").unwrap();
    assert_eq!(hidden.major, None);
    assert_eq!(hidden.dm_path, None);
    assert_eq!(r.vgs[0].vg, "vg-with-hyphens");
    assert!(r.lvs.iter().any(|v| v.segtype == "snapshot"));
}
#[test]
fn parser_rejects_bad_shape_numbers_and_nonfinite_percentages() {
    for input in [
        b"{}".as_slice(),
        b"{\"report\":[{\"lv\":[{}]}]}",
        b"not json",
    ] {
        assert!(parse_lvs_json(input).is_err());
    }
    for value in ["NaN", "inf", "-1", "100.1"] {
        let input = String::from_utf8(LVS.to_vec())
            .unwrap()
            .replace("25.50", value);
        assert!(parse_lvs_json(input.as_bytes()).is_err());
    }
    assert!(parse_vgs_json(VGS).is_ok());
    assert!(parse_vgs_json(
        String::from_utf8(VGS.to_vec())
            .unwrap()
            .replace("8589934592", "-1")
            .as_bytes()
    )
    .is_err());
    assert!(parse_lvs_json(&vec![b' '; MAX_BYTES + 1]).is_err());
}
#[test]
fn maps_device_numbers_and_uses_pool_usage() {
    let r = report();
    let mounts=crate::mount::parse_mountinfo(b"1 0 253:1 / /a rw - xfs /dev/dm-1 rw\n2 0 253:1 / /b rw - xfs /dev/mapper/odd rw\n3 0 253:1 / /c rw - xfs /dev/vg/data rw\n4 0 8:1 / /unmatched rw - xfs /dev/dm-1 rw\n").unwrap();
    let mapped = map_mounts(&r, &mounts);
    assert_eq!(mapped.len(), 3);
    for m in &mapped {
        let now = lvm_now(&r, m);
        assert_eq!(now.lv, "data");
        assert_eq!(now.data_pct, Some(25.5));
        assert_eq!(now.meta_pct, Some(3.25));
        assert_eq!(now.vg_free, r.vgs[0].free);
    }
    assert_eq!(to_samples(&r, &mapped).len(), 1);
}
fn script(body: &str) -> (tempfile::TempDir, std::ffi::OsString) {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("fake");
    std::fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
    (dir, path.into_os_string())
}
#[test]
fn command_environment_arguments_success_and_failure() {
    let (_dir,path)=script("[ \"$LC_ALL\" = C ] && [ \"$LVM_SUPPRESS_FD_WARNINGS\" = 1 ] || exit 3\n[ \"$*\" = '--reportformat json --units b --nosuffix -o test' ] || exit 4\nprintf '%s' '{\"report\":[]}'");
    assert_eq!(
        command(path, "test", Duration::from_secs(1)).unwrap(),
        b"{\"report\":[]}"
    );
    let (_dir, path) = script("echo failed >&2; exit 7");
    assert!(matches!(
        command(path, "test", Duration::from_secs(1)),
        Err(LvmError::Command(_))
    ));
    assert!(matches!(
        command(
            "/missing/tidemark-lvs".into(),
            "test",
            Duration::from_secs(1)
        ),
        Err(LvmError::NotInstalled)
    ));
}
#[test]
fn timeout_kills_and_reaps_child() {
    let dir = tempfile::tempdir().unwrap();
    let pidfile = dir.path().join("pid");
    let (_script, path) = script(&format!("echo $$ > '{}'; exec sleep 60", pidfile.display()));
    let start = Instant::now();
    assert!(matches!(
        command(path, "test", Duration::from_millis(100)),
        Err(LvmError::Timeout)
    ));
    assert!(start.elapsed() < Duration::from_secs(2));
    let pid = std::fs::read_to_string(pidfile).unwrap();
    assert!(!std::path::Path::new(&format!("/proc/{}", pid.trim())).exists());
}
#[test]
fn output_cap_and_disabled_collection() {
    let (_dir, path) = script("exec head -c 20000000 /dev/zero");
    assert!(matches!(
        command(path, "test", Duration::from_secs(3)),
        Err(LvmError::Invalid(_))
    ));
    assert!(matches!(
        collect(
            &LvmConfig {
                enabled: false,
                ..Default::default()
            },
            0
        ),
        Err(LvmError::Disabled)
    ));
}
#[cfg(feature = "privileged")]
mod privileged {
    use super::*;
    use test_support::lvm::{LoopVg, LvLayout};
    #[test]
    fn lvm_maps_mounts_to_lvs() {
        std::env::set_var("TIDEMARK_LVS_BIN", "/usr/sbin/lvs");
        std::env::set_var("TIDEMARK_VGS_BIN", "/usr/sbin/vgs");
        let vg = LoopVg::new(
            "w4amap",
            2 * 1024 * 1024 * 1024,
            LvLayout {
                linear: Some(512 * 1024 * 1024),
                thin_pool: Some((512 * 1024 * 1024, 1024 * 1024 * 1024)),
            },
        );
        let start = Instant::now();
        let r = collect(&LvmConfig::default(), 42).unwrap();
        assert!(start.elapsed() < Duration::from_secs(2));
        assert_eq!(
            r.vgs.iter().find(|v| v.vg == vg.name()).unwrap().free,
            test_support::oracle::vgs_free(vg.name()).unwrap()
        );
        let oracle = test_support::oracle::lvs_bytes(vg.name()).unwrap();
        let mut actual: Vec<_> = r
            .lvs
            .iter()
            .filter(|lv| lv.vg == vg.name())
            .map(|lv| lv.size)
            .collect();
        actual.sort_unstable();
        let mut expected: Vec<_> = oracle.iter().map(|row| row[0] as u64).collect();
        expected.sort_unstable();
        assert_eq!(actual, expected);
        let mounts = crate::mount::read_mountinfo().unwrap();
        let mapped = map_mounts(&r, &mounts);
        for lv in &vg.mounts {
            let raw = RawPath::from_path(&lv.mountpoint).unwrap();
            let m = mapped.iter().find(|m| m.mount == raw).unwrap();
            assert_eq!(m.vg, vg.name());
            let name = if lv.device.file_name().unwrap() == "thin" {
                "thin"
            } else {
                "linear"
            };
            assert_eq!(m.lv, name);
            assert_eq!(
                m.thin_pool.as_deref(),
                if name == "thin" { Some("pool") } else { None }
            );
        }
    }
}
