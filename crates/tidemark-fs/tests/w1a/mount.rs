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
// Recorded from a btrfs host: mountinfo shows the filesystem's anonymous
// device (0:29); stat on the /@home subvolume reports 0:51.
const BTRFS_HOME: &[u8] = b"56 32 0:29 /@home /home rw,relatime shared:152 - btrfs /dev/mapper/root rw,subvolid=257,subvol=/@home\n";
const XFS_DATA: &[u8] = b"99 1 253:2 / /data rw,relatime - xfs /dev/mapper/rl-data rw\n";
fn must_not_probe() -> io::Result<bool> {
    panic!("descriptor filesystem probed outside the btrfs branch")
}
#[test]
fn root_identity_keeps_device_check_off_btrfs() {
    for (sample, other_dev) in [
        (XFS_DATA, libc::makedev(253, 3)),
        (
            b"264 32 0:57 / /tmp rw - tmpfs tmpfs rw\n".as_slice(),
            libc::makedev(0, 58),
        ),
    ] {
        let mounts = parse_mountinfo(sample).unwrap();
        let m = &mounts[0];
        let id = u64::from(m.id);
        assert!(identity_matches(m, m.dev, Some(id), must_not_probe).unwrap());
        assert!(identity_matches(m, m.dev, None, must_not_probe).unwrap());
        assert!(!identity_matches(m, m.dev, Some(id + 1), must_not_probe).unwrap());
        assert!(!identity_matches(m, other_dev, Some(id), must_not_probe).unwrap());
        assert!(!identity_matches(m, other_dev, None, must_not_probe).unwrap());
    }
}
#[test]
fn root_identity_on_btrfs_subvolume_needs_mount_id_and_btrfs_descriptor() {
    let mounts = parse_mountinfo(BTRFS_HOME).unwrap();
    let m = &mounts[0];
    let subvolume = libc::makedev(0, 51);
    assert!(identity_matches(m, subvolume, Some(56), || Ok(true)).unwrap());
    assert!(!identity_matches(m, subvolume, Some(56), || Ok(false)).unwrap());
    assert!(!identity_matches(m, subvolume, Some(57), || Ok(true)).unwrap());
    // Without a mount ID, fail closed exactly like the device-only check.
    assert!(!identity_matches(m, subvolume, None, || Ok(true)).unwrap());
    assert!(identity_matches(m, m.dev, None, must_not_probe).unwrap());
    let cancelled = identity_matches(m, subvolume, Some(56), || {
        Err(io::Error::new(io::ErrorKind::Interrupted, "scan cancelled"))
    });
    assert_eq!(cancelled.unwrap_err().kind(), io::ErrorKind::Interrupted);
}
struct CountGate {
    units: std::sync::atomic::AtomicU32,
    cancel_at: u32,
    cancel: AtomicBool,
}
impl CountGate {
    fn new(cancel_at: u32) -> Self {
        Self {
            units: 0.into(),
            cancel_at,
            cancel: AtomicBool::new(false),
        }
    }
    fn units(&self) -> u32 {
        self.units.load(Ordering::Relaxed)
    }
}
impl Gate for CountGate {
    fn acquire(&self, cost: u32) {
        let total = self.units.fetch_add(cost, Ordering::Relaxed) + cost;
        if total == self.cancel_at {
            self.cancel.store(true, Ordering::Release);
        }
    }
    fn current_rate(&self) -> f64 {
        0.0
    }
}
#[test]
fn mount_id_sources_are_gated_in_order() {
    let gate = CountGate::new(0);
    let id = mount_id_with(&gate, &gate.cancel, || Some(7), || panic!("fdinfo read")).unwrap();
    assert_eq!((id, gate.units()), (Some(7), 1));
    let gate = CountGate::new(0);
    let id = mount_id_with(&gate, &gate.cancel, || None, || Some(8)).unwrap();
    assert_eq!((id, gate.units()), (Some(8), 2));
    let gate = CountGate::new(0);
    let id = mount_id_with(&gate, &gate.cancel, || None, || None).unwrap();
    assert_eq!((id, gate.units()), (None, 2));
}
#[test]
fn mount_id_sources_stop_once_cancelled() {
    let gate = CountGate::new(1);
    let err = mount_id_with(&gate, &gate.cancel, || panic!("statx"), || panic!("fdinfo"));
    assert_eq!(err.unwrap_err().kind(), io::ErrorKind::Interrupted);
    let gate = CountGate::new(2);
    let err = mount_id_with(&gate, &gate.cancel, || None, || panic!("fdinfo"));
    assert_eq!(err.unwrap_err().kind(), io::ErrorKind::Interrupted);
}
#[test]
fn fdinfo_mount_id_parser() {
    assert_eq!(
        parse_fdinfo_mount_id("pos:\t0\nflags:\t02200000\nmnt_id:\t56\nino:\t256\n"),
        Some(56)
    );
    assert_eq!(parse_fdinfo_mount_id("mnt_id: 42\nmnt_id: 99"), Some(42));
    for text in [
        "",
        "pos:\t0\n",
        "mnt_id:",
        "mnt_id: x",
        "mnt_id: -1",
        "mnt_id: 18446744073709551616",
    ] {
        assert_eq!(parse_fdinfo_mount_id(text), None, "{text:?}");
    }
}
#[test]
fn open_root_is_on_its_mountinfo_record() {
    use std::os::fd::AsFd;
    let dir = test_support::private_tempdir().unwrap();
    let file = std::fs::File::open(dir.path()).unwrap();
    let fd = file.as_fd();
    let mounts = read_mountinfo().unwrap();
    let path = RawPath::from_path(dir.path()).unwrap();
    let m = mount_for_path(&mounts, &path).unwrap();
    // fdinfo is the EL8 production source; statx must agree when it has an ID.
    assert_eq!(fdinfo_mount_id(fd), Some(u64::from(m.id)));
    if let Some(id) = statx_mount_id(fd) {
        assert_eq!(id, u64::from(m.id));
    }
    let st_dev = rustix::fs::fstat(fd).unwrap().st_dev;
    let gate = CountGate::new(0);
    assert!(descriptor_on_mount(fd, st_dev, m, &gate, &gate.cancel).unwrap());
    if st_dev != m.dev {
        // A btrfs subvolume: the case the device-only check refused.
        assert_eq!(m.fs_type, "btrfs");
        assert!(is_btrfs(fd));
    }
}
