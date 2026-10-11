//! Owned by: W1-A
//! Pure recommendations and metadata parser tests.
use super::*;
#[test]
fn advisory_rules_are_independent_and_unknowns_do_not_warn() {
    let mut host = HostFacts::default();
    let mut root = RootFacts {
        root: RawPath::new(b"/data".to_vec()).unwrap(),
        facts: BTreeMap::new(),
        leaves: None,
        mount_point: None,
        other_mounts: None,
    };
    assert!(recommendations(&host, std::slice::from_ref(&root)).is_empty());
    for (key, expected, code) in [
        ("rocky_latest_minor", "false", "update_rocky"),
        ("mount_namespace_equal", "false", "mount_namespace"),
    ] {
        host.facts.insert(key.into(), Fact::known(expected));
        assert!(recommendations(&host, &[]).iter().any(|r| r.code == code));
        host.facts.clear();
    }
    for (key, expected, code) in [
        ("xfs_ftype", "false", "ftype"),
        ("xfs_reflink", "true", "reflink"),
    ] {
        root.facts.insert(key.into(), Fact::known(expected));
        assert!(recommendations(&host, std::slice::from_ref(&root))
            .iter()
            .any(|r| r.code == code));
        root.facts.clear();
    }
    root.other_mounts = Some(vec![serde_json::json!({"path":"/bind"})]);
    assert_eq!(
        recommendations(&host, std::slice::from_ref(&root))[0].code,
        "bind_mounts"
    );
    root.other_mounts = Some(Vec::new());
    root.leaves = Some(vec![tidemark_core::dev::BlockDevInfo {
        name: "sda".into(),
        major: 8,
        minor: 0,
        kind: tidemark_core::dev::DevKind::Disk,
        rotational: Some(true),
        scheduler: Some("mq-deadline".into()),
    }]);
    assert_eq!(
        recommendations(&host, std::slice::from_ref(&root))[0].code,
        "scheduler"
    );
    root.leaves.as_mut().unwrap()[0].scheduler = None;
    assert!(recommendations(&host, &[root]).is_empty());
}
#[test]
fn host_metadata_parsers() {
    let os = parse_os_release(
        "# ignored\nID=rocky\nVERSION_ID=\"8.10\"\nPRETTY_NAME='Rocky Linux 8.10'\n",
    );
    assert_eq!(os["VERSION_ID"], "8.10");
    assert_eq!(os["PRETTY_NAME"], "Rocky Linux 8.10");
    assert_eq!(
        parse_named("Name:\ttidemark\nCapEff:\t0000000000000000\n", "CapEff").as_deref(),
        Some("0000000000000000")
    );
    assert_eq!(
        parse_named("MemTotal:  123 kB\n", "MemTotal").as_deref(),
        Some("123 kB")
    );
}
#[test]
fn missing_root_is_reported_without_panicking() {
    let mut cfg = Config::default();
    cfg.roots.clear();
    let root = RawPath::new(b"/tidemark-nonexistent-w1a-root".to_vec()).unwrap();
    let report = probe(&cfg, &[root.clone(), root]);
    assert_eq!(report.roots.len(), 1);
    assert!(report.roots[0].facts["canonical_path"].why.is_some());
    assert!(report.roots[0].facts["total_bytes"].value.is_none());
    assert!(report.roots[0].facts["fanotify_available"].why.is_some());
    assert!(report.roots[0]
        .facts
        .values()
        .all(|fact| fact.value.is_none() && fact.why.is_some()));
    assert!(report.roots[0].mount_point.is_none());
    assert!(report.roots[0].other_mounts.is_none());
}

#[test]
fn configured_paths_resolve_before_normalizing() {
    let dir = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink("/proc/sys/kernel", dir.path().join("link")).unwrap();
    std::fs::create_dir(dir.path().join("existing")).unwrap();
    let cfg = Config {
        roots: vec![
            tidemark_core::config::RootConfig {
                path: dir.path().join("link/.."),
                ..Default::default()
            },
            tidemark_core::config::RootConfig {
                path: dir.path().join("missing/../existing"),
                ..Default::default()
            },
        ],
        ..Default::default()
    };
    let report = probe(&cfg, &[]);
    assert_eq!(
        report.roots[0].root,
        canon::canonicalize(std::path::Path::new("/proc/sys")).unwrap()
    );
    assert_eq!(
        report.roots[0].facts["fs_type"].value.as_deref(),
        Some("proc")
    );
    let missing = &report.roots[1];
    assert!(missing.mount_point.is_none());
    assert!(missing.leaves.is_none());
    assert!(missing.other_mounts.is_none());
    assert!(missing
        .facts
        .values()
        .all(|f| f.value.is_none() && f.why.is_some()));
}
