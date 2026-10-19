//! Owned by: W7-A
//! History worker, identity, cancellation and cache contracts, with injected clocks.
#![allow(clippy::unwrap_used, clippy::expect_used)]

use super::*;
use crate::tui::tests::fixture;

fn setup() -> (tempfile::TempDir, Key) {
    let temp = test_support::private_tempdir().unwrap();
    let rows = [
        fixture::row(0, None, b"/fixture", 100),
        fixture::row(1, Some(0), b"/fixture/a", 25),
    ];
    let file = fixture::snapshot(temp.path(), &rows, 100);
    let meta = fixture::meta(rows[0].path.clone(), 100);
    let key = Key {
        root: rows[0].path.clone(),
        path: rows[1].path.clone(),
        window: window(&[], &meta, &file),
        fs_uuid: String::new(),
    };
    (temp, key)
}

fn opens(h: &History) -> u64 {
    h.worker
        .as_ref()
        .unwrap()
        .shared
        .opens
        .load(Ordering::Relaxed)
}
fn lookups(h: &History) -> u64 {
    h.worker
        .as_ref()
        .unwrap()
        .shared
        .lookups
        .load(Ordering::Relaxed)
}

#[test]
fn unchanged_turns_do_not_post_generations_and_warm_paths_do_no_io() {
    let (_temp, key) = setup();
    let mut h = History::default();
    h.update(key.clone(), None, true, false, 10);
    let generation = h.generation;
    for _ in 0..100 {
        h.update(key.clone(), None, true, false, 10);
    }
    h.settle(10);
    assert_eq!(h.generation, generation);
    assert_eq!(h.samples, vec![Some(Sample::Known(25))]);
    assert_eq!((opens(&h), lookups(&h)), (1, 1));
    let mut other = key.clone();
    other.path = key.root.clone();
    h.update(other, None, true, false, 11);
    h.settle(11);
    assert_eq!((opens(&h), lookups(&h)), (1, 2));
    h.update(key, None, true, false, 12);
    h.settle(12);
    assert_eq!((opens(&h), lookups(&h)), (1, 2));
}

#[test]
fn identity_and_uuid_failures_are_unavailable_not_zero() {
    let (_temp, key) = setup();
    for kind in 0..4 {
        let mut bad = key.clone();
        match kind {
            0 => bad.window[0].id = "20260101T000000Z-99999999".into(),
            1 => bad.window[0].started += 1,
            2 => bad.root = RawPath::new(b"/other".to_vec()).unwrap(),
            _ => bad.window[0].file = bad.window[0].file.with_extension("pruned"),
        }
        let mut h = History::default();
        h.update(bad, None, true, false, 1);
        h.settle(1);
        assert!(matches!(h.samples[0], Some(Sample::Unavailable(_))));
    }
    let mut meta = fixture::meta(key.root.clone(), 100);
    meta.fs_uuid = "saved-device".into();
    let mut request = key.clone();
    request.fs_uuid = "different-device".into();
    assert_eq!(
        invalid(&meta, &key.window[0], &request),
        Some("filesystem mismatch")
    );
    request.fs_uuid.clear();
    assert_eq!(invalid(&meta, &key.window[0], &request), None);
    meta.status = SnapshotStatus::Partial;
    assert_eq!(
        invalid(&meta, &key.window[0], &request),
        Some("incomplete snapshot")
    );
}

#[test]
fn missing_paths_are_negative_cached_and_failures_retry_after_five_seconds() {
    let (_temp, key) = setup();
    let mut h = History::default();
    let mut absent = key.clone();
    absent.path = RawPath::new(b"/fixture/renamed".to_vec()).unwrap();
    h.update(absent.clone(), None, true, false, 100);
    h.settle(100);
    assert_eq!(h.samples, vec![Some(Sample::MissingPath)]);
    h.update(key.clone(), None, true, false, 101);
    h.settle(101);
    h.update(absent, None, true, false, 102);
    h.settle(102);
    assert_eq!(lookups(&h), 2);
    let mut pruned = key;
    pruned.window[0].file.set_extension("missing");
    h.update(pruned.clone(), None, true, false, 200);
    h.settle(203);
    let generation = h.generation;
    let attempts = opens(&h);
    h.update(pruned.clone(), None, true, false, 207);
    assert_eq!(h.generation, generation);
    h.update(pruned, None, true, false, 208);
    h.settle(208);
    assert_eq!(opens(&h), attempts + 1);
}

