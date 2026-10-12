//! Owned by: W3-B
//! End-to-end scanned hard-link topology and symlink command evidence.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use assert_cmd::cargo::cargo_bin;
use std::{path::Path, process::Command};
fn run(config: &Path, args: &[&std::ffi::OsStr]) -> serde_json::Value {
    let out = Command::new(cargo_bin!("tidemark"))
        .arg("--config")
        .arg(config)
        .arg("--json")
        .args(args)
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}
fn config(tmp: &Path) -> std::path::PathBuf {
    let p = tmp.join("config.toml");
    std::fs::write(
        &p,
        format!(
            "[general]\nstate_dir = {:?}\nsocket = {:?}\n",
            tmp.join("state"),
            tmp.join("socket")
        ),
    )
    .unwrap();
    p
}
#[test]
fn bulk_show_paths_marks_live_and_gone() {
    use tidemark_core::{
        path::{RawName, RawPath},
        scan::ScanMode,
        snapshot::{LinkFilter, SortKey},
        time::ScanId,
    };
    use tidemark_store::{
        catalog::{root_slug, Catalog},
        reader::Snapshot,
        writer::SnapshotWriter,
    };
    let f=test_support::materialize("[[entry]]\npath='a/source'\nkind='file'\nsize=4096\n[[entry]]\npath='b/link'\nkind='hardlink'\ntarget='a/source'\n");
    let tmp = tempfile::tempdir().unwrap();
    let cfg = config(tmp.path());
    run(
        &cfg,
        &[
            "scan".as_ref(),
            f.root.as_os_str(),
            "--mode".as_ref(),
            "walk".as_ref(),
            "--throttle".as_ref(),
            "max".as_ref(),
        ],
    );
    let state = tmp.path().join("state");
    let catalog = Catalog::open(&state, true).unwrap();
    let root = RawPath::from_path(&f.root).unwrap();
    let entry = catalog.latest(&root).unwrap().unwrap();
    let snap = Snapshot::open(entry.file.as_ref().unwrap()).unwrap();
    let mut m = snap.meta().clone();
    m.started_at += 1;
    m.finished_at += 1;
    m.scan_id = ScanId::new(m.started_at).unwrap();
    m.mode = ScanMode::Bulk;
    let mut writer =
        SnapshotWriter::create(&state.join("snapshots").join(root_slug(&root)), &m).unwrap();
    let mut dirs = snap.subtree_dirs(0, u32::MAX, SortKey::Name, 100).unwrap();
    dirs.sort_unstable_by_key(|d| d.id);
    writer.put_dirs(&dirs).unwrap();
    let hits = snap
        .link_sets_under(0, &LinkFilter::default(), 100)
        .unwrap();
    writer
        .put_link_sets(&hits.iter().map(|h| h.row.clone()).collect::<Vec<_>>())
        .unwrap();
    for h in &hits {
        let mut instances = snap.link_instances(h.row.ino).unwrap();
        for i in &mut instances {
            i.name = None;
        }
        writer.put_link_instances(&instances).unwrap();
    }
    writer.finish(&m).unwrap();
    catalog.rebuild_from_files().unwrap();
    std::fs::remove_file(f.root.join("b/link")).unwrap();
    // A second live name in the same directory must be found as well.
    std::fs::hard_link(f.root.join("a/source"), f.root.join("a/second")).unwrap();
    let r = run(
        &cfg,
        &[
            "links".as_ref(),
            f.root.as_os_str(),
            "--show-paths".as_ref(),
        ],
    );
    let paths = r["data"]["paths"].as_array().unwrap();
    assert_eq!(paths.iter().filter(|p| p["status"] == "live").count(), 2);
    assert_eq!(paths.iter().filter(|p| p["status"] == "gone").count(), 1);
    assert!(!paths.iter().any(|p| p["instance"]
        == root.join(&RawName::new(b"a".to_vec()).unwrap()).to_json()
        && p["status"] == "gone"));
    // Replacing a recorded directory with another inode must be reported gone.
    std::fs::rename(f.root.join("b"), f.root.join("old-b")).unwrap();
    std::fs::create_dir(f.root.join("b")).unwrap();
    std::fs::hard_link(f.root.join("a/source"), f.root.join("b/replacement")).unwrap();
    let replaced = run(
        &cfg,
        &[
            "links".as_ref(),
            f.root.as_os_str(),
            "--show-paths".as_ref(),
        ],
    );
    let paths = replaced["data"]["paths"].as_array().unwrap();
    assert_eq!(paths.iter().filter(|p| p["status"] == "live").count(), 2);
    assert_eq!(paths.iter().filter(|p| p["status"] == "gone").count(), 1);
    // A symlink replacement must never expose names outside the scan root.
    std::fs::remove_file(f.root.join("b/replacement")).unwrap();
    std::fs::remove_dir(f.root.join("b")).unwrap();
    let outside = tempfile::tempdir().unwrap();
    std::os::unix::fs::symlink(outside.path(), f.root.join("b")).unwrap();
    let replaced = run(
        &cfg,
        &[
            "links".as_ref(),
            f.root.as_os_str(),
            "--show-paths".as_ref(),
        ],
    );
    let paths = replaced["data"]["paths"].as_array().unwrap();
    assert_eq!(paths.iter().filter(|p| p["status"] == "live").count(), 2);
    assert_eq!(paths.iter().filter(|p| p["status"] == "gone").count(), 1);
}
#[test]
fn text_and_json_report_snapshots() {
    use tidemark_core::path::RawPath;
    use tidemark_fs::symlinks::{SymlinkClass, SymlinkReport};
    use tidemark_store::links::{LinkReport, LinkTotals};
    let root = RawPath::new(b"/r".to_vec()).unwrap();
    let symlinks = SymlinkReport {
        total: 2,
        absolute: 0,
        relative: 2,
        by_class: vec![(SymlinkClass::Dangling, 1), (SymlinkClass::Loop, 1)],
        examples: vec![
            (
                RawPath::new(b"/r/a".to_vec()).unwrap(),
                b"../missing".to_vec(),
                SymlinkClass::Dangling,
            ),
            (
                RawPath::new(b"/r/\xff".to_vec()).unwrap(),
                vec![255],
                SymlinkClass::Loop,
            ),
        ],
    };
    let links = LinkReport {
        totals: LinkTotals {
            sets: 3,
            instances: 5,
            disk: 24576,
            incomplete_sets: 1,
            incomplete_disk: 8192,
            ..Default::default()
        },
        crossing: vec![],
        region_pairs: vec![],
        incomplete_top_dirs: vec![(RawPath::new(b"/r/b".to_vec()).unwrap(), 8192, 1)],
        largest_sets: vec![],
        most_shared_dirs: vec![(RawPath::new(b"/r/b".to_vec()).unwrap(), 12288, 12288)],
    };
    insta::with_settings!({snapshot_path=>"w3b/snapshots"}, {
        let mut output=Vec::new();
        tidemark_cli::render::symlinks::report(&root,&symlinks,false,&mut output).unwrap();
        insta::assert_snapshot!("symlinks_text",String::from_utf8(output).unwrap());
        let mut output=Vec::new();
        tidemark_cli::render::symlinks::report(&root,&symlinks,true,&mut output).unwrap();
        insta::assert_json_snapshot!("symlinks_json",serde_json::from_slice::<serde_json::Value>(&output).unwrap());
        let mut output=Vec::new();
        tidemark_cli::render::links::report(&links,None,None,&[],false,&mut output).unwrap();
        insta::assert_snapshot!("links_text",String::from_utf8(output).unwrap());
        let mut output=Vec::new();
        tidemark_cli::render::links::report(&links,None,None,&[],true,&mut output).unwrap();
        insta::assert_json_snapshot!("links_json",serde_json::from_slice::<serde_json::Value>(&output).unwrap());
    });
}

