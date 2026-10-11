//! Owned by: W1-A
//! Recorded-format mountinfo parser and mount selection tests.
use super::*;
fn raw(path: &[u8]) -> RawPath {
    RawPath::new(path.to_vec()).unwrap()
}
#[test]
fn mountinfo_samples_and_bind_mounts() {
    let m = parse_mountinfo(include_bytes!("data/mountinfo")).unwrap();
    assert_eq!(m.len(), 7);
    assert_eq!(m[1].source, "/dev/mapper/rl-data");
    assert_eq!(m[2].root, MountRoot::Path(raw(b"/database")));
    assert_eq!(m[3].mount_point, raw(b"/data/with space"));
    assert_eq!(m[4].mount_point, raw(b"/data/tab\tline\nslash\\"));
    assert_eq!(m[5].mount_point, raw(b"/data/\xff"));
    assert_eq!(mount_for_path(&m, &raw(b"/data/a")).unwrap().id, 99);
    assert_eq!(mount_for_path(&m, &raw(b"/database")).unwrap().id, 20);
    assert_eq!(other_mounts_of_dev(&m, m[1].dev, &raw(b"/data")).len(), 1);
    assert_eq!(submounts_under(&m, &raw(b"/data")).len(), 4);
}
#[test]
fn malformed_records_preserve_neighbors() {
    let mounts = parse_mountinfo(
        b"1 0 8:0 / /root rw - xfs /dev/sda rw\n2 1 8:0 /sub /root/bind rw - xfs /dev/sda\n",
    )
    .unwrap();
    assert_eq!(
        submounts_under(&mounts, &raw(b"/root")),
        vec![raw(b"/root/bind")]
    );
    assert!(mounts[1].fs_type.is_empty());
    assert!(mounts[1].super_opts.is_empty());
    for line in [
        b"1 2 8:0 / /bad\\04 rw - xfs x rw".as_slice(),
        b"1 2 8:0 / /bad\\777 rw - xfs x rw",
        b"1 2 8:0 / /bad\\000 rw - xfs x rw",
        b"1 2 8 / / rw - xfs x rw",
    ] {
        assert!(parse_mountinfo(line).is_err());
    }
    assert!(parse_mountinfo(b"").unwrap().is_empty());
}
#[test]
fn el8_shaped_lvm_md_and_bind_records() {
    for sample in [
        include_bytes!("data/mountinfo-el8-lvm").as_slice(),
        include_bytes!("data/mountinfo-el8-md").as_slice(),
        include_bytes!("data/mountinfo-el8-bind").as_slice(),
    ] {
        let mounts = parse_mountinfo(sample).unwrap();
        assert_eq!(
            mount_for_path(&mounts, &raw(b"/data/db mirror/a"))
                .unwrap()
                .fs_type,
            "xfs"
        );
    }
}
#[test]
fn nsfs_object_root_keeps_table_and_boundaries() {
    let mounts = parse_mountinfo(b"1 0 8:0 / / rw - xfs /dev/sda rw\n2 1 0:5 net:[4026533000] /run/netns/test rw - nsfs nsfs rw\n3 1 8:0 /bind /bind rw - xfs /dev/sda rw\n").unwrap();
    assert_eq!(mounts.len(), 3);
    assert_eq!(
        mounts[1].root,
        MountRoot::Object(b"net:[4026533000]".to_vec())
    );
    assert_eq!(
        mount_for_path(&mounts, &raw(b"/run/netns/test"))
            .unwrap()
            .id,
        2
    );
    assert_eq!(mount_for_path(&mounts, &raw(b"/other")).unwrap().id, 1);
    assert!(submounts_under(&mounts, &raw(b"/")).contains(&raw(b"/run/netns/test")));
    assert_eq!(
        other_mounts_of_dev(&mounts, mounts[0].dev, &raw(b"/")).len(),
        1
    );
    let object = parse_mountinfo(b"1 0 0:5 net:[a\\377] /net rw - nsfs nsfs rw").unwrap();
    assert_eq!(object[0].root, MountRoot::Object(b"net:[a\xff]".to_vec()));
}
#[test]
fn captured_el8_inputs_pending_vm() {
    let directory =
        std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/w1a/data/captured-el8");
    if !directory.is_dir() {
        eprintln!(
            "SKIP: real EL8 mountinfo captures are an unmet acceptance input pending VM access"
        );
        return;
    }
    let mut files: Vec<_> = std::fs::read_dir(directory)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    files.sort();
    assert!((3..=4).contains(&files.len()));
    for file in files {
        assert!(!parse_mountinfo(&std::fs::read(file).unwrap())
            .unwrap()
            .is_empty());
    }
}
