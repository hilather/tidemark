//! Owned by: W3-C
//! Synthetic exporter structure, accounting and streaming scale coverage.
#![allow(clippy::unwrap_used)]
#[path = "w3c/support.rs"]
mod support;
use tidemark_store::{export_ncdu, NcduOptions, Snapshot};
fn sum(value: &serde_json::Value) -> u64 {
    match value {
        serde_json::Value::Array(a) => a.iter().map(sum).sum(),
        serde_json::Value::Object(o) => o.get("dsize").and_then(|v| v.as_u64()).unwrap_or(0),
        _ => 0,
    }
}
#[test]
fn nested_arrays_subroot_depth_and_charged_totals() {
    let temp = ::test_support::private_tempdir().unwrap();
    let mut root = support::row(0, None, b"/fixture", 10);
    root.subtree_end = 3;
    root.t_charged = 100;
    root.t_apparent = 100;
    let mut a = support::row(1, Some(0), b"/fixture/a", 20);
    a.subtree_end = 2;
    a.t_charged = 50;
    a.t_apparent = 50;
    let mut b = support::row(2, Some(1), b"/fixture/a/b", 30);
    b.depth = 2;
    let c = support::row(3, Some(0), b"/fixture/c", 40);
    let path = support::snapshot(temp.path(), &[root, a, b, c], 100);
    let snap = Snapshot::open(&path).unwrap();
    for depth in [None, Some(0), Some(1)] {
        let mut out = vec![];
        let stats = export_ncdu(
            &snap,
            &snap.meta().root,
            &mut out,
            &NcduOptions {
                utf8_lossy: true,
                max_depth: depth,
            },
        )
        .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(sum(&json[3]), 100);
        assert_eq!(stats.disk_bytes, 100);
        assert_eq!(json[0], 1);
        assert_eq!(json[1], 2);
        assert!(json[2]["note"]
            .as_str()
            .unwrap()
            .contains("charged (additive)"));
        assert_eq!(json[3][0]["name"], "/fixture");
        if depth.is_none() {
            assert_eq!(json[3][2][0]["name"], "a");
            assert_eq!(json[3][2][2][0]["name"], "b");
            assert_eq!(json[3][3][0]["name"], "c");
        }
    }
    let under = tidemark_core::path::RawPath::new(b"/fixture/a".to_vec()).unwrap();
    let mut out = vec![];
    export_ncdu(
        &snap,
        &under,
        &mut out,
        &NcduOptions {
            utf8_lossy: true,
            max_depth: None,
        },
    )
    .unwrap();
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(sum(&json[3]), 50);
    assert_eq!(json[3][0]["name"], "/fixture/a");
}
#[test]
#[ignore = "scale"]
fn million_directory_export_rss_below_64_mib() {
    let temp = ::test_support::private_tempdir().unwrap();
    let mut meta = support::meta(
        tidemark_core::path::RawPath::new(b"/fixture".to_vec()).unwrap(),
        200,
    );
    meta.dirs = 1_000_000;
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    for first in (0..1_000_000u32).step_by(1024) {
        let rows: Vec<_> = (first..(first + 1024).min(1_000_000))
            .map(|id| {
                let mut row = support::row(
                    id,
                    (id != 0).then_some(0),
                    format!("/fixture/{id:07}").as_bytes(),
                    0,
                );
                if id == 0 {
                    row.path = meta.root.clone();
                    row.path_hash = row.path.hash64();
                    row.name = row.path.as_bytes().to_vec();
                    row.subtree_end = 999999;
                }
                row
            })
            .collect();
        writer.put_dirs(&rows).unwrap();
    }
    let file = writer.finish(&meta).unwrap();
    let snap = Snapshot::open(&file).unwrap();
    fn rss() -> u64 {
        let status = std::fs::read_to_string("/proc/self/status").unwrap();
        status
            .lines()
            .find_map(|line| {
                line.strip_prefix("VmRSS:")
                    .map(|s| s.split_whitespace().next().unwrap().parse::<u64>().unwrap() * 1024)
            })
            .unwrap()
    }
    struct MemorySink {
        written: usize,
        sampled: usize,
        peak: u64,
    }
    impl std::io::Write for MemorySink {
        fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
            self.written += bytes.len();
            if self.written - self.sampled >= 1024 * 1024 {
                self.peak = self.peak.max(rss());
                self.sampled = self.written;
            }
            Ok(bytes.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let before = rss();
    let mut sink = MemorySink {
        written: 0,
        sampled: 0,
        peak: before,
    };
    let started = std::time::Instant::now();
    let stats = export_ncdu(
        &snap,
        &meta.root,
        &mut sink,
        &NcduOptions {
            utf8_lossy: true,
            max_depth: None,
        },
    )
    .unwrap();
    let delta = sink.peak.max(rss()).saturating_sub(before);
    eprintln!(
        "directories={} rss_delta={} elapsed={:?}",
        stats.directories,
        delta,
        started.elapsed()
    );
    assert_eq!(stats.directories, 1_000_000);
    assert!(delta < 64 * 1024 * 1024);
}
#[test]
fn notable_files_above_old_cap_stream_and_io_errors_propagate() {
    use tidemark_core::{
        path::{RawName, RawPath},
        snapshot::FileRow,
    };
    let temp = ::test_support::private_tempdir().unwrap();
    let root = RawPath::new(b"/fixture".to_vec()).unwrap();
    let mut meta = support::meta(root.clone(), 300);
    meta.dirs = 1;
    let mut row = support::row(0, None, root.as_bytes(), 4097);
    row.subtree_end = 0;
    row.own_files = 4097;
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&[row]).unwrap();
    let files: Vec<_> = (0..4097)
        .map(|id| FileRow {
            id,
            dir_id: 0,
            name: Some(RawName::new(format!("file-{id}").into_bytes()).unwrap()),
            ino: u64::from(id) + 2,
            disk: 1,
            apparent: 1,
            mtime: 100,
            nlink: 1,
        })
        .collect();
    writer.put_files(&files).unwrap();
    let file = writer.finish(&meta).unwrap();
    let snap = Snapshot::open(&file).unwrap();
    let mut out = vec![];
    let opts = NcduOptions {
        utf8_lossy: true,
        max_depth: None,
    };
    let original = std::fs::read(&file).unwrap();
    let stats = export_ncdu(&snap, &root, &mut out, &opts).unwrap();
    assert_eq!(stats.files, 4097);
    assert_eq!(stats.disk_bytes, 4097);
    let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(sum(&json[3]), 4097);
    let entries = json[3].as_array().unwrap();
    let names: Vec<_> = entries[1..entries.len() - 1]
        .iter()
        .map(|entry| entry["name"].as_str().unwrap())
        .collect();
    assert_eq!(names.len(), 4097);
    for (id, name) in names.iter().enumerate() {
        assert_eq!(*name, format!("file-{id}"));
    }
    let mut again = vec![];
    export_ncdu(&snap, &root, &mut again, &opts).unwrap();
    assert_eq!(out, again);
    assert_eq!(entries.last().unwrap()["name"], "[other files: 0]");
    assert_eq!(std::fs::read(&file).unwrap(), original);
    let absent = RawPath::new(b"/absent".to_vec()).unwrap();
    assert!(export_ncdu(&snap, &absent, &mut out, &opts).is_err());
    let small = ::test_support::private_tempdir().unwrap();
    let mut row = support::row(0, None, b"/fixture", 0);
    row.subtree_end = 0;
    let file = support::snapshot(small.path(), &[row], 400);
    let snap = Snapshot::open(&file).unwrap();
    struct Failing;
    impl std::io::Write for Failing {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("injected writer failure"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    assert!(matches!(
        export_ncdu(&snap, &root, &mut Failing, &opts),
        Err(tidemark_store::StoreError::Io(_))
    ));
}

#[test]
fn symlinks_are_a_subset_of_other_files() {
    let temp = ::test_support::private_tempdir().unwrap();
    let mut row = support::row(0, None, b"/fixture", 30);
    row.subtree_end = 0;
    row.own_files = 3;
    row.own_symlinks = 1;
    let path = support::snapshot(temp.path(), &[row], 500);
    let snap = Snapshot::open(&path).unwrap();
    let mut out = vec![];
    export_ncdu(
        &snap,
        &snap.meta().root,
        &mut out,
        &NcduOptions {
            utf8_lossy: true,
            max_depth: None,
        },
    )
    .unwrap();
    let doc: serde_json::Value = serde_json::from_slice(&out).unwrap();
    assert_eq!(doc[3][1]["name"], "[other files: 3]");
}
#[test]
fn outside_subroot_link_keeps_apparent_charge_at_every_depth() {
    let temp = ::test_support::private_tempdir().unwrap();
    // A 500-byte shared inode has names in a/d/e and b and is charged at root.
    let mut root = support::row(0, None, b"/fixture", 500);
    root.subtree_end = 4;
    root.t_charged = 564;
    let mut a = support::row(1, Some(0), b"/fixture/a", 10);
    a.subtree_end = 3;
    a.t_charged = 32;
    a.t_apparent = 532;
    let mut d = support::row(2, Some(1), b"/fixture/a/d", 10);
    d.depth = 2;
    d.subtree_end = 3;
    d.t_charged = 22;
    d.t_apparent = 522;
    let mut e = support::row(3, Some(2), b"/fixture/a/d/e", 12);
    e.depth = 3;
    e.t_apparent = 512;
    let b = support::row(4, Some(0), b"/fixture/b", 32);
    let path = support::snapshot(temp.path(), &[root, a, d, e, b], 600);
    let snap = Snapshot::open(&path).unwrap();
    fn apparent(v: &serde_json::Value) -> u64 {
        if let Some(a) = v.as_array() {
            a.iter().map(apparent).sum()
        } else {
            v["asize"].as_u64().unwrap_or(0)
        }
    }
    let under = tidemark_core::path::RawPath::new(b"/fixture/a".to_vec()).unwrap();
    for depth in [None, Some(0), Some(1), Some(2), Some(3)] {
        let mut out = vec![];
        export_ncdu(
            &snap,
            &under,
            &mut out,
            &NcduOptions {
                utf8_lossy: true,
                max_depth: depth,
            },
        )
        .unwrap();
        let doc: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(apparent(&doc[3]), 32, "depth {depth:?}");
        assert_eq!(sum(&doc[3]), 32);
    }
}
#[test]
fn notable_paging_uses_index_seek_without_sort() {
    let source = include_str!("../src/export_ncdu.rs");
    let sql = source
        .split("let mut files = snap.conn().prepare(")
        .nth(1)
        .unwrap()
        .split('"')
        .nth(1)
        .unwrap();
    let temp = ::test_support::private_tempdir().unwrap();
    let mut row = support::row(0, None, b"/fixture", 0);
    row.subtree_end = 0;
    let path = support::snapshot(temp.path(), &[row], 700);
    let query = format!("EXPLAIN QUERY PLAN {sql}");
    let conn =
        rusqlite::Connection::open_with_flags(&path, rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY)
            .unwrap();
    let mut statement = conn.prepare(&query).unwrap();
    // Both the first and subsequent pages must seek, rather than sort/filter a directory repeatedly.
    for after in [-1i64, 0, 4096] {
        let mut rows = statement.query([0i64, after, 256]).unwrap();
        let mut plan = String::new();
        while let Some(row) = rows.next().unwrap() {
            plan.push_str(&row.get::<_, String>(3).unwrap());
        }
        assert!(
            plan.contains("SEARCH files USING INDEX files_dir") && plan.contains("rowid>"),
            "{plan}"
        );
        assert!(!plan.contains("TEMP B-TREE"), "{plan}");
    }
}

#[test]
fn truncated_apparent_sum_reports_overflow() {
    let temp = ::test_support::private_tempdir().unwrap();
    let mut root = support::row(0, None, b"/fixture", 0);
    root.subtree_end = 3;
    let mut rows = vec![root];
    for id in 1..=3 {
        let mut row = support::row(id, Some(0), format!("/fixture/{id}").as_bytes(), 0);
        row.own_apparent = i64::MAX as u64;
        rows.push(row);
    }
    let path = support::snapshot(temp.path(), &rows, 800);
    let snap = Snapshot::open(&path).unwrap();
    let error = export_ncdu(
        &snap,
        &snap.meta().root,
        &mut vec![],
        &NcduOptions {
            utf8_lossy: true,
            max_depth: Some(0),
        },
    )
    .unwrap_err();
    assert!(matches!(error, tidemark_store::StoreError::Integer(_)));
}
