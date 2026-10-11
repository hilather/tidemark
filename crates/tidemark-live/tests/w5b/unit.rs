//! Owned by: W5-B
//! Synthetic proc tables exercise aggregation without host dependencies.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use super::*;
struct Fake {
    passes: VecDeque<io::Result<Vec<OpenFile>>>,
    start: u64,
}
impl Source for Fake {
    fn scan(&mut self, _: &[DevId], flags: bool) -> io::Result<Vec<OpenFile>> {
        assert!(flags);
        self.start += 1;
        self.passes.pop_front().unwrap().map(|mut files| {
            for file in &mut files {
                if file.starttime == 0 {
                    file.starttime = self.start;
                }
            }
            files
        })
    }
    fn attribution(&self, pid: u32) -> Attribution {
        Attribution {
            pid,
            starttime: self.start,
            comm: format!("holder-{}", self.start),
            unit: Some("test.service".into()),
            uid: Some(0),
        }
    }
}
fn file(ino: u64, pid: u32, disk: u64) -> OpenFile {
    OpenFile {
        pid,
        starttime: 0,
        fd: 3,
        target: b"/data/log (deleted)".to_vec(),
        dev: 1,
        ino,
        nlink: 0,
        size: disk,
        disk,
        flags: Some(2),
    }
}
fn scanner(passes: Vec<io::Result<Vec<OpenFile>>>) -> ProcScanner {
    let mut s = ProcScanner::new(
        ProcScannerOptions {
            mounts: vec![(RawPath::new(b"/data".to_vec()).unwrap(), 1)],
            open_writers: true,
            exclude_prefixes: vec![RawPath::new(b"/data/excluded".to_vec()).unwrap()],
        },
        Arc::new(AttributionCache::new(Duration::ZERO, 0)),
    );
    s.source = Box::new(Fake {
        passes: passes.into(),
        start: 0,
    });
    s
}
#[test]
fn dedup_holders_suffix_and_raw_paths() {
    let mut second = file(1, 2, 512);
    second.fd = 4;
    let mut middle = file(2, 1, 1024);
    middle.target = b"/data/(deleted)/\xff".to_vec();
    let mut foreign = file(3, 3, 2048);
    foreign.dev = 2;
    let mut s = scanner(vec![Ok(vec![
        file(1, 1, 512),
        file(1, 2, 512),
        second,
        middle,
        foreign,
    ])]);
    assert_eq!(s.scan_once(0).unwrap().fds_seen, 5);
    let rows = s.phantom(None);
    assert_eq!(rows[0].total_bytes, 1536);
    assert_eq!(rows[0].files, 2);
    assert_eq!(rows[0].holders.len(), 3);
    assert_eq!(rows[0].holders[1].path, b"/data/log");
    assert_eq!(rows[0].holders[0].path, b"/data/(deleted)/\xff");
    let json = serde_json::to_string(&rows).unwrap();
    assert!(json.contains("path_b64"));
    assert!(!json.contains("[47,"));
    assert!(s
        .phantom(Some(&RawPath::new(b"/other".to_vec()).unwrap()))
        .is_empty());
    assert_eq!(s.take_samples().0.len(), 3);
    assert!(s.take_samples().0.is_empty());
}
#[test]
fn windows_reuse_exit_and_readonly_exclusions() {
    let mut excluded = file(2, 1, 512);
    excluded.target = b"/data/excluded/file".to_vec();
    let mut readonly = file(3, 1, 512);
    readonly.flags = Some(0);
    let mut sibling = file(4, 1, 512);
    sibling.target = b"/data/excluded-other".to_vec();
    let mut s = scanner(vec![
        Ok(vec![file(1, 1, 512), excluded, readonly, sibling]),
        Ok(vec![file(1, 1, 1536)]),
        Ok(vec![]),
        Ok(vec![file(1, 1, 2048)]),
        Ok(vec![]),
        Ok(vec![]),
    ]);
    s.scan_once(0).unwrap();
    assert_eq!(s.take_samples().1.len(), 2);
    s.scan_once(10).unwrap();
    let w = s.writers(None, Duration::from_secs(10), 10);
    assert_eq!(w[0].growth, 1024);
    assert_eq!(w[0].comm, "holder-2");
    s.scan_once(20).unwrap();
    assert!(s.writers(None, Duration::from_secs(20), 10).is_empty());
    s.scan_once(30).unwrap();
    assert_eq!(s.writers(None, Duration::from_secs(30), 10)[0].growth, 1536);
    s.scan_once(40).unwrap();
    s.scan_once(50).unwrap();
    assert!(s.tracked.is_empty());
}
#[test]
fn failure_preserves_state_and_clock_order() {
    let mut s = scanner(vec![
        Ok(vec![file(1, 1, 512)]),
        Err(io::Error::from_raw_os_error(5)),
        Ok(vec![file(1, 1, 1024)]),
    ]);
    s.scan_once(0).unwrap();
    assert!(s.scan_once(10).is_err());
    assert_eq!(s.phantom(None)[0].total_bytes, 512);
    assert!(s.scan_once(-1).is_err());
    s.scan_once(10).unwrap();
    assert_eq!(s.writers(None, Duration::from_secs(10), 1)[0].growth, 512);
    assert!(s.writers(None, Duration::ZERO, 1).is_empty());
    assert_eq!(delta(u64::MAX, 0), i64::MAX);
    assert_eq!(delta(0, u64::MAX), i64::MIN);
}
#[test]
fn ring_cap_eviction_and_twenty_thousand_fds() {
    let first: Vec<_> = (0..20_000).map(|i| file(i, 1, 512)).collect();
    let second: Vec<_> = (0..20_000)
        .map(|i| file(i, 1, if i == 9999 { 4096 } else { 512 }))
        .collect();
    let mut passes = vec![Ok(first), Ok(second)];
    passes.extend((2..65).map(|_| Ok(vec![file(9999, 1, 8192)])));
    let mut s = scanner(passes);
    let result = s.scan_once(0).unwrap();
    eprintln!("synthetic 20,000 fds: {:?}", result.duration);
    assert_eq!(result.fds_seen, 20_000);
    assert_eq!(s.tracked.len(), MAX_WRITERS);
    s.scan_once(1).unwrap();
    assert!(s.tracked.contains_key(&(1, 9999)));
    assert!(s.tracked.contains_key(&(1, 10_000)));
    for i in 2..65 {
        s.scan_once(i).unwrap();
    }
    assert_eq!(s.tracked[&(1, 9999)].samples.len(), HISTORY);
    assert!(s.writers(None, Duration::from_secs(60), 1).is_empty());
    assert_eq!(s.tracked[&(1, 9999)].samples.front().unwrap().0, 5);
}

