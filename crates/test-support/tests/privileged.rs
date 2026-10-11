//! Owned by: W0-C
//! Disposable VM-only smoke tests; feature gated, never ignored.
#![cfg(feature = "privileged")]
use test_support::{
    lvm::{LoopVg, LvLayout},
    xfs::LoopXfs,
};
#[test]
fn loop_xfs_smoke() {
    let mut fs = LoopXfs::new(512 * 1024 * 1024, &["-m", "reflink=0"], "noatime");
    std::fs::write(fs.mountpoint.join("smoke"), [0x5a; 65536]).unwrap();
    assert!(
        std::fs::metadata(fs.mountpoint.join("smoke"))
            .unwrap()
            .len()
            > 0
    );
    let image = fs.image.clone();
    let mount = fs.mountpoint.clone();
    fs.cleanup().unwrap();
    assert!(!image.exists() && !mount.exists());
}
#[test]
fn loop_lvm_thin_smoke() {
    let mut vg = LoopVg::new(
        "smoke",
        2 * 1024 * 1024 * 1024,
        LvLayout {
            linear: Some(512 * 1024 * 1024),
            thin_pool: Some((768 * 1024 * 1024, 1024 * 1024 * 1024)),
        },
    );
    assert!(vg.name().starts_with("tmtest_"));
    assert_eq!(vg.mounts.len(), 2);
    for lv in &vg.mounts {
        std::fs::write(lv.mountpoint.join("smoke"), [0x5a; 65536]).unwrap();
        assert_eq!(
            std::fs::metadata(lv.mountpoint.join("smoke"))
                .unwrap()
                .len(),
            65536
        );
    }
    let image = vg.image.clone();
    vg.cleanup().unwrap();
    assert!(!image.exists());
}

#[test]
fn special_fixture_smoke() {
    use std::os::unix::fs::FileTypeExt;
    assert!(
        rustix::process::geteuid().is_root(),
        "special fixture smoke requires root in a disposable VM"
    );
    let special = test_support::materialize(include_str!("../fixtures/mixed_special.toml"));
    assert!(std::fs::symlink_metadata(special.root.join("socket"))
        .unwrap()
        .file_type()
        .is_socket());
    let device = test_support::materialize(include_str!("../fixtures/mixed_special_root.toml"));
    assert!(std::fs::symlink_metadata(device.root.join("null"))
        .unwrap()
        .file_type()
        .is_char_device());
}
