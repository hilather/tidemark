//! Owned by: W1-A-fix
//! CLI path arguments preserve realpath semantics.
use super::*;
#[test]
fn cli_paths_resolve_before_normalizing() {
    let dir = tempfile::tempdir().unwrap();
    let base = dir.path();
    std::fs::create_dir_all(base.join("existing")).unwrap();
    std::os::unix::fs::symlink("/proc/sys/kernel", base.join("link")).unwrap();
    let mut cfg = tidemark_core::config::Config::default();
    cfg.roots.clear();
    let report = filesystem_report(
        &Args {
            root: vec![base.join("link/.."), base.join("missing/../existing")],
        },
        &cfg,
    );
    assert_eq!(
        report.roots[0].root.as_path(),
        std::path::Path::new("/proc/sys")
    );
    assert_eq!(
        report.roots[0].facts["fs_type"].value.as_deref(),
        Some("proc")
    );
    assert!(report.roots[1]
        .facts
        .values()
        .all(|f| f.value.is_none() && f.why.is_some()));
}