#[test]
#[ignore = "scale"]
fn million_set_cli_timing() {
    use tidemark_core::{
        path::RawPath,
        snapshot::{LinkInstanceRow, LinkSetRow, SortKey},
        time::ScanId,
    };
    use tidemark_store::{
        catalog::{root_slug, Catalog},
        reader::Snapshot,
        writer::SnapshotWriter,
    };
    let f = test_support::materialize("[[entry]]\npath='a/x/source'\nkind='file'\nsize=1\n[[entry]]\npath='b/y/link'\nkind='hardlink'\ntarget='a/x/source'\n");
    let tmp = tempfile::tempdir().unwrap();
    let cfg = config(tmp.path());
    run(
        &cfg,
        &[
            "scan".as_ref(),
            f.root.as_os_str(),
            "--mode".as_ref(),
            "walk".as_ref(),
            "--throttle".as_ref(),
            "max".as_ref(),
        ],
    );
    let state = tmp.path().join("state");
    let catalog = Catalog::open(&state, true).unwrap();
    let root = RawPath::from_path(&f.root).unwrap();
    let entry = catalog.latest(&root).unwrap().unwrap();
    let snap = Snapshot::open(entry.file.as_ref().unwrap()).unwrap();
    let mut meta = snap.meta().clone();
    meta.started_at += 1;
    meta.finished_at += 1;
    meta.scan_id = ScanId::new(meta.started_at).unwrap();
    let mut writer =
        SnapshotWriter::create(&state.join("snapshots").join(root_slug(&root)), &meta).unwrap();
    let mut dirs = snap.subtree_dirs(0, u32::MAX, SortKey::Name, 100).unwrap();
    dirs.sort_unstable_by_key(|d| d.id);
    let leaves: Vec<_> = dirs.iter().filter(|d| d.depth == 2).map(|d| d.id).collect();
    assert_eq!(leaves.len(), 2);
    writer.put_dirs(&dirs).unwrap();
    for base in (0..1_000_000u64).step_by(10_000) {
        let sets: Vec<_> = (base..base + 10_000)
            .map(|ino| LinkSetRow {
                ino,
                nlink: 2,
                found: 2,
                complete: true,
                disk: 1,
                apparent: 1,
                mtime: 0,
                lca_dir_id: 0,
            })
            .collect();
        let instances: Vec<_> = (base..base + 10_000)
            .flat_map(|ino| {
                leaves.iter().map(move |&dir_id| LinkInstanceRow {
                    ino,
                    dir_id,
                    name: None,
                })
            })
            .collect();
        writer.put_link_sets(&sets).unwrap();
        writer.put_link_instances(&instances).unwrap();
    }
    writer.finish(&meta).unwrap();
    catalog.rebuild_from_files().unwrap();
    let start = std::time::Instant::now();
    let report = run(
        &cfg,
        &[
            "links".as_ref(),
            f.root.as_os_str(),
            "--boundary-depth".as_ref(),
            "2".as_ref(),
        ],
    );
    let elapsed = start.elapsed();
    eprintln!("one million link sets, full CLI boundary depth 2: {elapsed:?}");
    assert_eq!(report["data"]["totals"]["crossing_sets"], 1_000_000);
    assert!(
        elapsed < std::time::Duration::from_secs(10),
        "elapsed {elapsed:?}"
    );
}
