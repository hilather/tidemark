//! Owned by: WSTATE-trust-A
//! Scan admission checks private regular lock leaves before opening or flocking.
#![allow(clippy::unwrap_used)]
use std::os::unix::fs::{symlink, PermissionsExt};
use tidemark_scan::orchestrator::ScanLock;
#[test]
fn scan_lock_refuses_hostile_leaves_without_changes() {
    for kind in ["symlink", "hardlink", "fifo", "mode"] {
        let state = test_support::private_tempdir().unwrap();
        let outside = test_support::private_tempdir().unwrap();
        let victim = outside.path().join("victim");
        test_support::private_write(&victim, b"sentinel").unwrap();
        let lock = state.path().join("scan.lock");
        match kind {
            "symlink" => symlink(&victim, &lock).unwrap(),
            "hardlink" => std::fs::hard_link(&victim, &lock).unwrap(),
            "fifo" => rustix::fs::mknodat(
                rustix::fs::CWD,
                &lock,
                rustix::fs::FileType::Fifo,
                rustix::fs::Mode::from_raw_mode(0o600),
                0,
            )
            .unwrap(),
            "mode" => {
                test_support::private_write(&lock, b"unsafe").unwrap();
                std::fs::set_permissions(&lock, std::fs::Permissions::from_mode(0o620)).unwrap();
            }
            _ => panic!("unknown fixture"),
        }
        assert!(ScanLock::acquire(state.path()).is_err(), "{kind}");
        assert_eq!(std::fs::read_dir(state.path()).unwrap().count(), 1);
        assert_eq!(std::fs::read(&victim).unwrap(), b"sentinel");
        assert_eq!(std::fs::metadata(&victim).unwrap().len(), 8);
        assert_eq!(
            std::fs::metadata(&victim).unwrap().permissions().mode() & 0o777,
            0o600
        );
    }
}
