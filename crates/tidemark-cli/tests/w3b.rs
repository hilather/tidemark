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
fn scanned_hardlink_web_crossings_and_paths() {
    use std::os::unix::fs::MetadataExt;
    let f = test_support::materialize(include_str!(
        "../../test-support/fixtures/hardlink_web.toml"
    ));
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
    let r = run(
        &cfg,
        &[
            "links".as_ref(),
            f.root.as_os_str(),
            "--boundary-depth".as_ref(),
            "1".as_ref(),
            "--top".as_ref(),
            "100".as_ref(),
            "--show-paths".as_ref(),
        ],
    );
    let data = &r["data"];
    assert_eq!(data["totals"]["sets"], 49);
    assert_eq!(data["totals"]["crossing_sets"], 49);
    assert_eq!(data["totals"]["instances"], (2..=50u64).sum::<u64>());
    assert_eq!(data["totals"]["incomplete_sets"], 2);
    assert_eq!(data["region_pairs"][0]["sets"], 49);
    let bytes = (2..=50)
        .map(|s| {
            std::fs::symlink_metadata(f.root.join(format!("d00/s{s}")))
                .unwrap()
                .blocks()
                * 512
        })
        .sum::<u64>();
    assert_eq!(data["totals"]["disk"], bytes);
    assert_eq!(data["totals"]["crossing_disk"], bytes);
    assert_eq!(
        data["paths"].as_array().unwrap().len(),
        (2..=50usize).sum::<usize>()
    );
    assert!(data["paths"]
        .as_array()
        .unwrap()
        .iter()
        .all(|p| p["status"] == "snapshot"));
    let incomplete = run(
        &cfg,
        &[
            "links".as_ref(),
            f.root.as_os_str(),
            "--incomplete".as_ref(),
            "--top".as_ref(),
            "100".as_ref(),
        ],
    );
    assert_eq!(
        incomplete["data"]["largest_sets"].as_array().unwrap().len(),
        2
    );
    let out = Command::new(cargo_bin!("tidemark"))
        .arg("--config")
        .arg(&cfg)
        .arg("links")
        .arg(&f.root)
        .arg("--boundary-depth")
        .arg("1")
        .output()
        .unwrap();
    assert!(out.status.success());
    let text = String::from_utf8(out.stdout).unwrap();
    for section in [
        "Link sets:",
        "Incomplete:",
        "Crossing:",
        "Pinned directories:",
        "Largest link sets:",
        "Most shared directories:",
    ] {
        assert!(text.contains(section));
    }
}
#[test]
fn symlink_filters_preserve_total_counts() {
    let f = test_support::materialize(include_str!(
        "../../test-support/fixtures/symlink_loops.toml"
    ));
    let tmp = tempfile::tempdir().unwrap();
    let cfg = config(tmp.path());
    let r = run(
        &cfg,
        &[
            "symlinks".as_ref(),
            f.root.as_os_str(),
            "--check-targets".as_ref(),
            "--dangling".as_ref(),
            "--limit".as_ref(),
            "1".as_ref(),
        ],
    );
    assert_eq!(r["schema"], "tidemark.symlinks.v1");
    assert_eq!(r["data"]["total"], 4);
    let examples = r["data"]["examples"].as_array().unwrap();
    assert_eq!(examples.len(), 1);
    assert_eq!(examples[0]["class"], "dangling");
    assert_eq!(examples[0]["target"]["path"], "absent");
    let out = Command::new(cargo_bin!("tidemark"))
        .arg("--config")
        .arg(&cfg)
        .arg("symlinks")
        .arg(&f.root)
        .arg("--dangling")
        .output()
        .unwrap();
    assert_eq!(out.status.code(), Some(2));
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