#[test]
fn window_retains_same_time_predecessors_and_excludes_open_siblings_and_future() {
    let (_temp, key) = setup();
    let meta = fixture::meta(key.root.clone(), 100);
    let rows: Vec<_> = (0..12)
        .map(|i| {
            (
                format!("id-{i:02}"),
                PathBuf::from(format!("/{i}")),
                if i < 9 { i / 2 } else { 100 },
                None,
            )
        })
        .collect();
    let result = window(&rows, &meta, &key.window[0].file);
    assert_eq!(result.len(), 7);
    assert_eq!(result.last().unwrap().id, meta.scan_id.as_str());
    assert!(result[..6].iter().all(|e| e.started < 100));
    assert_eq!(
        result[..6]
            .iter()
            .map(|e| e.id.as_str())
            .collect::<Vec<_>>(),
        ["id-02", "id-05", "id-04", "id-07", "id-06", "id-08"]
    );
}

#[test]
fn cache_has_path_and_byte_bounds_and_oversized_paths_still_query() {
    let (_temp, key) = setup();
    let mut h = History::default();
    for i in 0..140 {
        let mut request = key.clone();
        request.path = RawPath::new(format!("/fixture/{i}").into_bytes()).unwrap();
        h.update(request, None, true, false, 1);
        h.settle(1);
    }
    assert_eq!(h.cache.len(), CACHE_PATHS);
    assert!(h.bytes <= CACHE_BYTES);
    for i in 0..30 {
        let mut request = key.clone();
        request.path =
            RawPath::new(format!("/fixture/{i}{}", "x".repeat(30_000)).into_bytes()).unwrap();
        h.update(request, None, true, false, 1);
        h.settle(1);
    }
    assert!(h.cache.len() < CACHE_PATHS);
    assert!(h.bytes <= CACHE_BYTES);
    let mut request = key;
    request.path =
        RawPath::new(format!("/fixture/{}", "x".repeat(CACHE_BYTES)).into_bytes()).unwrap();
    h.update(request.clone(), None, true, false, 1);
    h.settle(1);
    assert_eq!(h.samples, vec![Some(Sample::MissingPath)]);
    assert!(!h.cache.iter().any(|c| c.key == request));
}

fn pause(h: &mut History) -> Arc<Shared> {
    let worker = h
        .worker
        .get_or_insert_with(|| Worker::new(tidemark_core::config::RetentionConfig::default()));
    worker.shared.lock().paused = true;
    worker.shared.clone()
}

fn entered(shared: &Shared) {
    let (mailbox, timeout) = shared
        .wake
        .wait_timeout_while(shared.lock(), std::time::Duration::from_secs(10), |m| {
            m.entered == 0
        })
        .unwrap();
    assert!(!timeout.timed_out());
    assert!(mailbox.entered > 0);
}

#[test]
fn delayed_reader_coalesces_requests_and_never_publishes_or_applies_stale_results() {
    let (_temp, key) = setup();
    let mut h = History::default();
    let shared = pause(&mut h);
    h.update(key.clone(), None, true, false, 1);
    entered(&shared);
    for i in 0..100 {
        let mut next = key.clone();
        next.path = RawPath::new(format!("/fixture/{i}").into_bytes()).unwrap();
        h.update(next, None, true, false, 1);
    }
    assert_eq!(opens(&h), 1);
    assert_eq!(
        shared.lock().pending.as_ref().unwrap().key.path.as_bytes(),
        b"/fixture/99"
    );
    shared.lock().paused = false;
    shared.wake.notify_all();
    h.settle(1);
    assert_eq!(shared.discarded.load(Ordering::Relaxed), 1);
    assert_eq!(lookups(&h), 1);
    assert_eq!(h.samples, vec![Some(Sample::MissingPath)]);
    shared.lock().result = Some(ResultRow {
        generation: h.generation - 1,
        samples: vec![Sample::Known(999)],
    });
    h.poll(1);
    assert_eq!(h.samples, vec![Some(Sample::MissingPath)]);
}

