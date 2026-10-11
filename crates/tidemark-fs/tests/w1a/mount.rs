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
    assert_eq!(m[2].root, raw(b"/postgres"));
    assert_eq!(m[3].mount_point, raw(b"/data/with space"));
    assert_eq!(m[4].mount_point, raw(b"/data/tab\tline\nslash\\"));
    assert_eq!(m[5].mount_point, raw(b"/data/\xff"));
    assert_eq!(mount_for_path(&m, &raw(b"/data/a")).unwrap().id, 99);
    assert_eq!(mount_for_path(&m, &raw(b"/database")).unwrap().id, 20);
    assert_eq!(other_mounts_of_dev(&m, m[1].dev, &raw(b"/data")).len(), 1);
    assert_eq!(submounts_under(&m, &raw(b"/data")).len(), 4);
}
#[test]
fn malformed_records_are_errors() {
    for s in [
        "1 2 8:0 / / rw xfs /dev/sda rw",
        "1 2 8 / / rw - xfs x rw",
        "1 2 8:0 / /bad\\04 rw - xfs x rw",
        "1 2 8:0 / /bad\\777 rw - xfs x rw",
        "1 2 8:0 / /bad\\000 rw - xfs x rw",
    ] {
        assert!(parse_mountinfo(s.as_bytes()).is_err(), "{s}");
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
fn nsfs_nonabsolute_root_requires_contract_decision() {
    assert!(
        parse_mountinfo(b"1 2 0:5 net:[4026533000] /run/netns/test rw - nsfs nsfs rw\n").is_err()
    );
}
