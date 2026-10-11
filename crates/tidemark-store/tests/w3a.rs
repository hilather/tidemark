//! Owned by: W3-A
//! Synthetic snapshot difference and explanation evidence.
#![allow(clippy::unwrap_used, clippy::expect_used)]
mod w3a {
    pub mod support;
}
use tidemark_core::path::RawPath;
use tidemark_store::{diff::*, Snapshot};
use w3a::support::*;
fn p(bytes: &[u8]) -> RawPath {
    RawPath::new(bytes.to_vec()).unwrap()
}
fn pair() -> (tempfile::TempDir, std::path::PathBuf, std::path::PathBuf) {
    let dir = tempfile::tempdir().unwrap();
    let mut old = vec![
        row(0, None, b"/fixture", 100),
        row(1, Some(0), b"/fixture/a", 60),
        row(2, Some(1), b"/fixture/a/deep", 40),
        row(3, Some(0), b"/fixture/removed", 20),
    ];
    old[0].own_charged = 20;
    old[1].own_charged = 20;
    old[1].subtree_end = 2;
    old[2].depth = 2;
    let mut new = vec![
        row(0, None, b"/fixture", 210),
        row(1, Some(0), b"/fixture/a", 160),
        row(2, Some(1), b"/fixture/a/deep", 140),
        row(3, Some(0), b"/fixture/new", 30),
    ];
    new[0].own_charged = 20;
    new[1].own_charged = 20;
    new[1].subtree_end = 2;
    new[2].depth = 2;
    let a = snapshot(dir.path(), &old, 1000);
    let b = snapshot(dir.path(), &new, 2000);
    (dir, a, b)
}
#[test]
fn classes_depth_missing_targets_and_limits() {
    let (_dir, a, b) = pair();
    let new = Snapshot::open(&b).unwrap();
    let rows = diff(&new, &a, &p(b"/fixture"), DiffMetric::Disk, 3, 20).unwrap();
    assert_eq!(rows.len(), 5);
    assert_eq!(rows[0].delta, 110);
    assert!(rows
        .iter()
        .any(|r| r.class == DiffClass::New && r.delta == 30));
    assert!(rows
        .iter()
        .any(|r| r.class == DiffClass::Removed && r.delta == -20));
    assert_eq!(
        rows.iter()
            .find(|r| r.path == p(b"/fixture/a/deep"))
            .unwrap()
            .depth_rel,
        2
    );
    assert_eq!(
        diff(&new, &a, &p(b"/fixture/a"), DiffMetric::Disk, 0, 20)
            .unwrap()
            .len(),
        1
    );
    assert!(diff(&new, &a, &p(b"/fixture/new"), DiffMetric::Disk, 3, 20)
        .unwrap()
        .iter()
        .all(|r| r.class == DiffClass::New));
    assert!(
        diff(&new, &a, &p(b"/fixture/removed"), DiffMetric::Disk, 3, 20)
            .unwrap()
            .iter()
            .all(|r| r.class == DiffClass::Removed)
    );
    assert!(diff(&new, &a, &p(b"/absent"), DiffMetric::Disk, 3, 20).is_err());
    assert!(diff(&new, &a, &p(b"/fixture"), DiffMetric::Disk, 3, 0)
        .unwrap()
        .is_empty());
    let reverse = diff(
        &Snapshot::open(&a).unwrap(),
        &b,
        &p(b"/fixture"),
        DiffMetric::Disk,
        3,
        20,
    )
    .unwrap();
    assert!(reverse.iter().any(|r| r.class == DiffClass::Shrunk));
    assert_eq!(
        diff(&new, &a, &p(b"/fixture"), DiffMetric::Reclaimable, 0, 1).unwrap()[0].delta,
        55
    );
    assert_eq!(
        diff(&new, &a, &p(b"/fixture"), DiffMetric::Files, 0, 1).unwrap()[0].class,
        DiffClass::Same
    );
}
#[test]
fn dominant_chain_minimum_and_symmetric_shrink() {
    let (_dir, a, b) = pair();
    let new = Snapshot::open(&b).unwrap();
    let e = explain(&new, &a, &p(b"/fixture"), DiffMetric::Disk, 0.5, 1, 5).unwrap();
    assert_eq!(e.steps.len(), 2);
    assert_eq!(e.steps[1].path, p(b"/fixture/a/deep"));
    assert_eq!(e.own_delta, 100);
    assert_eq!(e.spread_children, 0);
    let e = explain(&new, &a, &p(b"/fixture"), DiffMetric::Disk, 0.5, 101, 1).unwrap();
    assert!(e.steps.is_empty());
    assert_eq!(e.spread.len(), 1);
    let e = explain(
        &Snapshot::open(&a).unwrap(),
        &b,
        &p(b"/fixture"),
        DiffMetric::Disk,
        0.5,
        1,
        5,
    )
    .unwrap();
    assert_eq!(e.target_delta, -110);
    assert_eq!(e.steps[1].delta, -100);
    for ratio in [0.0, -1.0, 1.1, f64::NAN, f64::INFINITY] {
        assert!(explain(&new, &a, &p(b"/fixture"), DiffMetric::Disk, ratio, 1, 5).is_err());
    }
}
#[test]
fn spread_tie_bytes_and_own_files() {
    let dir = tempfile::tempdir().unwrap();
    let old = vec![
        row(0, None, b"/fixture", 0),
        row(1, Some(0), b"/fixture/z", 0),
        row(2, Some(0), b"/fixture/a\xff", 0),
    ];
    let mut new = vec![
        row(0, None, b"/fixture", 100),
        row(1, Some(0), b"/fixture/z", 40),
        row(2, Some(0), b"/fixture/a\xff", 40),
    ];
    new[0].own_charged = 20;
    let a = snapshot(dir.path(), &old, 1);
    let b = snapshot(dir.path(), &new, 2);
    let snap = Snapshot::open(&b).unwrap();
    let e = explain(&snap, &a, &p(b"/fixture"), DiffMetric::Disk, 0.5, 1, 5).unwrap();
    assert!(e.steps.is_empty());
    assert_eq!(e.spread_children, 2);
    assert_eq!(e.own_delta, 20);
    let e = explain(&snap, &a, &p(b"/fixture"), DiffMetric::Disk, 0.4, 1, 5).unwrap();
    assert_eq!(e.steps[0].path, p(b"/fixture/a\xff"));
    let rows = diff(&snap, &a, &p(b"/fixture"), DiffMetric::Disk, 1, 3).unwrap();
    assert_eq!(rows[1].path, p(b"/fixture/a\xff"));
}
#[test]
#[ignore = "scale"]
fn million_directory_depth_three() {
    let dir = tempfile::tempdir().unwrap();
    let mut rows = vec![row(0, None, b"/fixture", 0)];
    rows[0].subtree_end = 999_999;
    for id in 1..1_000_000 {
        rows.push(row(
            id,
            Some(0),
            format!("/fixture/{id:08}").as_bytes(),
            id as u64,
        ));
    }
    let a = snapshot(dir.path(), &rows, 1);
    for r in &mut rows {
        r.t_disk += 1;
    }
    let b = snapshot(dir.path(), &rows, 2);
    drop(rows);
    let snap = Snapshot::open(&b).unwrap();
    let start = std::time::Instant::now();
    assert_eq!(
        diff(&snap, &a, &p(b"/fixture"), DiffMetric::Disk, 3, 25)
            .unwrap()
            .len(),
        25
    );
    let elapsed = start.elapsed();
    eprintln!("million directory diff: {elapsed:?}");
    assert!(elapsed.as_secs_f64() < 5.0, "diff took {elapsed:?}");
}