#[test]
fn hidden_chart_defers_io_and_drop_releases_a_blocked_open() {
    let (_temp, key) = setup();
    let mut h = History::default();
    h.update(key.clone(), None, false, false, 1);
    assert!(h.worker.is_none());
    let shared = pause(&mut h);
    h.update(key.clone(), None, true, false, 1);
    entered(&shared);
    h.update(key, None, false, false, 1);
    assert!(shared.lock().pending.is_none());
    drop(h); // cancel is the other condition in the open seam; no manual release needed.
    assert!(shared.cancel.load(Ordering::Acquire));
}

#[test]
fn worker_rejects_known_uuid_mismatch_and_corrupt_files() {
    let temp = test_support::private_tempdir().unwrap();
    let row = fixture::row(0, None, b"/fixture", 100);
    let mut meta = fixture::meta(row.path.clone(), 100);
    meta.fs_uuid = "saved-device".into();
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(std::slice::from_ref(&row)).unwrap();
    let file = writer.finish(&meta).unwrap();
    let mut key = Key {
        root: row.path.clone(),
        path: row.path,
        window: window(&[], &meta, &file),
        fs_uuid: "other-device".into(),
    };
    let mut h = History::default();
    h.update(key.clone(), None, true, false, 1);
    h.settle(1);
    assert_eq!(
        h.samples,
        vec![Some(Sample::Unavailable("filesystem mismatch"))]
    );
    assert_eq!(lookups(&h), 0);
    key.fs_uuid.clear();
    h.update(key.clone(), None, true, false, 2);
    h.settle(2);
    assert_eq!(h.samples, vec![Some(Sample::Known(100))]);
    drop(h);
    std::fs::write(&file, b"corrupt database").unwrap();
    let mut h = History::default();
    h.update(key, None, true, false, 3);
    h.settle(3);
    assert_eq!(
        h.samples,
        vec![Some(Sample::Unavailable("cannot open snapshot"))]
    );
}

#[test]
fn deep_and_non_utf8_paths_are_exact_not_display_or_inode_keys() {
    let temp = test_support::private_tempdir().unwrap();
    let mut rows = vec![fixture::row(0, None, b"/fixture", 100)];
    let mut path = b"/fixture".to_vec();
    for id in 1..=20 {
        path.extend_from_slice(b"/deep");
        let mut row = fixture::row(id, Some(id - 1), &path, 100 - u64::from(id));
        row.depth = id;
        row.subtree_end = 21;
        rows.push(row);
    }
    path.extend_from_slice(b"/\xff\n\x1b");
    let mut leaf = fixture::row(21, Some(20), &path, 13);
    leaf.depth = 21;
    rows.push(leaf);
    rows[0].subtree_end = 21;
    let file = fixture::snapshot(temp.path(), &rows, 100);
    let meta = fixture::meta(rows[0].path.clone(), 100);
    let key = Key {
        root: rows[0].path.clone(),
        path: rows[21].path.clone(),
        window: window(&[], &meta, &file),
        fs_uuid: String::new(),
    };
    let mut h = History::default();
    h.update(key.clone(), None, true, false, 1);
    h.settle(1);
    assert_eq!(h.samples, vec![Some(Sample::Known(13))]);
    let mut renamed = key;
    renamed.path = RawPath::new([path.as_slice(), b"renamed"].concat()).unwrap();
    h.update(renamed, None, true, false, 1);
    h.settle(1);
    assert_eq!(h.samples, vec![Some(Sample::MissingPath)]);
}