#[test]
fn irregular_window_baseline_sort_limits_and_subsecond() {
    let mut s = scanner(vec![
        Ok(vec![file(1, 1, 512), file(2, 2, 512)]),
        Ok(vec![file(1, 1, 1024), file(2, 2, 1536)]),
        Ok(vec![file(1, 1, 2048), file(2, 2, 4096)]),
        Ok(vec![file(1, 1, 4096)]),
    ]);
    s.scan_once(0).unwrap();
    s.scan_once(10).unwrap();
    s.scan_once(30).unwrap();
    let rows = s.writers(None, Duration::from_secs(15), 1);
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0].ino, 2);
    assert_eq!(rows[0].growth, 2560);
    assert_eq!(rows[0].window_secs, 20);
    assert!(s
        .writers(
            Some(&RawPath::new(b"/other".to_vec()).unwrap()),
            Duration::from_secs(30),
            10
        )
        .is_empty());
    assert!(s.writers(None, Duration::from_secs(30), 0).is_empty());
    s.scan_once(30).unwrap();
    let rows = s.writers(None, Duration::from_millis(1), 1);
    assert_eq!(rows[0].growth, 3072);
    assert_eq!(rows[0].window_secs, 20);
}

#[test]
fn admission_reaches_initially_excluded_grower() {
    let passes = (0..8)
        .map(|pass| {
            Ok((0..20_000)
                .map(|i| file(i, 1, if i == 19_999 { 512 + pass * 512 } else { 512 }))
                .collect())
        })
        .collect();
    let mut s = scanner(passes);
    let mut found = false;
    let mut evictions = 0;
    for at in 0..8 {
        let result = s.scan_once(at).unwrap();
        assert!(result.writers_full);
        evictions += result.writer_evictions;
        assert!(s.tracked.len() <= MAX_WRITERS);
        assert!(s.tracked.values().all(|t| t.samples.len() <= HISTORY));
        found |= s
            .writers(None, Duration::from_secs(300), 20)
            .iter()
            .any(|r| r.ino == 19_999);
    }
    assert!(found);
    assert!(evictions > 0);
}
#[test]
fn short_history_and_repeated_seconds() {
    let mut s = scanner(vec![
        Ok(vec![file(1, 1, 512)]),
        Ok(vec![file(1, 1, 1024)]),
        Ok(vec![file(1, 1, 2048)]),
    ]);
    s.scan_once(0).unwrap();
    s.scan_once(0).unwrap();
    assert!(s.writers(None, Duration::from_millis(500), 1).is_empty());
    s.scan_once(10).unwrap();
    let row = &s.writers(None, Duration::from_secs(3600), 1)[0];
    assert_eq!(row.growth, 1536);
    assert_eq!(row.window_secs, 10);
}
#[test]
fn enumeration_identity_mismatch_is_unknown() {
    let mut old = file(1, 1, 512);
    old.starttime = 99;
    let mut s = scanner(vec![Ok(vec![old])]);
    s.scan_once(0).unwrap();
    let row = &s.phantom(None)[0].holders[0];
    assert_eq!(row.comm, "?");
    assert_eq!(row.unit, None);
    assert_eq!(s.take_samples().1[0].comm, "?");
}