#[test]
fn own_only_zero_change_missing_target_and_read_only_snapshots() {
    let dir = tempfile::tempdir().unwrap();
    let mut old = row(0, None, b"/fixture", 100);
    old.subtree_end = 0;
    let mut new = old.clone();
    new.t_disk = 300;
    new.own_charged = 300;
    let a = snapshot(dir.path(), &[old], 100);
    let b = snapshot(dir.path(), &[new], 200);
    // Reading Tidemark's own state is allowed; these are not scanned files.
    let old_bytes = std::fs::read(&a).unwrap();
    let new_bytes = std::fs::read(&b).unwrap();
    let snap = Snapshot::open(&b).unwrap();
    let e = explain(&snap, &a, &p(b"/fixture"), DiffMetric::Disk, 0.5, 1, 0).unwrap();
    assert!(e.steps.is_empty());
    assert_eq!(e.own_delta, 200);
    assert!(e.spread.is_empty());
    let e = explain(&snap, &b, &p(b"/fixture"), DiffMetric::Disk, 0.5, 0, 5).unwrap();
    assert_eq!(e.target_delta, 0);
    assert!(e.steps.is_empty());
    assert!(diff(&snap, &a, &p(b"/fixture"), DiffMetric::Disk, 0, 1_000_001).is_err());
    assert!(explain(
        &snap,
        &a,
        &p(b"/fixture"),
        DiffMetric::Disk,
        0.5,
        1,
        1_000_001
    )
    .is_err());
    assert_eq!(std::fs::read(&a).unwrap(), old_bytes);
    assert_eq!(std::fs::read(&b).unwrap(), new_bytes);
    let (_dir, a, b) = pair();
    let snap = Snapshot::open(&b).unwrap();
    let e = explain(&snap, &a, &p(b"/fixture/new"), DiffMetric::Disk, 0.5, 1, 5).unwrap();
    assert_eq!(e.target_delta, 30);
    let e = explain(
        &snap,
        &a,
        &p(b"/fixture/removed"),
        DiffMetric::Disk,
        0.5,
        1,
        5,
    )
    .unwrap();
    assert_eq!(e.target_delta, -20);
}