#[test]
fn seven_reader_bound_eviction_and_cold_warm_measurements() {
    let temp = test_support::private_tempdir().unwrap();
    let rows = [
        fixture::row(0, None, b"/fixture", 100),
        fixture::row(1, Some(0), b"/fixture/a", 25),
    ];
    let mut entries = Vec::new();
    for i in 0..8 {
        let mut meta = fixture::meta(rows[0].path.clone(), i);
        meta.scan_id = serde_json::from_str(&format!("\"20260101T000000Z-{i:08}\"")).unwrap();
        let mut writer =
            tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
        writer.put_dirs(&rows).unwrap();
        entries.push(Entry {
            id: meta.scan_id.to_string(),
            file: writer.finish(&meta).unwrap(),
            started: i,
        });
    }
    let key = Key {
        root: rows[0].path.clone(),
        path: rows[1].path.clone(),
        window: entries[..7].to_vec(),
        fs_uuid: String::new(),
    };
    let mut h = History::default();
    let start = std::time::Instant::now();
    h.update(key.clone(), Some(25), true, false, 1);
    h.settle(1);
    let cold = start.elapsed();
    assert_eq!((opens(&h), lookups(&h)), (6, 6));
    let mut other = key.clone();
    other.path = key.root.clone();
    let start = std::time::Instant::now();
    h.update(other, Some(100), true, false, 2);
    h.settle(2);
    let readers_warm = start.elapsed();
    assert_eq!((opens(&h), lookups(&h)), (6, 12));
    let start = std::time::Instant::now();
    h.update(key.clone(), Some(25), true, false, 3);
    h.settle(3);
    let cached = start.elapsed();
    assert_eq!((opens(&h), lookups(&h)), (6, 12));
    println!("history fixture: cold {cold:?} (6 opens/6 lookups), warm readers {readers_warm:?} (0 opens/6 lookups), cached path {cached:?} (0/0)");
    let mut uncached = key.clone();
    uncached.path = RawPath::new(b"/fixture/missing".to_vec()).unwrap();
    h.update(uncached, None, true, false, 4);
    h.settle(4);
    assert_eq!(
        h.worker
            .as_ref()
            .unwrap()
            .shared
            .readers
            .load(Ordering::Relaxed),
        7
    );
    let mut switched = key.clone();
    switched.window = vec![entries[7].clone()];
    h.update(switched, None, true, false, 5);
    h.settle(5);
    assert_eq!(
        h.worker
            .as_ref()
            .unwrap()
            .shared
            .readers
            .load(Ordering::Relaxed),
        1
    );
    h.update(key, Some(25), true, false, 6);
    h.settle(6);
    assert_eq!(
        h.worker
            .as_ref()
            .unwrap()
            .shared
            .readers
            .load(Ordering::Relaxed),
        0,
        "cached window still evicts unrelated readers"
    );
}

#[test]
fn blocked_history_reader_does_not_block_navigation_or_relabel_old_bars() {
    use crossterm::event::KeyCode;
    let (temp, mut b) = crate::tui::tests::browser(3);
    let mut meta = fixture::meta(b.dir.path.clone(), 100);
    meta.scan_id = serde_json::from_str("\"20260101T000000Z-11111111\"").unwrap();
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&[b.dir.clone()]).unwrap();
    b.snapshots = vec![(
        meta.scan_id.to_string(),
        writer.finish(&meta).unwrap(),
        100,
        Some(999),
    )];
    let shared = pause(&mut b.history);
    b.prepare(140, 40).unwrap();
    entered(&shared);
    let generation = b.history.generation;
    let start = std::time::Instant::now();
    b.on_key(KeyCode::Down, &Default::default()).unwrap();
    b.prepare(140, 40).unwrap();
    let elapsed = start.elapsed();
    assert!(b.history.generation > generation);
    assert!(b.subject().0.path.as_bytes().ends_with(b"00000002"));
    assert_eq!(b.history.key.as_ref().unwrap().path, b.subject().0.path);
    assert_eq!(b.history.samples, [None, Some(Sample::Known(4096))]);
    // The I/O barrier is still held: input and title replacement completed independently.
    assert!(shared.lock().paused);
    println!("navigation with history open held at barrier: {elapsed:?}");
    b.history.cancel();
    drop(b);
}
