//! Owned by: W1-A
//! Synthetic sysfs stacked topology and ordered sampling tests.
use super::*;
use std::os::unix::fs::symlink;
fn device(root: &Path, name: &str, number: &str) -> PathBuf {
    let path = root.join(name);
    std::fs::create_dir_all(path.join("slaves")).unwrap();
    std::fs::create_dir_all(path.join("queue")).unwrap();
    std::fs::write(path.join("dev"), number).unwrap();
    std::fs::write(path.join("queue/rotational"), "1\n").unwrap();
    std::fs::write(path.join("queue/scheduler"), "mq-deadline [bfq] none\n").unwrap();
    path
}
#[test]
fn dm_md_partitions_resolve_to_unique_sorted_disks() {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path();
    let a = device(root, "sda", "8:0");
    let b = device(root, "sdb", "8:16");
    let part = device(&a, "sda1", "8:1");
    std::fs::write(part.join("partition"), "1").unwrap();
    let md = device(root, "md0", "9:0");
    std::fs::create_dir(md.join("md")).unwrap();
    std::fs::write(md.join("md/level"), "raid5").unwrap();
    symlink(&part, md.join("slaves/sda1")).unwrap();
    symlink(&b, md.join("slaves/sdb")).unwrap();
    let dm = device(root, "dm-0", "253:0");
    symlink(&md, dm.join("slaves/md0")).unwrap();
    symlink(&a, dm.join("slaves/sda")).unwrap();
    symlink(&dm, root.join("253:0")).unwrap();
    let leaves = resolve_at(root, 253, 0).unwrap();
    assert_eq!(resolve_graph_at(root, 253, 0).unwrap().1, ["9:0 raid5"]);
    assert_eq!(
        leaves.iter().map(|d| d.name.as_str()).collect::<Vec<_>>(),
        ["sda", "sdb"]
    );
    assert!(leaves
        .iter()
        .all(|d| d.rotational == Some(true) && d.scheduler.as_deref() == Some("bfq")));
}
#[test]
fn disk_stat_variants_and_invalid_input() {
    for tail in ["", " 12 13 14 15", " 12 13 14 15 16 17"] {
        let stat = parse_disk_stat(&format!("1 2 3 4 5 6 7 8 9 10 11{tail}")).unwrap();
        assert_eq!(stat.read_ios, 1);
        assert_eq!(stat.io_ticks_ms, 10);
        assert_eq!(stat.time_in_queue_ms, 11);
    }
    for line in [
        "",
        "1 2",
        "-1 2 3 4 5 6 7 8 9 10 11",
        "1 2 3 4 5 6 7 8 9 10 11 bad",
    ] {
        assert!(parse_disk_stat(line).is_none());
    }
}
#[test]
fn samples_preserve_order_duplicates_and_fail_atomically() {
    let dir = tempfile::tempdir().unwrap();
    let mut devices = Vec::new();
    for (minor, count) in [(0, 10), (1, 20)] {
        let path = dir.path().join(format!("8:{minor}"));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(path.join("stat"), format!("{count} 0 0 0 0 0 0 0 0 0 0")).unwrap();
        devices.push(BlockDevInfo {
            name: format!("disk{minor}"),
            major: 8,
            minor,
            kind: DevKind::Disk,
            rotational: None,
            scheduler: None,
        });
    }
    let requested = vec![devices[1].clone(), devices[0].clone(), devices[1].clone()];
    assert_eq!(
        read_stats_at(dir.path(), &requested)
            .unwrap()
            .iter()
            .map(|s| s.read_ios)
            .collect::<Vec<_>>(),
        [20, 10, 20]
    );
    std::fs::remove_file(dir.path().join("8:0/stat")).unwrap();
    assert!(read_stats_at(dir.path(), &requested).is_err());
    assert!(read_disk_stats(&[]).unwrap().is_empty());
}

#[test]
fn missing_attributes_are_unknown_and_cycles_are_bounded() {
    let dir = tempfile::tempdir().unwrap();
    let disk = device(dir.path(), "loop0", "7:0");
    std::fs::remove_file(disk.join("queue/scheduler")).unwrap();
    std::fs::write(disk.join("queue/rotational"), "bad").unwrap();
    symlink(&disk, dir.path().join("7:0")).unwrap();
    let leaves = resolve_at(dir.path(), 7, 0).unwrap();
    assert!(leaves[0].scheduler.is_none());
    assert!(leaves[0].rotational.is_none());
    symlink(&disk, disk.join("slaves/self")).unwrap();
    assert!(resolve_at(dir.path(), 7, 0).is_err());
}
