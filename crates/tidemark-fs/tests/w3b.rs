//! Owned by: W3-B
//! Symlink classification, traversal safety and pending VM evidence.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    os::unix::{ffi::OsStringExt, fs::symlink},
    sync::atomic::{AtomicBool, AtomicU64, Ordering},
};
use tidemark_core::{path::RawPath, scan::Gate};
use tidemark_fs::symlinks::*;
#[derive(Default)]
struct CountingGate(AtomicU64);
impl Gate for CountingGate {
    fn current_rate(&self) -> f64 {
        0.0
    }
    fn acquire(&self, cost: u32) {
        self.0.fetch_add(u64::from(cost), Ordering::Relaxed);
    }
}
fn count(r: &SymlinkReport, c: SymlinkClass) -> u64 {
    r.by_class.iter().find(|(k, _)| *k == c).unwrap().1
}
#[test]
fn symlink_loops_lexical_and_checked() {
    let f = test_support::materialize(include_str!(
        "../../test-support/fixtures/symlink_loops.toml"
    ));
    let root = RawPath::from_path(&f.root).unwrap();
    let gate = CountingGate::default();
    let cancel = AtomicBool::new(false);
    let lexical = analyze(
        &root,
        &gate,
        &cancel,
        &SymlinkOpts {
            check_targets: false,
            limit: 100,
        },
    )
    .unwrap();
    assert_eq!(lexical.total, 4);
    assert_eq!(count(&lexical, SymlinkClass::InsideRoot), 4);
    let charges = gate.0.swap(0, Ordering::Relaxed);
    assert!(charges > 4);
    let checked = analyze(
        &root,
        &gate,
        &cancel,
        &SymlinkOpts {
            check_targets: true,
            limit: 100,
        },
    )
    .unwrap();
    assert_eq!(checked.total, 4);
    assert_eq!(count(&checked, SymlinkClass::Loop), 2);
    assert_eq!(count(&checked, SymlinkClass::Dangling), 1);
    assert_eq!(count(&checked, SymlinkClass::InsideRoot), 1);
    assert!(gate.0.load(Ordering::Relaxed) > charges + 4);
}
#[test]
fn targets_preserve_bytes_and_links_are_never_descended() {
    let temp = tempfile::tempdir().unwrap();
    let root_path = temp.path().join("root");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&root_path).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::create_dir(root_path.join("dir")).unwrap();
    std::fs::write(root_path.join("file"), []).unwrap();
    symlink("missing", outside.join("must_not_visit")).unwrap();
    symlink("file", root_path.join("relative")).unwrap();
    symlink(root_path.join("file"), root_path.join("absolute")).unwrap();
    symlink("../outside", root_path.join("escape")).unwrap();
    symlink("../file", root_path.join("dir/up")).unwrap();
    symlink(
        "missing",
        root_path.join(std::ffi::OsString::from_vec(vec![255])),
    )
    .unwrap();
    let root = RawPath::from_path(&root_path).unwrap();
    let gate = CountingGate::default();
    let cancel = AtomicBool::new(false);
    let lexical = analyze(
        &root,
        &gate,
        &cancel,
        &SymlinkOpts {
            check_targets: false,
            limit: 100,
        },
    )
    .unwrap();
    assert_eq!(lexical.total, 5);
    assert_eq!(lexical.absolute, 1);
    assert_eq!(lexical.relative, 4);
    assert_eq!(count(&lexical, SymlinkClass::OutsideRoot), 1);
    assert_eq!(count(&lexical, SymlinkClass::InsideRoot), 4);
    assert!(lexical
        .examples
        .iter()
        .any(|(p, t, _)| p.as_bytes().ends_with(&[255]) && t == b"missing"));
    let checked = analyze(
        &root,
        &gate,
        &cancel,
        &SymlinkOpts {
            check_targets: true,
            limit: 0,
        },
    )
    .unwrap();
    assert_eq!(checked.total, 5);
    assert!(checked.examples.is_empty());
    assert_eq!(count(&checked, SymlinkClass::Dangling), 1);
    // Root entry itself must not be followed by the analyzer.
    symlink(&root_path, temp.path().join("alias")).unwrap();
    assert!(analyze(
        &RawPath::from_path(&temp.path().join("alias")).unwrap(),
        &gate,
        &cancel,
        &SymlinkOpts {
            check_targets: false,
            limit: 1
        }
    )
    .is_err());
}
#[test]
fn cancellation_and_caps_fail_explicitly() {
    let temp = tempfile::tempdir().unwrap();
    let root = RawPath::from_path(temp.path()).unwrap();
    let gate = CountingGate::default();
    assert_eq!(
        analyze(
            &root,
            &gate,
            &AtomicBool::new(true),
            &SymlinkOpts {
                check_targets: false,
                limit: 1
            }
        )
        .err()
        .unwrap()
        .kind(),
        std::io::ErrorKind::Interrupted
    );
    assert_eq!(gate.0.load(Ordering::Relaxed), 0);
    assert!(analyze(
        &root,
        &gate,
        &AtomicBool::new(false),
        &SymlinkOpts {
            check_targets: false,
            limit: 1_000_001
        }
    )
    .is_err());
    let mut long = b"/".to_vec();
    long.extend(vec![b'x'; 1024 * 1024]);
    let long = RawPath::new(long).unwrap();
    assert!(analyze(
        &long,
        &gate,
        &AtomicBool::new(false),
        &SymlinkOpts {
            check_targets: false,
            limit: 1
        }
    )
    .is_err());
    assert_eq!(gate.0.load(Ordering::Relaxed), 0);
}
#[test]
fn checked_stat_uses_original_target_before_dotdot_normalization() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    let outside = temp.path().join("outside");
    std::fs::create_dir(&root).unwrap();
    std::fs::create_dir(&outside).unwrap();
    std::fs::create_dir(outside.join("sub")).unwrap();
    std::fs::write(outside.join("exists"), []).unwrap();
    symlink(outside.join("sub"), root.join("jump")).unwrap();
    symlink("jump/../exists", root.join("link")).unwrap();
    let r = analyze(
        &RawPath::from_path(&root).unwrap(),
        &CountingGate::default(),
        &AtomicBool::new(false),
        &SymlinkOpts {
            check_targets: true,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(count(&r, SymlinkClass::Dangling), 0);
}
#[cfg(feature = "privileged")]
#[test]
fn other_filesystem_target_and_mount_boundary_pending_vm() {
    let mut xfs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    let root = xfs.mountpoint.parent().unwrap();
    std::fs::write(xfs.mountpoint.join("target"), []).unwrap();
    symlink("absent", xfs.mountpoint.join("must_not_visit")).unwrap();
    symlink(xfs.mountpoint.join("target"), root.join("cross_fs")).unwrap();
    let r = analyze(
        &RawPath::from_path(root).unwrap(),
        &CountingGate::default(),
        &AtomicBool::new(false),
        &SymlinkOpts {
            check_targets: true,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(r.total, 1);
    assert_eq!(count(&r, SymlinkClass::OtherFs), 1);
    std::fs::remove_file(root.join("cross_fs")).unwrap();
    xfs.cleanup().unwrap();
}
#[cfg(feature = "privileged")]
#[test]
fn same_device_bind_mount_is_skipped_pending_vm() {
    use std::process::Command;
    struct Bind {
        path: std::path::PathBuf,
        mounted: bool,
    }
    impl Bind {
        fn cleanup(&mut self) {
            assert!(
                Command::new("umount")
                    .arg("--")
                    .arg(&self.path)
                    .status()
                    .unwrap()
                    .success(),
                "failed to unmount temporary bind fixture"
            );
            self.mounted = false;
        }
    }
    impl Drop for Bind {
        fn drop(&mut self) {
            if self.mounted {
                let _ = Command::new("umount").arg("--").arg(&self.path).status();
            }
        }
    }
    let xfs = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "");
    let root = xfs.mountpoint.join("root");
    let source = root.join("source");
    let alias = root.join("bind");
    std::fs::create_dir_all(&source).unwrap();
    std::fs::create_dir(&alias).unwrap();
    symlink("missing", source.join("link")).unwrap();
    assert!(Command::new("mount")
        .arg("--bind")
        .arg("--")
        .arg(&source)
        .arg(&alias)
        .status()
        .unwrap()
        .success());
    let mut bind = Bind {
        path: alias,
        mounted: true,
    };
    let r = analyze(
        &RawPath::from_path(&root).unwrap(),
        &CountingGate::default(),
        &AtomicBool::new(false),
        &SymlinkOpts {
            check_targets: true,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(r.total, 1);
    assert_eq!(count(&r, SymlinkClass::Dangling), 1);
    bind.cleanup();
}

#[test]
fn filtered_examples_apply_limit_after_classification() {
    let f = test_support::materialize(include_str!(
        "../../test-support/fixtures/symlink_loops.toml"
    ));
    let r = analyze_matching(
        &RawPath::from_path(&f.root).unwrap(),
        &CountingGate::default(),
        &AtomicBool::new(false),
        &SymlinkOpts {
            check_targets: true,
            limit: 1,
        },
        |_, _, class| class == SymlinkClass::Dangling,
    )
    .unwrap();
    assert_eq!(r.total, 4);
    assert_eq!(count(&r, SymlinkClass::Loop), 2);
    assert_eq!(r.examples.len(), 1);
    assert_eq!(r.examples[0].2, SymlinkClass::Dangling);
}

#[test]
fn long_target_retries_are_charged() {
    let temp = tempfile::tempdir().unwrap();
    let root = RawPath::from_path(temp.path()).unwrap();
    let link = temp.path().join("link");
    let opts = SymlinkOpts {
        check_targets: false,
        limit: 1,
    };
    let cancel = AtomicBool::new(false);
    let gate = CountingGate::default();
    symlink("short", &link).unwrap();
    analyze(&root, &gate, &cancel, &opts).unwrap();
    let short = gate.0.swap(0, Ordering::Relaxed);
    std::fs::remove_file(&link).unwrap();
    let target = "x".repeat(300);
    symlink(&target, &link).unwrap();
    let report = analyze(&root, &gate, &cancel, &opts).unwrap();
    assert_eq!(report.examples[0].1, target.as_bytes());
    assert_eq!(gate.0.load(Ordering::Relaxed), short + 1);
}

// Compile a private copy so the test-only seam never enters the public fs API.
use tidemark_fs::mount;
#[allow(dead_code)]
#[path = "../src/symlinks.rs"]
mod bounded_resolution;

#[test]
fn selected_mount_boundary_stops_before_external_metadata_or_link_text() {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().join("root");
    std::fs::create_dir(&root).unwrap();
    symlink("missing", temp.path().join("outside")).unwrap();
    symlink("../outside", root.join("second")).unwrap();
    symlink("second", root.join("first")).unwrap();
    symlink("absent", root.join("dangling")).unwrap();
    symlink("loop_b", root.join("loop_a")).unwrap();
    symlink("loop_a", root.join("loop_b")).unwrap();
    std::fs::write(root.join("file"), []).unwrap();
    symlink("file", root.join("valid")).unwrap();
    let raw = RawPath::from_path(&root).unwrap();
    let absolute_out = temp.path().join("outside");
    let absolute_in = root.join("valid");
    use bounded_resolution::SymlinkClass as Class;
    use std::os::unix::ffi::OsStrExt;
    for (target, expected) in [
        (b"../outside".as_slice(), Class::OtherFs),
        (absolute_out.as_os_str().as_bytes(), Class::OtherFs),
        (b"first", Class::OtherFs),
        (b"dangling", Class::Dangling),
        (b"loop_a", Class::Loop),
        (absolute_in.as_os_str().as_bytes(), Class::InsideRoot),
        (b"valid/../file", Class::Dangling),
    ] {
        let (class, calls) =
            bounded_resolution::test_check_target(&raw, target, &CountingGate::default());
        assert_eq!(class, expected, "target {target:?}");
        assert!(calls.iter().all(|(path, _)| path.starts_with(&raw)));
        assert!(calls
            .iter()
            .all(|(path, _)| !path.as_bytes().ends_with(b"outside")));
    }
}

#[cfg(feature = "privileged")]
#[test]
fn cross_mount_chain_preserves_second_link_atime_pending_vm() {
    use std::os::unix::fs::MetadataExt;
    let mut first = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "strictatime");
    let mut second = test_support::xfs::LoopXfs::new(512 * 1024 * 1024, &[], "strictatime");
    let outside = second.mountpoint.join("outside");
    symlink("missing", &outside).unwrap();
    let old = rustix::fs::Timespec {
        tv_sec: 1_000_000_000,
        tv_nsec: 123,
    };
    rustix::fs::utimensat(
        rustix::fs::CWD,
        &outside,
        &rustix::fs::Timestamps {
            last_access: old,
            last_modification: old,
        },
        rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
    )
    .unwrap();
    symlink(&outside, first.mountpoint.join("link")).unwrap();
    let report = analyze(
        &RawPath::from_path(&first.mountpoint).unwrap(),
        &CountingGate::default(),
        &AtomicBool::new(false),
        &SymlinkOpts {
            check_targets: true,
            limit: 10,
        },
    )
    .unwrap();
    assert_eq!(count(&report, SymlinkClass::OtherFs), 1);
    assert_eq!(count(&report, SymlinkClass::Dangling), 0);
    assert_eq!(count(&report, SymlinkClass::Loop), 0);
    let metadata = std::fs::symlink_metadata(&outside).unwrap();
    assert_eq!(
        (metadata.atime(), metadata.atime_nsec()),
        (old.tv_sec, old.tv_nsec)
    );
    first.cleanup().unwrap();
    second.cleanup().unwrap();
}
