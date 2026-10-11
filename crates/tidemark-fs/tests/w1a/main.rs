//! Owned by: W1-A
//! VM-only mount, loop-XFS and loop-LVM acceptance tests.
#[cfg(feature = "privileged")]
mod privileged {
    use std::{os::unix::fs::MetadataExt, process::Command};
    use tidemark_core::{config::Config, path::RawPath};
    use tidemark_fs::{blockdev, mount, probe};
    #[test]
    fn probe_loop_xfs_geometry_matches_xfs_info() {
        let fs = test_support::xfs::LoopXfs::new(1024 * 1024 * 1024, &[], "noatime");
        let mut cfg = Config::default();
        cfg.roots.clear();
        let report = probe::probe(&cfg, &[RawPath::from_path(&fs.mountpoint).unwrap()]);
        let root = &report.roots[0];
        assert_eq!(root.facts["fs_type"].value.as_deref(), Some("xfs"));
        let geometry = root.facts["xfs_geometry"]
            .value
            .as_ref()
            .expect("geometry must be known after W1-C integration");
        let info = Command::new("xfs_info")
            .arg(&fs.mountpoint)
            .output()
            .unwrap();
        assert!(info.status.success());
        let text = String::from_utf8(info.stdout).unwrap();
        let blocksize = text
            .lines()
            .find(|l| l.starts_with("data"))
            .unwrap()
            .split_whitespace()
            .find_map(|w| w.strip_prefix("bsize="))
            .unwrap();
        assert!(geometry.contains(&format!("blocksize={blocksize} ")));
        let inode_size = text
            .split_whitespace()
            .find_map(|w| w.strip_prefix("isize="))
            .unwrap();
        assert!(geometry.contains(&format!("inodesize={inode_size} ")));
    }
    #[test]
    fn resolve_loop_lvm_returns_pv_disk() {
        let vg = test_support::lvm::LoopVg::new(
            "w1a",
            2 * 1024 * 1024 * 1024,
            test_support::lvm::LvLayout {
                linear: Some(1024 * 1024 * 1024),
                thin_pool: None,
            },
        );
        let dev = std::fs::metadata(&vg.mounts[0].mountpoint).unwrap().dev();
        let leaves = blockdev::resolve_leaves(dev).unwrap();
        assert_eq!(leaves.len(), 1);
        let result = Command::new("lvs")
            .args(["--noheadings", "-o", "devices", "--select"])
            .arg(format!("vg_name={}", vg.name()))
            .output()
            .unwrap();
        assert!(result.status.success());
        let pv = String::from_utf8(result.stdout).unwrap();
        let number = std::fs::metadata(pv.trim().split_once('(').unwrap().0)
            .unwrap()
            .rdev();
        assert_eq!(
            (leaves[0].major, leaves[0].minor),
            (libc::major(number), libc::minor(number))
        );
    }
    #[test]
    fn resolve_vm_md_array_matches_lsblk_members() {
        // The VM harness supplies an existing disposable RAID array; this test is read-only.
        let device = std::env::var_os("TIDEMARK_W1A_MD_DEVICE")
            .expect("set TIDEMARK_W1A_MD_DEVICE to a VM RAID array device");
        let number = std::fs::metadata(&device).unwrap().rdev();
        let sys = format!(
            "/sys/dev/block/{}:{}/md/level",
            libc::major(number),
            libc::minor(number)
        );
        assert!(std::fs::read_to_string(sys)
            .unwrap()
            .trim()
            .starts_with("raid"));
        let output = Command::new("lsblk")
            .args(["--json", "--inverse", "--output", "TYPE,MAJ:MIN"])
            .arg(&device)
            .output()
            .unwrap();
        assert!(output.status.success());
        let json: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let mut pending: Vec<_> = json["blockdevices"].as_array().unwrap().iter().collect();
        let mut expected = std::collections::BTreeSet::new();
        let mut visited = 0;
        while let Some(node) = pending.pop() {
            visited += 1;
            assert!(visited <= 4096);
            if matches!(node["type"].as_str(), Some("disk" | "loop")) {
                expected.insert(node["maj:min"].as_str().unwrap().to_owned());
            }
            if let Some(children) = node["children"].as_array() {
                pending.extend(children);
            }
        }
        assert!(
            expected.len() >= 2,
            "RAID acceptance needs at least two members"
        );
        let actual: std::collections::BTreeSet<_> = blockdev::resolve_leaves(number)
            .unwrap()
            .iter()
            .map(|d| format!("{}:{}", d.major, d.minor))
            .collect();
        assert_eq!(actual, expected);
    }
    struct Bind(std::path::PathBuf);
    impl Drop for Bind {
        fn drop(&mut self) {
            let status = Command::new("umount").arg(&self.0).status().unwrap();
            assert!(status.success());
        }
    }
    #[test]
    fn real_bind_mount_is_a_same_device_submount() {
        let fs = test_support::xfs::LoopXfs::new(1024 * 1024 * 1024, &[], "noatime");
        let source = fs.mountpoint.join("source");
        let target = fs.mountpoint.join("bind");
        std::fs::create_dir(&source).unwrap();
        std::fs::create_dir(&target).unwrap();
        assert!(Command::new("mount")
            .arg("--bind")
            .arg(&source)
            .arg(&target)
            .status()
            .unwrap()
            .success());
        let _bind = Bind(target.clone());
        let mounts = mount::read_mountinfo().unwrap();
        let root = RawPath::from_path(&fs.mountpoint).unwrap();
        let target = RawPath::from_path(&target).unwrap();
        let dev = std::fs::metadata(&fs.mountpoint).unwrap().dev();
        assert!(mount::submounts_under(&mounts, &root).contains(&target));
        assert!(mount::other_mounts_of_dev(&mounts, dev, &root)
            .iter()
            .any(|m| m.mount_point == target));
    }
}
