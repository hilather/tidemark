//! Owned by: W7-A
//! In-module browser tests exercising the real store and drawing path.
#![allow(clippy::unwrap_used, clippy::expect_used)]

#[path = "fixture.rs"]
pub(super) mod fixture;
#[path = "tide_chart.rs"]
mod tide_chart;
use super::*;
use ratatui::{backend::TestBackend, Terminal};
use tidemark_core::{
    path::{RawName, RawPath},
    snapshot::{FileRow, LinkInstanceRow, LinkSetRow},
};

pub(super) fn browser(count: u32) -> (tempfile::TempDir, Browser) {
    let temp = ::test_support::private_tempdir().unwrap();
    let mut rows = vec![fixture::row(0, None, b"/fixture", u64::from(count) * 4096)];
    rows[0].subtree_end = count;
    rows[0].own_charged = 0;
    for id in 1..=count {
        rows.push(fixture::row(
            id,
            Some(0),
            format!("/fixture/{id:08}").as_bytes(),
            4096,
        ));
    }
    let file = fixture::snapshot(temp.path(), &rows, 1_000_000);
    let snapshot = Snapshot::open(&file).unwrap();
    let dir = snapshot.root().unwrap();
    let mut b = Browser {
        snapshot,
        snapshot_file: file.clone(),
        old: None,
        prev_open_failed_at: None,
        dir,
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        clock: 1_000_100,
        history: history::History::default(),
        history_refresh: false,
        data_revision: 0,
        info_subject: None,
        info_error: None,
        pages: Pages::default(),
        refresh: None,
        theme: theme::Theme::plain(),
        refresh_elapsed_ms: 0,
        main_selected: 0,
    };
    b.load().unwrap();
    (temp, b)
}
/// Children whose charged size grows with the id, so name order and size order disagree.
fn browser_by_size(count: u32) -> (tempfile::TempDir, Browser) {
    let temp = ::test_support::private_tempdir().unwrap();
    let mut rows = vec![fixture::row(0, None, b"/fixture", u64::from(count) * 4096)];
    rows[0].subtree_end = count;
    rows[0].own_charged = 0;
    for id in 1..=count {
        rows.push(fixture::row(
            id,
            Some(0),
            format!("/fixture/{id:08}").as_bytes(),
            u64::from(id) * 4_096,
        ));
    }
    let file = fixture::snapshot(temp.path(), &rows, 1_000_000);
    let snapshot = Snapshot::open(&file).unwrap();
    let dir = snapshot.root().unwrap();
    let mut b = Browser {
        snapshot,
        snapshot_file: file.clone(),
        old: None,
        prev_open_failed_at: None,
        dir,
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        clock: 1_000_100,
        history: history::History::default(),
        history_refresh: false,
        data_revision: 0,
        info_subject: None,
        info_error: None,
        pages: Pages::default(),
        refresh: None,
        theme: theme::Theme::plain(),
        refresh_elapsed_ms: 0,
        main_selected: 0,
    };
    b.load().unwrap();
    (temp, b)
}
fn render(b: &Browser, width: u16, height: u16) -> String {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| draw::draw(frame, b)).unwrap();
    let buffer = terminal.backend().buffer();
    (0..height)
        .map(|y| {
            (0..width)
                .map(|x| buffer[(x, y)].symbol())
                .collect::<String>()
        })
        .collect::<Vec<_>>()
        .join("\n")
}
#[test]
fn pages_all_2500_children() {
    let (_temp, mut b) = browser(2500);
    let mut ids = Vec::new();
    loop {
        assert!(b.rows.len() <= PAGE);
        ids.extend(b.rows.iter().map(|r| r.0.id));
        if b.rows.len() < PAGE {
            break;
        }
        b.page(false).unwrap();
    }
    ids.sort_unstable();
    assert_eq!(ids, (1..=2500).collect::<Vec<_>>());
}
#[test]
fn descend_ascend_filter_sort_and_bounds() {
    let (_temp, mut b) = browser(10);
    b.state.filter = "00000005".into();
    b.boundary(false).unwrap();
    assert_eq!(b.visible().len(), 1);
    b.enter().unwrap();
    assert_eq!(b.dir.id, 5);
    assert_eq!(b.rows.len(), 0);
    let parent = b.snapshot.dir(b.dir.parent_id.unwrap()).unwrap();
    b.navigate(parent).unwrap();
    b.state.filter.clear();
    b.boundary(false).unwrap();
    b.state.sort = state::Sort::Name;
    b.state.order(&mut b.rows);
    assert_eq!(b.rows[0].0.id, 1);
    b.state.move_by(999, b.count());
    assert_eq!(b.state.selected, 10);
}
#[test]
fn deterministic_render_at_both_sizes() {
    let (_temp, b) = browser(3);
    for (width, height) in [(80, 24), (160, 48)] {
        let first = render(&b, width, height);
        assert_eq!(first, render(&b, width, height));
        assert!(first.contains(if width >= 160 { "./ (files here)" } else { "." }));
        if width < 160 {
            assert!(!first.contains("./ (files here)"), "{first}");
        }
        assert!(first.contains("steady"), "{first}");
        assert!(!first.contains("degraded=false"), "{first}");
        insta::with_settings!({snapshot_path => "snapshots"}, { insta::assert_snapshot!(format!("main_{width}_{height}"),first); });
    }
}
#[test]
fn too_small() {
    let (_temp, b) = browser(1);
    let text = render(&b, 79, 23);
    assert!(text.contains("terminal too small"), "{text}");
    assert!(text.contains("need 80×24, have 79×23"), "{text}");
    assert!(text.contains("minimum 80x24"), "{text}");
    let mut ascii = b;
    ascii.theme = theme::Theme::select(true, false, None, Some("linux"), Some("C"));
    let text = render(&ascii, 79, 23);
    assert!(text.contains("need 80x24, have 79x23"), "{text}");
    assert!(!text.contains('×'), "{text}");
}
#[test]
fn hostile_names_in_data_panes() {
    let temp = ::test_support::private_tempdir().unwrap();
    let name = RawName::new([b"\x1b\n\xff".as_slice(), &[b'x'; 300]].concat()).unwrap();
    let root = RawPath::new(b"/fixture".to_vec()).unwrap().join(&name);
    let path = root.join(&name);
    let mut rows = vec![
        fixture::row(0, None, root.as_bytes(), 8192),
        fixture::row(1, Some(0), path.as_bytes(), 4096),
    ];
    rows[0].subtree_end = 1;
    let meta = fixture::meta(rows[0].path.clone(), 1_000_000);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&rows).unwrap();
    writer
        .put_files(&[FileRow {
            id: 0,
            dir_id: 0,
            name: Some(name.clone()),
            ino: 100,
            disk: 4096,
            apparent: 4096,
            mtime: 100,
            nlink: 2,
        }])
        .unwrap();
    writer
        .put_link_sets(&[LinkSetRow {
            ino: 100,
            nlink: 2,
            found: 2,
            complete: true,
            disk: 4096,
            apparent: 4096,
            mtime: 100,
            lca_dir_id: 0,
        }])
        .unwrap();
    writer
        .put_link_instances(&[LinkInstanceRow {
            ino: 100,
            dir_id: 0,
            name: Some(name),
        }])
        .unwrap();
    let file = writer.finish(&meta).unwrap();
    let snapshot = Snapshot::open(&file).unwrap();
    let mut b = Browser {
        dir: snapshot.root().unwrap(),
        snapshot,
        snapshot_file: file.clone(),
        old: None,
        prev_open_failed_at: None,
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        clock: 1_000_100,
        history: history::History::default(),
        history_refresh: false,
        data_revision: 0,
        info_subject: None,
        info_error: None,
        pages: Pages::default(),
        refresh: None,
        theme: theme::Theme::plain(),
        refresh_elapsed_ms: 0,
        main_selected: 0,
    };
    b.load().unwrap();
    for pane in [
        Pane::Main,
        Pane::Info,
        Pane::Files,
        Pane::Links,
        Pane::Help,
        Pane::Picker,
    ] {
        if pane == Pane::Info {
            b.navigate(b.snapshot.dir(1).unwrap()).unwrap();
        } else if b.dir.id != 0 {
            b.navigate(b.snapshot.root().unwrap()).unwrap();
        }
        b.pane(pane).unwrap();
        let output = render(&b, 160, 48);
        assert!(!output.contains('\x1b'));
        assert!(!output.contains('\u{fffd}'));
        assert!(output.contains("\\x1B\\x0A\\xFF"));
    }
    b.pane(Pane::Links).unwrap();
    b.enter().unwrap();
    assert!(render(&b, 160, 48).contains("\\x1B\\x0A\\xFF"));
}
#[test]
#[ignore = "scale"]
fn million_directory_navigation() {
    let count = std::env::var("TIDEMARK_TUI_SCALE_DIRS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(1_000_000);
    let temp = ::test_support::private_tempdir().unwrap();
    let root = RawPath::new(b"/fixture".to_vec()).unwrap();
    let mut meta = fixture::meta(root, 1_000_000);
    meta.dirs = u64::from(count) + 1;
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    let mut root = fixture::row(0, None, b"/fixture", u64::from(count) * 4096);
    root.subtree_end = count;
    writer.put_dirs(&[root]).unwrap();
    for start in (1..=count).step_by(PAGE) {
        let rows = (start..=count.min(start.saturating_add(999)))
            .map(|id| fixture::row(id, Some(0), format!("/fixture/{id:08}").as_bytes(), 4096))
            .collect::<Vec<_>>();
        writer.put_dirs(&rows).unwrap();
    }
    let file = writer.finish(&meta).unwrap();
    let snapshot = Snapshot::open(&file).unwrap();
    let mut b = Browser {
        dir: snapshot.root().unwrap(),
        snapshot,
        snapshot_file: file.clone(),
        old: None,
        prev_open_failed_at: None,
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        clock: 1_000_100,
        history: history::History::default(),
        history_refresh: false,
        data_revision: 0,
        info_subject: None,
        info_error: None,
        pages: Pages::default(),
        refresh: None,
        theme: theme::Theme::plain(),
        refresh_elapsed_ms: 0,
        main_selected: 0,
    };
    for step in 0..10 {
        let start = std::time::Instant::now();
        b.load().unwrap();
        let elapsed = start.elapsed();
        println!(
            "dirs={count} step={step} load_page_ms={:.3}",
            elapsed.as_secs_f64() * 1000.0
        );
        let nav = std::time::Instant::now();
        b.state.move_by(1, b.count());
        println!("navigation_ms={:.3}", nav.elapsed().as_secs_f64() * 1000.0);
        let page = std::time::Instant::now();
        b.page(false).unwrap();
        println!("next_page_ms={:.3}", page.elapsed().as_secs_f64() * 1000.0);
    }
}

#[test]
fn query_pages_skip_nested_subtrees_and_support_back() {
    let temp = ::test_support::private_tempdir().unwrap();
    let mut rows = vec![
        fixture::row(0, None, b"/fixture", 10),
        fixture::row(1, Some(0), b"/fixture/a", 5),
        fixture::row(2, Some(1), b"/fixture/a/nested", 2),
        fixture::row(3, Some(0), b"/fixture/b", 5),
    ];
    rows[0].subtree_end = 3;
    rows[1].subtree_end = 2;
    let file = fixture::snapshot(temp.path(), &rows, 1_000_000);
    let snapshot = Snapshot::open(&file).unwrap();
    let mut b = Browser {
        dir: snapshot.root().unwrap(),
        snapshot,
        snapshot_file: file.clone(),
        old: None,
        prev_open_failed_at: None,
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        clock: 1_000_100,
        history: history::History::default(),
        history_refresh: false,
        data_revision: 0,
        info_subject: None,
        info_error: None,
        pages: Pages::default(),
        refresh: None,
        theme: theme::Theme::plain(),
        refresh_elapsed_ms: 0,
        main_selected: 0,
    };
    b.load().unwrap();
    assert_eq!(
        b.rows.iter().map(|r| r.0.id).collect::<Vec<_>>(),
        vec![1, 3]
    );
    let (_temp, mut b) = browser(2500);
    b.page(false).unwrap();
    assert_eq!(b.rows[0].0.id, 1001);
    b.page(false).unwrap();
    assert_eq!(b.rows[0].0.id, 2001);
    b.page(true).unwrap();
    assert_eq!(b.rows[0].0.id, 1001);
    b.page(true).unwrap();
    assert_eq!(b.rows[0].0.id, 1);
}
#[test]
fn delta_lookups_and_growth_sort_are_snapshot_relative() {
    let (_temp, mut b) = browser(3);
    let old_temp = ::test_support::private_tempdir().unwrap();
    let mut rows = vec![
        fixture::row(0, None, b"/fixture", 100),
        fixture::row(1, Some(0), b"/fixture/00000001", 8000),
        fixture::row(2, Some(0), b"/fixture/00000002", 100),
    ];
    rows[0].subtree_end = 2;
    let file = fixture::snapshot(old_temp.path(), &rows, 900_000);
    b.old = Some(Snapshot::open(&file).unwrap());
    b.deltas(24, false).unwrap();
    assert_eq!(b.rows[0].1, Some(-3904));
    assert_eq!(b.rows[1].1, Some(3996));
    assert_eq!(b.rows[2].1, Some(4096));
    b.state.sort = state::Sort::Growth;
    b.state.order(&mut b.rows);
    assert_eq!(b.rows[0].0.id, 3);
}

#[test]
fn global_orders_filter_and_boundaries() {
    let (_temp, mut b) = browser(2500);
    // All query orders must cover all pages, with global byte-name ties.
    for sort in [
        state::Sort::Size,
        state::Sort::Reclaim,
        state::Sort::Files,
        state::Sort::Newest,
        state::Sort::Growth,
        state::Sort::Name,
    ] {
        b.state.sort = sort;
        b.boundary(false).unwrap();
        let mut ids = Vec::new();
        loop {
            ids.extend(b.rows.iter().map(|r| r.0.id));
            if b.rows.len() < PAGE {
                break;
            }
            b.page(false).unwrap();
        }
        assert_eq!(ids, (1..=2500).collect::<Vec<_>>());
        b.boundary(true).unwrap();
        assert_eq!(b.rows.last().unwrap().0.id, 2500);
        assert_eq!(b.state.selected, b.count().saturating_sub(1));
        b.boundary(false).unwrap();
        assert_eq!(b.rows[0].0.id, 1);
    }
    b.state.filter = "000020".into();
    b.boundary(false).unwrap();
    let expected = (1..=2500)
        .filter(|id| format!("{id:08}").contains("000020"))
        .collect::<Vec<_>>();
    assert_eq!(b.rows.iter().map(|r| r.0.id).collect::<Vec<_>>(), expected);
    b.state.filter = "no such name".into();
    b.boundary(false).unwrap();
    assert!(b.rows.is_empty());
    assert_eq!(b.count(), 0);
}

#[test]
fn growth_limit_preserves_current_sort() {
    let (_temp, mut b) = browser(10_001);
    b.state.sort = state::Sort::Newest;
    b.cycle_sort().unwrap();
    assert_eq!(b.state.sort, state::Sort::Newest);
    assert!(b.message.contains("10,000"));
    b.cycle_sort().unwrap();
    assert_eq!(b.state.sort, state::Sort::Name);
    let (_temp, mut b) = browser(10_000);
    b.state.sort = state::Sort::Newest;
    b.cycle_sort().unwrap();
    assert_eq!(b.state.sort, state::Sort::Growth);
    assert_eq!(b.pages.growth.as_ref().unwrap().len(), 10_000);
}

#[test]
fn own_files_columns_and_info() {
    let (_temp, mut b) = browser(3);
    b.dir.own_charged = 4096;
    b.dir.own_files = 12;
    let output = render(&b, 160, 48);
    let line = output
        .lines()
        .find(|l| l.contains("./ (files here)"))
        .unwrap();
    assert!(line.contains(&crate::render::size::bytes(4096)));
    assert!(line.contains("12"));
    assert_eq!(line.matches('-').count(), 4);
    b.state.selected = b.own_position();
    b.pane(Pane::Info).unwrap();
    assert!(!b.text.contains("own files as charged"));
    assert!(b.text.contains("Disk 12.0 KiB"));
}

#[test]
fn instances_page_all_rows_forward_backward_and_end() {
    let (_temp, mut b) = browser(1);
    let temp = ::test_support::private_tempdir().unwrap();
    let meta = fixture::meta(b.dir.path.clone(), 1_000_000);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&[b.dir.clone()]).unwrap();
    let rows = (0..2500)
        .map(|id| LinkInstanceRow {
            ino: 100,
            dir_id: 0,
            name: Some(RawName::new(format!("{id:08}").into_bytes()).unwrap()),
        })
        .collect::<Vec<_>>();
    writer.put_link_instances(&rows).unwrap();
    b.snapshot = Snapshot::open(&writer.finish(&meta).unwrap()).unwrap();
    b.pages.inode = Some(100);
    b.state.pane = Pane::Instances;
    b.instance_page(false, true).unwrap();
    assert_eq!(b.pages.instances.len(), 1000);
    assert!(b.text.starts_with("/fixture/00000000"));
    b.page(false).unwrap();
    assert!(b.text.starts_with("/fixture/00001000"));
    b.page(false).unwrap();
    assert_eq!(b.pages.instances.len(), 500);
    b.page(true).unwrap();
    assert!(b.text.starts_with("/fixture/00001000"));
    b.instance_page(true, true).unwrap();
    assert!(b.text.ends_with("/fixture/00002499"));
}

#[test]
fn worker_live_refresh_mutation_and_cancellation() {
    use std::os::unix::fs::MetadataExt;
    let fixture = test_support::materialize(
        r#"
[[entry]]
path = "a/file"
kind = "file"
size = 8192
"#,
    );
    let state = ::test_support::private_tempdir().unwrap();
    let mut config = tidemark_core::config::Config::default();
    config.general.state_dir = state.path().to_owned();
    config.general.socket = state.path().join("test.sock");
    config.throttle.sample_interval = std::time::Duration::from_millis(20);
    let path = RawPath::from_path(&fixture.root).unwrap();
    let rows =
        crate::live_rows::live_rows(&path, &config, &std::sync::atomic::AtomicBool::new(false))
            .unwrap();
    let newest = ["a", "a/file"]
        .map(|path| std::fs::metadata(fixture.root.join(path)).unwrap().mtime())
        .into_iter()
        .max()
        .unwrap();
    assert_eq!(
        rows.iter()
            .find(|row| row.path.as_path() == fixture.root.join("a"))
            .unwrap()
            .newest_mtime,
        newest
    );
    let store = ::test_support::private_tempdir().unwrap();
    let file = fixture::snapshot(store.path(), &rows, 1_000_000);
    let (_temp, mut b) = browser(0);
    b.snapshot = Snapshot::open(&file).unwrap();
    b.snapshot_file = file.clone();
    b.navigate(b.snapshot.root().unwrap()).unwrap();
    let baseline = b.rows[0].0.t_disk;
    std::fs::write(fixture.root.join("a/new"), vec![0; 1024 * 1024]).unwrap();
    b.start_refresh(&config);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
    while b.refresh.is_some() {
        assert!(std::time::Instant::now() < deadline);
        b.poll_refresh().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(b.pages.live.is_some(), "{}", b.message);
    let newest = ["a", "a/file", "a/new"]
        .map(|path| std::fs::metadata(fixture.root.join(path)).unwrap().mtime())
        .into_iter()
        .max()
        .unwrap();
    assert_eq!(
        b.rows[0].0.newest_mtime, newest,
        "live rows retain Unix seconds from stat, through aggregation and refresh"
    );
    let screen = render(&b, 160, 48);
    let row = screen.lines().find(|line| line.contains("a/")).unwrap();
    assert!(row.contains(&draw::fmt_when(newest, b.clock)), "{row}");
    assert_eq!(
        b.rows[0].0.t_disk,
        test_support::oracle::du_disk(fixture.root.join("a"))
    );
    assert_eq!(
        b.rows[0].1,
        Some(i128::from(b.rows[0].0.t_disk) - i128::from(baseline))
    );
    assert!(render(&b, 160, 48).contains("live"));
    config.throttle.min_ops_per_sec = 1;
    config.throttle.max_ops_per_sec = 1;
    b.start_refresh(&config);
    std::thread::sleep(std::time::Duration::from_millis(100));
    b.state.move_by(1, b.count());
    b.refresh
        .as_ref()
        .unwrap()
        .cancel
        .store(true, std::sync::atomic::Ordering::Release);
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while b.refresh.is_some() {
        assert!(std::time::Instant::now() < deadline);
        b.poll_refresh().unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    assert!(b.message.contains("cancelled"));
}

#[test]
fn distinct_metrics_and_previous_snapshot_order_across_pages() {
    let (_temp, mut b) = browser(2500);
    let store = ::test_support::private_tempdir().unwrap();
    let previous = ::test_support::private_tempdir().unwrap();
    let mut rows = vec![fixture::row(0, None, b"/fixture", 1)];
    rows[0].subtree_end = 2500;
    for id in 1..=2500 {
        let mut row = fixture::row(
            id,
            Some(0),
            format!("/fixture/{id:08}").as_bytes(),
            u64::from(id * 37 % 2501),
        );
        row.t_reclaimable = u64::from(id * 19 % 2501);
        row.t_files = u64::from(id * 13 % 2501);
        row.newest_mtime = i64::from(id * 7 % 2501);
        rows.push(row);
    }
    let mut old_rows = rows.clone();
    for row in old_rows.iter_mut().skip(1) {
        row.t_disk = u64::from(row.id * 29 % 2501);
    }
    b.old = Some(Snapshot::open(&fixture::snapshot(previous.path(), &old_rows, 900_000)).unwrap());
    b.snapshot = Snapshot::open(&fixture::snapshot(store.path(), &rows, 1_000_000)).unwrap();
    b.dir = b.snapshot.root().unwrap();
    for sort in [
        state::Sort::Size,
        state::Sort::Reclaim,
        state::Sort::Files,
        state::Sort::Newest,
        state::Sort::Growth,
        state::Sort::Name,
    ] {
        b.state.sort = sort;
        b.state.filter = "00001".into(); // More than one SQL page; matches raw child names.
        b.boundary(false).unwrap();
        let mut expected = rows
            .iter()
            .skip(1)
            .filter(|r| byte_contains(&r.name, b"00001"))
            .collect::<Vec<_>>();
        let value = |r: &DirRow| match sort {
            state::Sort::Size => i128::from(r.t_disk),
            state::Sort::Reclaim => i128::from(r.t_reclaimable),
            state::Sort::Files => i128::from(r.t_files),
            state::Sort::Newest => i128::from(r.newest_mtime),
            state::Sort::Growth => {
                i128::from(r.t_disk) - i128::from(old_rows[r.id as usize].t_disk)
            }
            state::Sort::Name => 0,
        };
        expected.sort_by(|a, b| value(b).cmp(&value(a)).then_with(|| a.name.cmp(&b.name)));
        assert!(expected.len() > PAGE);
        let mut actual = Vec::new();
        loop {
            actual.extend(b.rows.iter().map(|r| r.0.id));
            if b.rows.len() < PAGE {
                break;
            }
            b.page(false).unwrap();
        }
        assert_eq!(
            actual,
            expected.iter().map(|r| r.id).collect::<Vec<_>>(),
            "{sort:?}"
        );
        b.boundary(true).unwrap();
        assert_eq!(b.rows.last().unwrap().0.id, expected.last().unwrap().id);
        b.boundary(false).unwrap();
        assert_eq!(b.rows.first().unwrap().0.id, expected.first().unwrap().id);
    }
}

#[test]
fn live_growth_order_keeps_snapshot_display_deltas() {
    let mut rows = vec![
        (fixture::row(1, Some(0), b"/fixture/a", 50), Some(100)),
        (fixture::row(2, Some(0), b"/fixture/b", 60), Some(-100)),
    ];
    let growth = std::collections::HashMap::from([(1, Some(-50)), (2, Some(60))]);
    order_live(state::Sort::Growth, &mut rows, &growth);
    assert_eq!(rows[0].0.id, 2);
    assert_eq!(rows[0].1, Some(-100));
    assert_eq!(rows[1].1, Some(100));
}

#[test]
fn live_additions_exceeding_growth_cap_fall_back_without_exiting() {
    let (_temp, mut b) = browser(1);
    b.state.sort = state::Sort::Growth;
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    let children = (1..=10_001)
        .map(|id| {
            (
                fixture::row(id, Some(0), format!("/fixture/{id:08}").as_bytes(), 1),
                Some(1),
            )
        })
        .collect();
    tx.send(Ok(LiveResult {
        root: b.dir.clone(),
        children,
        sort: state::Sort::Growth,
        growth: Default::default(),
    }))
    .unwrap();
    b.poll_refresh().unwrap();
    assert_eq!(b.state.sort, state::Sort::Name);
    assert_eq!(b.rows.len(), PAGE);
    assert!(b.message.contains("showing name"));
}

#[test]
fn human_header_and_pane_labels_are_lowercase() {
    let (_temp, b) = browser(1);
    for width in [80, 100] {
        let text = render(&b, width, 24);
        assert!(text.contains("depth · /fixture · sort size"), "{text}");
        assert!(text.contains("walk"), "{text}");
        assert!(text.contains("1m 39s ago"), "{text}");
        assert!(text.contains("20260101T000000Z-00000000"), "{text}");
        assert!(text.contains("steady"), "{text}");
        assert!(
            text.contains(if width == 80 {
                "hw "
            } else {
                "root high-water"
            }),
            "{text}"
        );
        assert!(!text.contains("Walk") && !text.contains("Size"), "{text}");
        assert!(!text.contains("degraded=false"), "{text}");
    }
}

#[test]
fn own_row_sorts_and_enter_lists_direct_files() {
    let (_temp, mut b) = browser(3);
    b.dir.own_charged = 8192;
    b.dir.own_files = 12;
    for sort in [state::Sort::Size, state::Sort::Name, state::Sort::Files] {
        b.state.sort = sort;
        b.state.order(&mut b.rows);
        assert_eq!(b.own_position(), 0);
    }
    b.state.selected = b.own_position();
    b.enter().unwrap();
    assert_eq!(b.state.pane, Pane::Files);
    // Sorting uses the displayed directory; details use its immutable snapshot row.
    assert!(b
        .text
        .contains("0 retained representatives listed; 1 file entries here"));
}

#[test]
fn direct_retained_files_filter_before_limit_and_show_utc() {
    let (_temp, mut b) = browser(1);
    let temp = ::test_support::private_tempdir().unwrap();
    let mut rows = vec![b.dir.clone(), b.rows[0].0.clone()];
    rows[0].own_charged = 12288;
    rows[0].own_files = 3;
    let meta = fixture::meta(rows[0].path.clone(), 2_000_000);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&rows).unwrap();
    writer
        .put_files(&[
            FileRow {
                id: 0,
                dir_id: 1,
                name: Some(RawName::new(b"child".to_vec()).unwrap()),
                ino: 1,
                disk: 16384,
                apparent: 20000,
                mtime: 0,
                nlink: 1,
            },
            FileRow {
                id: 1,
                dir_id: 0,
                name: Some(RawName::new(b"root".to_vec()).unwrap()),
                ino: 2,
                disk: 8192,
                apparent: 9000,
                mtime: 0,
                nlink: 1,
            },
        ])
        .unwrap();
    let file = writer.finish(&meta).unwrap();
    b.snapshot = Snapshot::open(&file).unwrap();
    b.dir = b.snapshot.root().unwrap();
    let direct =
        crate::cmd::files::retained_files(&b.snapshot, 0, &Default::default(), 1, true).unwrap();
    assert_eq!(direct.len(), 1);
    assert_eq!(direct[0].row.dir_id, 0);
    assert_eq!(
        crate::cmd::files::retained_files(&b.snapshot, 0, &Default::default(), 1, false).unwrap()
            [0]
        .row
        .dir_id,
        1
    );
    b.state.selected = b.own_position();
    b.enter().unwrap();
    assert!(b.text.contains("/fixture/root"));
    assert!(!b.text.contains("child"));
    assert!(b.text.contains("1970-01-01 00:00:00Z"));
    assert!(b
        .text
        .contains("1 retained representatives listed; 3 file entries here"));
    assert!(b.text.contains(&format!(
        "charged to this directory: {}",
        crate::render::size::bytes(12288)
    )));
}

#[test]
fn missing_path_message_does_not_invent_a_mount_reason() {
    let (_temp, b) = browser(0);
    let context = crate::Context {
        config: Default::default(),
        config_path: "/unused".into(),
        json: false,
        color: false,
    };
    let path = RawPath::new(b"/fixture/data".to_vec()).unwrap();
    let error = crate::cmd::snapshots::missing_path(&context, &path, &b.snapshot).to_string();
    assert!(error.contains("of /fixture ("));
    assert!(error.contains("run: tidemark scan /fixture/data"));
    // An absent path alone does not prove a filesystem boundary.
    assert!(!error.contains("different filesystem"));
}

#[test]
fn own_row_appears_once_across_pages_and_sorts_between_children() {
    let (_temp, mut b) = browser(2500);
    for sort in [
        state::Sort::Size,
        state::Sort::Files,
        state::Sort::Name,
        state::Sort::Growth,
    ] {
        b.state.sort = sort;
        b.boundary(false).unwrap();
        let mut own_rows = 0;
        loop {
            own_rows += usize::from(b.own_position() != usize::MAX);
            if b.rows.len() < PAGE {
                break;
            }
            b.page(false).unwrap();
        }
        assert_eq!(own_rows, 1);
    }
    let (_temp, mut b) = browser(3);
    b.dir.own_charged = 8192;
    b.dir.own_files = 2;
    b.rows[0].0.t_disk = 16384;
    b.rows[0].0.t_files = 3;
    b.rows[1].0.t_files = 1;
    b.rows[2].0.t_files = 0;
    for sort in [state::Sort::Size, state::Sort::Files] {
        b.state.sort = sort;
        b.state.order(&mut b.rows);
        assert_eq!(b.own_position(), 1);
        assert_eq!(b.child_at(0).unwrap().0.id, 1);
        assert_eq!(b.child_at(2).unwrap().0.id, 2);
    }
    b.state.selected = b.own_position();
    b.enter().unwrap();
    b.close_pane();
    assert_eq!(b.state.pane, Pane::Main);
}

#[test]
fn missing_path_message_names_recorded_filesystem_boundary() {
    let temp = ::test_support::private_tempdir().unwrap();
    let root = fixture::row(0, None, b"/", 4096);
    let meta = fixture::meta(root.path.clone(), 1_000_000);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&[root]).unwrap();
    writer
        .put_skipped(&[tidemark_core::snapshot::SkipRow {
            dir_id: 0,
            name: RawName::new(b"data".to_vec()).unwrap(),
            reason: tidemark_core::scan::SkipReason::OtherFs,
        }])
        .unwrap();
    let file = writer.finish(&meta).unwrap();
    let snapshot = Snapshot::open(&file).unwrap();
    let mut config = tidemark_core::config::Config::default();
    config.general.socket = temp.path().join("unreachable.sock");
    let context = crate::Context {
        config,
        config_path: "/unused".into(),
        json: false,
        color: false,
    };
    let path = RawPath::new(b"/data".to_vec()).unwrap();
    let message = crate::cmd::snapshots::missing_path(&context, &path, &snapshot).to_string();
    assert!(message.contains("selected complete snapshot"));
    assert!(message.contains("of / ("));
    assert!(message.contains("a different filesystem"));
    assert!(message.contains("run: tidemark scan /data"));
    assert!(!message.contains("next scheduled scan"));
}

#[test]
fn missing_path_message_includes_daemon_schedule_for_configured_root() {
    use std::io::{BufRead, BufReader, Write};
    let temp = ::test_support::private_tempdir().unwrap();
    let socket = temp.path().join("daemon.sock");
    let listener = match std::os::unix::net::UnixListener::bind(&socket) {
        Ok(listener) => listener,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
            ) =>
        {
            eprintln!("socket unavailable; skipping schedule message test: {error}");
            return;
        }
        Err(error) => panic!("bind fixture socket: {error}"),
    };
    let worker = std::thread::spawn(move || {
        let mut stream = accept_schedule_before(
            &listener,
            std::time::Instant::now() + std::time::Duration::from_secs(5),
        )
        .unwrap();
        stream
            .set_read_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(std::time::Duration::from_secs(5)))
            .unwrap();
        let mut request = String::new();
        BufReader::new(&stream).read_line(&mut request).unwrap();
        let request: serde_json::Value = serde_json::from_str(&request).unwrap();
        assert_eq!(request["method"], "scan.status");
        writeln!(stream, "{}", serde_json::json!({"v": 1, "ok": true, "id": request["id"], "result": [{"root": "/data", "running": null, "last_complete": null, "next_due": 0}], "error": null})).unwrap();
    });
    let (_temp, b) = browser(0);
    let mut config = tidemark_core::config::Config::default();
    config.general.socket = socket;
    let context = crate::Context {
        config,
        config_path: "/unused".into(),
        json: false,
        color: false,
    };
    let path = RawPath::new(b"/data".to_vec()).unwrap();
    let message = crate::cmd::snapshots::missing_path(&context, &path, &b.snapshot).to_string();
    worker.join().unwrap();
    assert!(
        message.contains("next scheduled scan: 1970-01-01 00:00:00Z"),
        "{message}"
    );
}

#[test]
fn end_includes_own_row_at_an_exact_page_boundary() {
    let (_temp, mut b) = browser(2000);
    b.boundary(true).unwrap();
    assert_eq!(b.own_position(), PAGE);
    assert_eq!(b.state.selected, PAGE);
    b.page(true).unwrap();
    assert_eq!(b.own_position(), usize::MAX);
    b.state.sort = state::Sort::Name;
    b.boundary(true).unwrap();
    b.page(true).unwrap();
    assert_eq!(b.pages.offset, 0);
    assert_eq!(b.own_position(), 0);
}

#[test]
fn own_files_hardlink_totals_are_separate() {
    // top_files=1 retains only shared (8 MiB), charged at /r, not /r/a.
    let (_temp, mut b) = browser(0);
    let temp = ::test_support::private_tempdir().unwrap();
    let mut rows = vec![
        fixture::row(0, None, b"/r", 9 * 1024 * 1024),
        fixture::row(1, Some(0), b"/r/a", 9 * 1024 * 1024),
        fixture::row(2, Some(0), b"/r/b", 8 * 1024 * 1024),
    ];
    rows[0].subtree_end = 2;
    rows[0].own_files = 0;
    rows[0].own_charged = 8 * 1024 * 1024;
    rows[0].t_files = 3;
    rows[0].t_inodes = 2;
    rows[0].t_dirs = 3;
    rows[0].t_reclaimable = 9 * 1024 * 1024;
    // The unretained /r/a/small is a single-link 1 MiB file.
    rows[1].own_files = 2;
    rows[1].own_charged = 1024 * 1024;
    rows[1].t_charged = 1024 * 1024;
    rows[1].t_files = 2;
    rows[1].t_inodes = 2;
    rows[1].t_reclaimable = 1024 * 1024;
    rows[2].own_charged = 0;
    rows[2].t_charged = 0;
    rows[2].t_reclaimable = 0;
    for row in &mut rows {
        row.own_apparent = row.own_charged;
        row.age0 = row.t_charged;
    }
    let meta = fixture::meta(rows[0].path.clone(), 2_000_000);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&rows).unwrap();
    writer
        .put_link_sets(&[LinkSetRow {
            ino: 42,
            nlink: 2,
            found: 2,
            complete: true,
            disk: 8 * 1024 * 1024,
            apparent: 8 * 1024 * 1024,
            mtime: 0,
            lca_dir_id: 0,
        }])
        .unwrap();
    writer
        .put_link_instances(&[1, 2].map(|dir_id| LinkInstanceRow {
            ino: 42,
            dir_id,
            name: Some(RawName::new(b"shared".to_vec()).unwrap()),
        }))
        .unwrap();
    writer
        .put_files(&[FileRow {
            id: 0,
            dir_id: 1,
            name: Some(RawName::new(b"shared".to_vec()).unwrap()),
            ino: 42,
            disk: 8 * 1024 * 1024,
            apparent: 8 * 1024 * 1024,
            mtime: 0,
            nlink: 2,
        }])
        .unwrap();
    let file = writer.finish(&meta).unwrap();
    b.snapshot = Snapshot::open(&file).unwrap();
    b.navigate(b.snapshot.dir(1).unwrap()).unwrap();
    b.state.selected = b.own_position();
    b.enter().unwrap();
    assert!(
        b.text
            .contains("1 retained representatives listed; 2 file entries here"),
        "{}",
        b.text
    );
    assert!(b.text.contains("[hard links]"));
}

#[test]
fn own_files_after_refresh_use_snapshot_observation() {
    let (_temp, mut b) = browser(0);
    let temp = ::test_support::private_tempdir().unwrap();
    let mut root = b.dir.clone();
    root.own_files = 1;
    root.own_charged = 8 * 1024 * 1024;
    let meta = fixture::meta(root.path.clone(), 2_000_000);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&[root.clone()]).unwrap();
    writer
        .put_files(&[FileRow {
            id: 0,
            dir_id: 0,
            name: Some(RawName::new(b"A".to_vec()).unwrap()),
            ino: 42,
            disk: 8 * 1024 * 1024,
            apparent: 8 * 1024 * 1024,
            mtime: 0,
            nlink: 1,
        }])
        .unwrap();
    let file = writer.finish(&meta).unwrap();
    b.snapshot = Snapshot::open(&file).unwrap();
    b.dir = root.clone();
    // A was deleted and replaced by B (16 MiB). Deliver the live scan result.
    root.own_charged = 16 * 1024 * 1024;
    install_refresh(&mut b, root, vec![]);
    assert_eq!(b.dir.own_charged, 16 * 1024 * 1024);
    b.state.selected = b.own_position();
    b.enter().unwrap();
    assert!(b.text.contains("/fixture/A"));
    assert!(!b.text.contains("/fixture/B"));
    assert!(
        b.text
            .contains("1 retained representatives listed; 1 file entries here"),
        "{}",
        b.text
    );
    assert!(b.text.contains(&meta.scan_id.to_string()));
    assert!(b
        .text
        .contains(&crate::render::size::timestamp(meta.finished_at)));
    assert!(b.text.contains("does not include changes since"));
    for width in [80, 160] {
        let screen = render(&b, width, 48);
        assert!(screen.contains(&crate::render::size::timestamp(meta.finished_at)));
        assert!(screen.contains("does not include changes since this snapshot"));
        assert!(!screen.contains("live (Δ snapshot)"));
        // The file list stays the snapshot. The finished refresh still reaches the message line.
        assert!(screen.contains("live values; delta"), "{screen}");
        if width >= 160 {
            assert!(
                screen.contains("live values; delta against snapshot"),
                "{screen}"
            );
        }
    }
}

fn install_refresh(b: &mut Browser, root: DirRow, children: Vec<(DirRow, Option<i128>)>) {
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    tx.send(Ok(LiveResult {
        root,
        children,
        sort: b.state.sort,
        growth: Default::default(),
    }))
    .unwrap();
    b.poll_refresh().unwrap();
}

#[test]
fn refresh_keeps_the_own_row_when_its_bytes_grow_past_the_children() {
    let (_temp, mut b) = browser(3);
    let config = tidemark_core::config::Config::default();
    assert_eq!(b.own_position(), 3);
    for _ in 0..3 {
        b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    }
    assert_eq!(b.state.selected, b.own_position());
    assert_eq!(b.main_selected, b.own_position());
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    assert_eq!(b.state.pane, Pane::Info);
    let mut root = b.dir.clone();
    root.own_charged = 8192;
    let children = b.rows.clone();
    install_refresh(&mut b, root, children);
    assert_eq!(b.main_selected, b.own_position());
    assert_ne!(b.own_position(), usize::MAX);
    assert!(
        !b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
    let screen = render(&b, 140, 40);
    assert!(screen.contains("(whole dir)"), "{screen}");
    assert!(!screen.contains("strata · 00000003/"), "{screen}");
    assert!(!screen.contains("soundings · 00000003/"), "{screen}");
}

#[test]
fn poll_refresh_keeps_dir_id_and_parent_id() {
    let (_temp, mut b) = browser(3);
    let id = b.dir.id;
    let parent_id = b.dir.parent_id;
    let mut root = b.dir.clone();
    root.id = id.saturating_add(1);
    root.parent_id = Some(42);
    root.t_disk += 1;
    let t_disk = root.t_disk;
    let children = b.rows.clone();
    install_refresh(&mut b, root, children);
    assert_eq!(b.dir.id, id);
    assert_eq!(b.dir.parent_id, parent_id);
    assert_eq!(b.dir.t_disk, t_disk);
}

#[test]
fn refresh_keeps_a_child_on_a_later_page() {
    let (_temp, mut b) = browser(1_001);
    let config = tidemark_core::config::Config::default();
    b.state.sort = state::Sort::Name;
    b.boundary(false).unwrap();
    b.on_key(crossterm::event::KeyCode::PageDown, &config)
        .unwrap();
    assert_eq!(b.pages.offset, 1_000);
    assert_eq!(b.own_position(), usize::MAX);
    let row = b
        .child_at(b.main_selected)
        .expect("child on the later page")
        .0
        .clone();
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    let children = (1..=1_001)
        .map(|id| {
            (
                fixture::row(id, Some(0), format!("/fixture/{id:08}").as_bytes(), 4096),
                None,
            )
        })
        .collect();
    let root = b.dir.clone();
    install_refresh(&mut b, root, children);
    assert_eq!(b.pages.offset, 1_000);
    assert_eq!(b.child_at(b.main_selected).unwrap().0.path, row.path);
    assert!(
        !b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
    let screen = render(&b, 140, 40);
    let name = escape(&row.name);
    assert!(screen.contains(&format!("strata · {name}/")), "{screen}");
    assert!(screen.contains(&format!("soundings · {name}/")), "{screen}");
    assert!(!screen.contains("(whole dir)"), "{screen}");
}

#[test]
fn live_refresh_from_later_page_restores_own_row() {
    let (_temp, mut b) = browser(2500);
    b.state.sort = state::Sort::Name;
    b.boundary(false).unwrap();
    b.page(false).unwrap();
    assert!(b.pages.cursor.is_some());
    assert_eq!(b.pages.offset, 1_000);
    let kept = b.child_at(b.main_selected).unwrap().0.path.clone();
    let children = (1..=2500)
        .map(|id| {
            (
                fixture::row(id, Some(0), format!("/fixture/{id:08}").as_bytes(), 4096),
                None,
            )
        })
        .collect();
    let root = b.dir.clone();
    install_refresh(&mut b, root, children);
    // Name sort puts `./` on page 0. The open child on this page stays selected.
    assert_eq!(b.pages.offset, 1_000);
    assert_eq!(b.child_at(b.main_selected).unwrap().0.path, kept);
    assert_eq!(b.own_position(), usize::MAX);
    assert!(b.pages.cursor.is_none());
    assert!(!b.pages.reverse);
    b.boundary(false).unwrap();
    let mut own_rows = 0;
    loop {
        own_rows += usize::from(b.own_position() != usize::MAX);
        if b.rows.len() < PAGE {
            break;
        }
        b.page(false).unwrap();
    }
    assert_eq!(own_rows, 1);
}

#[test]
fn operator_changed_config_recipes_apply_to_running_daemon() {
    let guide = include_str!("../../../../docs/operator-guide.md");
    let metrics = guide
        .split("Prometheus textfile export is off by default")
        .nth(1)
        .unwrap();
    let recipe = metrics.split("Tidemark writes Prometheus").next().unwrap();
    assert!(recipe.contains("sudo tidemark deploy stage --from ./tidemark.toml --start-now"));
    let hot = guide
        .split("An unwatched path exits 5")
        .nth(1)
        .unwrap()
        .split("<!--")
        .next()
        .unwrap();
    assert!(hot.contains("--start-now"));
}

fn selected_missing_path(partial: bool, newer_has_path: bool, newer_complete: bool) -> String {
    let state = ::test_support::private_tempdir().unwrap();
    let live = ::test_support::private_tempdir().unwrap();
    std::fs::create_dir(live.path().join("new")).unwrap();
    let root = RawPath::from_path(live.path()).unwrap();
    let path = RawPath::from_path(&live.path().join("new")).unwrap();
    let mut old = fixture::meta(root.clone(), 1_000_000);
    if partial {
        old.status = tidemark_core::scan::SnapshotStatus::Partial;
    }
    let dir = state
        .path()
        .join("snapshots")
        .join(tidemark_store::catalog::root_slug(&root));
    let mut row = fixture::row(0, None, root.as_bytes(), 0);
    row.subtree_end = 0;
    let mut writer = tidemark_store::writer::SnapshotWriter::create(&dir, &old).unwrap();
    writer.put_dirs(&[row.clone()]).unwrap();
    writer.finish(&old).unwrap();
    let mut newer = fixture::meta(root, 2_000_000);
    newer.scan_id = serde_json::from_str("\"20260102T000000Z-00000000\"").unwrap();
    if !newer_complete {
        newer.status = tidemark_core::scan::SnapshotStatus::Partial;
    }
    row.subtree_end = u32::from(newer_has_path);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(&dir, &newer).unwrap();
    let mut rows = vec![row];
    if newer_has_path {
        rows.push(fixture::row(1, Some(0), path.as_bytes(), 0));
    }
    writer.put_dirs(&rows).unwrap();
    writer.finish(&newer).unwrap();
    let catalog = tidemark_store::catalog::Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    let mut config = tidemark_core::config::Config::default();
    config.general.state_dir = state.path().to_path_buf();
    let context = crate::Context {
        config,
        config_path: "/unused".into(),
        json: false,
        color: false,
    };
    let (_, latest) =
        crate::cmd::snapshots::select(&context, Some(&live.path().join("new")), None).unwrap();
    assert_eq!(
        latest.dir_by_path(&path).unwrap().is_some(),
        newer_has_path && newer_complete
    );
    let (_, selected) = crate::cmd::snapshots::select(
        &context,
        Some(&live.path().join("new")),
        Some(&old.scan_id.to_string()),
    )
    .unwrap();
    assert_eq!(selected.meta().status, old.status);
    crate::cmd::snapshots::missing_path(&context, &path, &selected).to_string()
}

#[test]
fn missing_path_explicit_old_snapshot_does_not_deny_newer_evidence() {
    let message = selected_missing_path(false, true, true);
    assert!(message.contains("selected complete snapshot"));
    assert!(!message.contains("no complete snapshot"), "{message}");
    assert!(
        message.contains("newer complete snapshot 20260102T000000Z-00000000"),
        "{message}"
    );
    assert!(message.contains("drop --snapshot"), "{message}");
    assert!(
        message.contains("--snapshot 20260102T000000Z-00000000"),
        "{message}"
    );
    assert!(!message.contains("run: tidemark scan"), "{message}");
}

#[test]
fn missing_path_partial_snapshot_is_labelled_partial() {
    let message = selected_missing_path(true, true, true);
    assert!(message.contains("selected partial snapshot"), "{message}");
    assert!(!message.contains("selected complete snapshot"));
}

#[test]
fn operator_upgrade_explains_metrics_defaults_and_stale_export() {
    let upgrade = include_str!("../../../../docs/operator-guide.md")
        .split("## Upgrade or remove")
        .nth(1)
        .unwrap();
    for required in [
        "omitted",
        "textfile",
        "explicit",
        "tidemark.prom",
        "node_exporter",
        "node_textfile_mtime_seconds",
        "delete",
    ] {
        assert!(
            upgrade.contains(required),
            "upgrade guidance missing {required}"
        );
    }
}

fn accept_schedule_before(
    listener: &std::os::unix::net::UnixListener,
    deadline: std::time::Instant,
) -> std::io::Result<std::os::unix::net::UnixStream> {
    listener.set_nonblocking(true)?;
    loop {
        match listener.accept() {
            Ok((stream, _)) => return Ok(stream),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                let remaining = deadline.saturating_duration_since(std::time::Instant::now());
                if remaining.is_zero() {
                    return Err(std::io::Error::new(
                        std::io::ErrorKind::TimedOut,
                        "schedule fixture accept deadline",
                    ));
                }
                std::thread::sleep(remaining.min(std::time::Duration::from_millis(10)));
            }
            Err(error) => return Err(error),
        }
    }
}

#[test]
fn schedule_socket_accept_without_client_has_deadline() {
    let temp = ::test_support::private_tempdir().unwrap();
    let listener = match std::os::unix::net::UnixListener::bind(temp.path().join("no-client.sock"))
    {
        Ok(listener) => listener,
        Err(error)
            if matches!(
                error.kind(),
                std::io::ErrorKind::PermissionDenied | std::io::ErrorKind::Unsupported
            ) =>
        {
            eprintln!("socket unavailable; skipping accept deadline test: {error}");
            return;
        }
        Err(error) => panic!("bind fixture socket: {error}"),
    };
    let start = std::time::Instant::now();
    let error = accept_schedule_before(&listener, start + std::time::Duration::from_millis(30))
        .unwrap_err();
    assert_eq!(error.kind(), std::io::ErrorKind::TimedOut);
    assert!(start.elapsed() < std::time::Duration::from_secs(5));
}

// RC3-fix2: each case isolates an unsupported remainder from the re-review.
#[test]
fn own_files_snapshot_totals_cover_remainder_counterexamples() {
    let mut failures = Vec::new();
    for (case, viewed, own_files, charged, retained_dir, retained_disk, nlink) in [
        (
            "unretained shared plus large",
            1,
            2,
            16 * 1024 * 1024,
            1,
            16 * 1024 * 1024,
            1,
        ),
        ("representative in sibling", 2, 1, 0, 1, 8 * 1024 * 1024, 2),
        (
            "LCA without direct files",
            0,
            0,
            8 * 1024 * 1024,
            1,
            8 * 1024 * 1024,
            2,
        ),
        (
            "directory blocks",
            1,
            1,
            1024 * 1024 + 4096,
            1,
            1024 * 1024,
            1,
        ),
        (
            "threshold retention",
            2,
            2,
            1024 * 1024,
            1,
            8 * 1024 * 1024,
            2,
        ),
    ] {
        let (_temp, mut b) = browser(0);
        let temp = ::test_support::private_tempdir().unwrap();
        let mut rows = vec![
            fixture::row(0, None, b"/r", 24 * 1024 * 1024),
            fixture::row(1, Some(0), b"/r/a", 24 * 1024 * 1024),
            fixture::row(2, Some(0), b"/r/b", 8 * 1024 * 1024),
        ];
        rows[0].subtree_end = 2;
        rows[0].own_files = 0;
        rows[0].own_charged = 8 * 1024 * 1024;
        rows[1].own_files = 2;
        rows[1].own_charged = 1024 * 1024;
        rows[2].own_files = 1;
        rows[2].own_charged = 0;
        if case == "unretained shared plus large" {
            rows[1].own_charged = 16 * 1024 * 1024;
        } else if case == "threshold retention" {
            rows[1].own_files = 1;
            rows[1].own_charged = 0;
        } else if case == "directory blocks" {
            rows[0].own_charged = 0;
            rows[1].dir_disk = 4096;
            rows[2].own_files = 0;
        }
        rows[viewed].own_files = own_files;
        rows[viewed].own_charged = charged;
        let shared = if case == "directory blocks" {
            0
        } else {
            8 * 1024 * 1024
        };
        for row in rows.iter_mut().skip(1) {
            row.t_charged = row.own_charged;
            row.t_disk = row.own_charged + shared;
            row.t_files = row.own_files;
            row.t_inodes = row.own_files;
            row.t_reclaimable = row.own_charged;
        }
        rows[0].t_charged = rows.iter().map(|r| r.own_charged).sum();
        rows[0].t_disk = rows[0].t_charged;
        rows[0].t_files = rows.iter().map(|r| r.own_files).sum();
        rows[0].t_inodes = rows[0].t_files - u64::from(shared > 0);
        rows[0].t_reclaimable = rows[0].t_charged;
        rows[0].t_dirs = 3;
        for row in &mut rows {
            row.own_apparent = row.own_charged;
            row.t_apparent = row.t_disk;
            row.age0 = row.t_charged;
        }
        let meta = fixture::meta(rows[0].path.clone(), 2_000_000);
        let mut writer =
            tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
        writer.put_dirs(&rows).unwrap();
        if shared > 0 {
            let ino = if nlink == 1 { 43 } else { 42 };
            writer
                .put_link_sets(&[LinkSetRow {
                    ino,
                    nlink: 2,
                    found: 2,
                    complete: true,
                    disk: shared,
                    apparent: shared,
                    mtime: 0,
                    lca_dir_id: 0,
                }])
                .unwrap();
            writer
                .put_link_instances(&[1, 2].map(|dir_id| LinkInstanceRow {
                    ino,
                    dir_id,
                    name: Some(RawName::new(b"shared".to_vec()).unwrap()),
                }))
                .unwrap();
        }
        let mut files = vec![FileRow {
            id: 0,
            dir_id: retained_dir,
            name: Some(RawName::new(b"retained".to_vec()).unwrap()),
            ino: 42,
            disk: retained_disk,
            apparent: retained_disk,
            mtime: 0,
            nlink,
        }];
        if case == "threshold retention" {
            // top_files=0, threshold=1 MiB retains both inode candidates.
            files.push(FileRow {
                id: 1,
                dir_id: 2,
                name: Some(RawName::new(b"small".to_vec()).unwrap()),
                ino: 43,
                disk: 1024 * 1024,
                apparent: 1024 * 1024,
                mtime: 0,
                nlink: 1,
            });
        }
        writer.put_files(&files).unwrap();
        let file = writer.finish(&meta).unwrap();
        b.snapshot = Snapshot::open(&file).unwrap();
        b.navigate(b.snapshot.dir(viewed as u32).unwrap()).unwrap();
        b.state.selected = b.own_position();
        b.enter().unwrap();
        let listed = files.iter().filter(|f| f.dir_id == viewed as u32).count();
        let expected =
            format!("{listed} retained representatives listed; {own_files} file entries here");
        let charge = format!(
            "charged to this directory: {}",
            crate::render::size::bytes(charged)
        );
        if b.text.contains("not individually retained")
            || !b.text.contains(&expected)
            || !b.text.contains(&charge)
        {
            failures.push(format!("{case}: {}", b.text));
        }
        if nlink > 1 && listed > 0 && retained_dir == viewed as u32 {
            assert!(b.text.contains("[hard links]"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
#[test]
fn live_reverse_boundary_keeps_own_row_for_every_sort() {
    let mut failures = Vec::new();
    for sort in [
        state::Sort::Size,
        state::Sort::Reclaim,
        state::Sort::Files,
        state::Sort::Newest,
        state::Sort::Name,
    ] {
        for (count, rank) in [(1, 1), (2500, 0), (2500, 1000), (2500, 1250), (2500, 2500)] {
            let (_temp, mut b) = browser(count);
            b.state.sort = sort;
            let children = (1..=count)
                .map(|id| {
                    let before = id <= rank;
                    let prefix = if before { '!' } else { 'z' };
                    let mut row = fixture::row(
                        id,
                        Some(0),
                        format!("/fixture/{prefix}{id:08}").as_bytes(),
                        if before { 4096 } else { 0 },
                    );
                    row.t_reclaimable = if before { 4096 } else { 0 };
                    row.newest_mtime = if before { 100 } else { i64::MIN };
                    row.t_files = if before { 2 } else { 0 };
                    (row, None)
                })
                .collect();
            let mut root = b.dir.clone();
            root.own_files = 1;
            root.own_charged = 2048;
            install_refresh(&mut b, root, children);
            // Refresh follows the open directory, which can leave page 0.
            // The reverse-page check below starts at the first live page.
            b.pages.offset = 0;
            b.pages.cursor = None;
            b.pages.reverse = false;
            b.load().unwrap();
            let initial = b.own_position();
            assert_eq!(
                initial,
                if rank < PAGE as u32 || count == 1 {
                    rank as usize
                } else {
                    usize::MAX
                }
            );
            // PgUp and Up on the first selected child both call page(true).
            for _ in 0..2 {
                b.state.selected = 0;
                b.page(true).unwrap();
                if b.own_position() != initial || b.pages.cursor.is_some() {
                    failures.push(format!(
                        "{sort:?}, {count}: no-op reverse changed own position {initial} to {}",
                        b.own_position()
                    ));
                }
            }
            let mut forward_own = usize::from(b.own_position() != usize::MAX);
            while b.rows.len() == PAGE {
                b.page(false).unwrap();
                forward_own += usize::from(b.own_position() != usize::MAX);
            }
            let mut reverse_own = usize::from(b.own_position() != usize::MAX);
            while b.pages.offset > 0 {
                b.page(true).unwrap();
                reverse_own += usize::from(b.own_position() != usize::MAX);
            }
            if forward_own != 1 || reverse_own != 1 {
                failures.push(format!("{sort:?}, {count}: own occurrences forward={forward_own}, reverse={reverse_own}"));
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn navigation_after_refresh_clears_live_footer() {
    let (_temp, mut b) = browser(1);
    let root = b.dir.clone();
    let children = b.rows.clone();
    install_refresh(&mut b, root, children);
    assert!(render(&b, 160, 24).contains("live values; delta against snapshot"));
    b.state.selected = (0..b.count()).find(|&i| b.child_at(i).is_some()).unwrap();
    b.enter().unwrap();
    assert_eq!(b.dir.id, 1);
    assert!(b.pages.live.is_none());
    assert!(!render(&b, 160, 24).contains("live values; delta against snapshot"));
    b.navigate(b.snapshot.dir(0).unwrap()).unwrap();
    assert!(!render(&b, 160, 24).contains("live values; delta against snapshot"));
}

#[test]
fn post_tui_disjoint_traversals_all_sorts() {
    for sort in [
        state::Sort::Size,
        state::Sort::Reclaim,
        state::Sort::Files,
        state::Sort::Newest,
        state::Sort::Name,
        state::Sort::Growth,
    ] {
        for rank in [0, 999, 1000, 2000, 2500] {
            let count = 2500;
            for live in [false, true] {
                let (_temp, mut b) = browser(count);
                b.state.sort = sort;
                // Own rank 750 reproduces the review's overlap for every numeric/name sort.
                let mut root = b.dir.clone();
                root.own_charged = 2048;
                root.own_files = 1;
                let mut rows = vec![root];
                for id in 1..=count {
                    let before = id <= rank;
                    let mut row = fixture::row(
                        id,
                        Some(0),
                        format!("/fixture/{}{id:08}", if before { '!' } else { 'z' }).as_bytes(),
                        if before { 4096 } else { 0 },
                    );
                    row.t_files = if before { 2 } else { 0 };
                    row.t_reclaimable = if before { 4096 } else { 0 };
                    row.newest_mtime = i64::MIN;
                    rows.push(row);
                }
                let temp = ::test_support::private_tempdir().unwrap();
                let file = fixture::snapshot(temp.path(), &rows, 1_000_000);
                b.snapshot = Snapshot::open(&file).unwrap();
                b.dir = b.snapshot.root().unwrap();
                if live {
                    let children = b
                        .snapshot
                        .children_page(
                            0,
                            tidemark_core::snapshot::SortKey::Name,
                            false,
                            None,
                            false,
                            10000,
                            b"",
                        )
                        .unwrap()
                        .into_iter()
                        .map(|r| (r, None))
                        .collect();
                    let root = b.dir.clone();
                    install_refresh(&mut b, root, children);
                }
                for reverse in [false, true] {
                    b.boundary(reverse).unwrap();
                    for _ in 0..3 {
                        let own_rank = if sort == state::Sort::Growth {
                            count
                        } else {
                            rank
                        } as usize;
                        let expected = own_rank >= b.pages.offset
                            && (own_rank < b.pages.offset + b.rows.len()
                                || b.pages.offset + b.rows.len() == count as usize);
                        assert_eq!(b.own_position() != usize::MAX, expected,
                            "membership {sort:?} rank={rank} live={live} offset={} reverse={reverse}", b.pages.offset);
                        if b.pages.offset > 0 {
                            let offset = b.pages.offset;
                            b.state.selected = 0;
                            let first = b.child_at(0).map(|r| r.0.id).unwrap_or(0);
                            b.page(true).unwrap();
                            b.state.selected = b.count() - 1;
                            let previous =
                                b.child_at(b.state.selected).map(|r| r.0.id).unwrap_or(0);
                            let own_rank = if sort == state::Sort::Growth {
                                count
                            } else {
                                rank
                            };
                            let mut logical = (1..=count).collect::<Vec<_>>();
                            logical.insert(own_rank as usize, 0);
                            let first_rank = logical.iter().position(|&id| id == first).unwrap();
                            assert_eq!(
                                previous,
                                logical[first_rank - 1],
                                "Up must move exactly one row"
                            );
                            assert_eq!(
                                b.own_position() != usize::MAX,
                                own_rank as usize >= b.pages.offset
                                    && ((own_rank as usize) < b.pages.offset + b.rows.len()
                                        || b.pages.offset + b.rows.len() == count as usize)
                            );
                            b.page(false).unwrap();
                            assert_eq!(b.pages.offset, offset);
                            assert_eq!(b.child_at(0).map(|r| r.0.id).unwrap_or(0), first);
                            // Down from the previous page's last row, then Up,
                            // must return to that same adjacent logical row.
                            b.page(true).unwrap();
                            b.state.selected = b.count() - 1;
                            assert_eq!(
                                b.child_at(b.state.selected).map(|r| r.0.id).unwrap_or(0),
                                previous
                            );
                            b.page(false).unwrap();
                        }
                        b.page(reverse).unwrap();
                    }
                }
                let mut forward = Vec::new();
                b.boundary(false).unwrap();
                for _ in 0..count + 10 {
                    forward.push(b.child_at(b.state.selected).map(|r| r.0.id).unwrap_or(0));
                    if b.state.selected + 1 == b.count() {
                        let offset = b.pages.offset;
                        b.page(false).unwrap();
                        if offset == b.pages.offset {
                            break;
                        }
                    } else {
                        b.state.move_by(1, b.count());
                    }
                }
                let mut backward = Vec::new();
                b.boundary(true).unwrap();
                for _ in 0..count + 10 {
                    backward.push(b.child_at(b.state.selected).map(|r| r.0.id).unwrap_or(0));
                    if b.state.selected == 0 {
                        let offset = b.pages.offset;
                        b.page(true).unwrap();
                        if offset == b.pages.offset {
                            break;
                        }
                        b.state.selected = b.count().saturating_sub(1);
                    } else {
                        b.state.move_by(-1, b.count());
                    }
                }
                backward.reverse();
                assert_eq!(
                    forward, backward,
                    "one-row boundary movement {sort:?} live={live}"
                );
                assert_eq!(forward.len(), count as usize + 1);
                for reverse in [true, false] {
                    b.boundary(reverse).unwrap();
                    let mut seen = Vec::new();
                    let mut own = 0;
                    for _ in 0..10 {
                        seen.extend(b.rows.iter().map(|r| r.0.id));
                        own += usize::from(b.own_position() != usize::MAX);
                        let offset = b.pages.offset;
                        b.page(reverse).unwrap();
                        if offset == b.pages.offset {
                            break;
                        }
                    }
                    seen.sort_unstable();
                    assert_eq!(
                        seen,
                        (1..=count).collect::<Vec<_>>(),
                        "{sort:?} live={live} reverse={reverse}"
                    );
                    assert_eq!(own, 1, "{sort:?} live={live} reverse={reverse}");
                }
            }
        }
    }
}

#[test]
fn post_tui_files_pane_fits_80_columns() {
    let (_temp, mut b) = browser(0);
    b.state.selected = b.own_position();
    b.enter().unwrap();
    let rendered = render(&b, 80, 24);
    let title = rendered.lines().nth(1).expect("files title row");
    assert!(
        title.contains(&b.snapshot.meta().scan_id.to_string()),
        "{title}"
    );
    assert!(
        title.contains(&crate::render::size::timestamp(
            b.snapshot.meta().finished_at
        )),
        "{title}"
    );
    assert!(
        rendered.contains("(includes directory blocks and hard-linked files"),
        "{rendered}"
    );
    assert!(
        rendered.contains("whose links share this directory as common ancestor)"),
        "{rendered}"
    );
}

#[test]
fn post_tui_missing_path_keeps_scan_remedy_without_complete_evidence() {
    for (has_path, complete) in [(false, true), (true, false)] {
        let message = selected_missing_path(false, has_path, complete);
        assert!(message.contains("run: tidemark scan"), "{message}");
        assert!(!message.contains("drop --snapshot"), "{message}");
    }
}

fn token_end_column(line: &str, token: &str) -> Option<usize> {
    let start = line.find(token)?;
    Some(line[..start].chars().count() + token.chars().count())
}

#[test]
fn fmt_when_uses_the_utc_year_and_keeps_epoch_dates() {
    let now = 1_781_524_800;
    assert_eq!(draw::fmt_when(now, now), "06-15 12:00");
    assert_eq!(draw::fmt_when(100, 1_000_100), "01-01 00:01");
    assert_eq!(draw::fmt_when(0, now), "1970-01-01");
    assert_eq!(draw::fmt_when(100, now), "1970-01-01");
    assert_eq!(draw::fmt_when(-1, now), "1969-12-31");
}

#[test]
fn fmt_newest_dashes_only_unset_maxima() {
    let now = 1_781_524_800;
    assert_eq!(draw::fmt_newest(0, now), "-");
    assert_eq!(draw::fmt_newest(i64::MIN, now), "-");
    assert_eq!(draw::fmt_newest(100, now), "1970-01-01");
    assert_eq!(draw::fmt_newest(now, now), "06-15 12:00");
}

#[test]
fn files_pane_shows_a_time_zero_file_as_the_epoch_date() {
    let now = 1_781_524_800;
    let hit = tidemark_core::snapshot::FileHit {
        row: FileRow {
            id: 0,
            dir_id: 0,
            name: Some(RawName::new(b"epoch".to_vec()).unwrap()),
            ino: 7,
            disk: 4096,
            apparent: 4096,
            mtime: 0,
            nlink: 1,
        },
        path: Some(RawPath::new(b"/fixture/epoch".to_vec()).unwrap()),
    };
    let text = files_text(&[hit], now);
    let line = text.lines().find(|l| l.contains("/fixture/epoch")).unwrap();
    assert!(line.contains("1970-01-01"), "{line}");
    assert!(!line.split_whitespace().any(|w| w == "-"), "{line}");
}

#[test]
fn fmt_files_stays_within_six_columns() {
    let pins = [
        (9_999, "9999"),
        (10_000, "10.0k"),
        (99_949, "99.9k"),
        (99_950, "100k"),
        (999_499, "999k"),
        (999_500, "1.00M"),
        (9_994_999, "9.99M"),
        (999_499_999, "999M"),
        (1_000_000_000, "1.00G"),
        (1_000_000_000_000, "1.00T"),
        (1_000_000_000_000_000, "1.00P"),
        (1_000_000_000_000_000_000, "1.00E"),
        (u64::MAX, "18.4E"),
    ];
    for (value, expected) in pins {
        let text = draw::fmt_files(value);
        assert_eq!(text, expected, "{value}");
        assert!(text.chars().count() <= 6, "{text}");
    }
}

#[test]
fn level_cells_use_largest_remainder() {
    assert_eq!(draw::level_cells(0, &[1, 1, 1], 4), vec![0, 0, 0]);
    assert_eq!(draw::level_cells(3, &[1, 1, 1], 3), vec![1, 1, 1]);
    assert_eq!(draw::level_cells(3, &[1, 1, 1], 4), vec![2, 1, 1]);
    assert_eq!(draw::level_cells(1_000, &[1], 12), vec![0]);
    let huge = draw::level_cells(u64::MAX, &[u64::MAX, 0], 4);
    assert_eq!(huge, vec![4, 0]);
    // Remainder 7 beats remainder 2, so the last row takes the leftover, not the first.
    assert_eq!(draw::level_cells(10, &[6, 3, 1], 7), vec![4, 2, 1]);
    // Equal remainders: the lower index wins the one leftover.
    assert_eq!(draw::level_cells(5, &[1, 1], 3), vec![1, 0]);
    // A weight past the directory makes the floors overshoot; the split still sums to the width.
    assert_eq!(draw::level_cells(5, &[8, 2], 4), vec![3, 1]);
}

#[test]
fn theme_select_follows_color_flags_and_locale() {
    use theme::{Level, Theme};
    assert_eq!(
        Theme::select(
            false,
            false,
            Some("truecolor"),
            Some("xterm-256color"),
            Some("C.UTF-8")
        )
        .level,
        Level::Mono
    );
    assert_eq!(
        Theme::select(
            true,
            true,
            Some("truecolor"),
            Some("xterm-256color"),
            Some("C.UTF-8")
        )
        .level,
        Level::Mono
    );
    let rgb = Theme::select(
        true,
        false,
        Some("truecolor"),
        Some("xterm-256color"),
        Some("C.UTF-8"),
    );
    assert_eq!(rgb.level, Level::Rgb);
    assert_eq!(rgb.glyphs.anchor, "⚓");
    assert_eq!(
        Theme::select(true, false, None, Some("xterm-256color"), Some("C.UTF-8")).level,
        Level::Indexed
    );
    let basic = Theme::select(true, false, Some("other"), Some("xterm"), Some("C.UTF-8"));
    assert_eq!(basic.level, Level::Basic);
    assert_eq!(basic.color(theme::Role::Text), ratatui::style::Color::Reset);
    assert_eq!(basic.color(theme::Role::Teal), ratatui::style::Color::Cyan);
    assert_eq!(basic.color(theme::Role::Coral), ratatui::style::Color::Red);
    assert_eq!(basic.strata(0), ratatui::style::Color::Reset);
    assert_ne!(basic.strata(0), ratatui::style::Color::White);
    assert_eq!(basic.strata(5), ratatui::style::Color::DarkGray);
    let linux = Theme::select(
        true,
        false,
        Some("truecolor"),
        Some("linux"),
        Some("C.UTF-8"),
    );
    assert_eq!(linux.level, Level::Rgb);
    assert_eq!(linux.glyphs.anchor, "H");
    assert_eq!(linux.glyphs.spinner[0], "|");
    let posix = Theme::select(
        true,
        false,
        Some("24bit"),
        Some("xterm-256color"),
        Some("C"),
    );
    assert_eq!(posix.glyphs.anchor, "H");
    assert!(!posix.glyphs.unicode);
}

#[test]
fn previous_complete_picks_the_nearer_start_then_smaller_id() {
    let rows = vec![
        (
            "b-scan".into(),
            std::path::PathBuf::from("b"),
            1_000,
            Some(9_000),
        ),
        (
            "a-scan".into(),
            std::path::PathBuf::from("a"),
            1_000,
            Some(1_000),
        ),
        (
            "later".into(),
            std::path::PathBuf::from("l"),
            2_500,
            Some(50),
        ),
        (
            "open".into(),
            std::path::PathBuf::from("o"),
            3_000,
            Some(3_000),
        ),
    ];
    assert_eq!(previous_complete(&rows, 2_000, "open").unwrap().0, "a-scan");
    assert_eq!(previous_complete(&rows, 3_000, "open").unwrap().0, "later");
    assert!(previous_complete(&rows, 1_000, "a-scan").is_none());
}

#[test]
fn header_keeps_the_scan_id_and_high_water() {
    let (_temp, mut b) = browser(1);
    let id = b.snapshot.meta().scan_id.to_string();
    b.snapshots = vec![(
        id.clone(),
        b.snapshot_file.clone(),
        b.snapshot.meta().started_at,
        Some(72_241_886),
    )];
    let text = render(&b, 80, 24);
    assert!(text.contains(&id), "{text}");
    assert!(text.contains("walk"), "{text}");
    assert!(text.contains("1m 39s ago"), "{text}");
    assert!(text.contains("hw 68.9 MiB"), "{text}");
    assert!(text.contains("68.9 MiB"), "{text}");
    assert!(text.contains("steady"), "{text}");
    let header = text.lines().next().unwrap();
    assert!(header.ends_with("68.9 MiB"), "{header}");
    let wide = render(&b, 160, 24);
    let wide_header = wide.lines().next().unwrap();
    assert_eq!(wide_header.chars().count(), 160, "{wide_header}");
    assert!(wide_header.ends_with("68.9 MiB"), "{wide_header}");
    assert!(wide_header.contains('≋'), "{wide_header}");
}

/// Older complete scan in `dir`. The scan id is not the fixture default, so it
/// does not collide with `browser`'s `20260101T000000Z-00000000` file.
fn complete_scan(
    dir: &std::path::Path,
    scan_id: &str,
    started_at: i64,
    child_disk: u64,
) -> (String, std::path::PathBuf) {
    let mut rows = vec![
        fixture::row(0, None, b"/fixture", child_disk),
        fixture::row(1, Some(0), b"/fixture/00000001", child_disk),
    ];
    rows[0].subtree_end = 1;
    rows[0].own_charged = 0;
    let mut meta = fixture::meta(rows[0].path.clone(), started_at);
    meta.scan_id = serde_json::from_str(&format!("\"{scan_id}\"")).unwrap();
    meta.dirs = rows.len() as u64;
    let mut writer = tidemark_store::writer::SnapshotWriter::create(dir, &meta).unwrap();
    writer.put_dirs(&rows).unwrap();
    let file = writer.finish(&meta).unwrap();
    (meta.scan_id.to_string(), file)
}

fn child_line(screen: &str) -> String {
    screen
        .lines()
        .find(|line| line.contains("00000001/"))
        .unwrap_or_else(|| panic!("{screen}"))
        .to_string()
}

#[test]
fn header_age_follows_injected_clock() {
    let (_temp, mut b) = browser(1);
    let first = render(&b, 80, 24);
    assert!(first.contains("1m 39s ago"), "{first}");
    b.clock += 75;
    let second = render(&b, 80, 24);
    assert!(second.contains("2m 54s"), "{second}");
    assert!(!second.contains("1m 39s"), "{second}");
}

#[test]
fn previous_snapshot_retry_fills_tide() {
    let (temp, mut b) = browser(1);
    assert!(b.old.is_none());
    assert_eq!(b.clock, 1_000_100);
    let (id, file) = complete_scan(temp.path(), "20251201T000000Z-00000000", 900_000, 0);
    b.snapshots = vec![(id, file.clone(), 900_000, Some(0))];
    let aside = file.with_extension("aside");
    std::fs::rename(&file, &aside).unwrap();
    let grew = crate::render::tree::signed_bytes(4_096);
    b.prepare_frame(140, 24);
    b.deltas(24, false).unwrap();
    let missing = child_line(&render(&b, 140, 24));
    assert!(!missing.contains(&grew), "{missing}");
    std::fs::rename(&aside, &file).unwrap();
    b.clock += 4;
    b.prepare_frame(140, 24);
    b.deltas(24, false).unwrap();
    let cooling = child_line(&render(&b, 140, 24));
    assert!(!cooling.contains(&grew), "{cooling}");
    b.clock += 1;
    b.prepare_frame(140, 24);
    b.deltas(24, false).unwrap();
    let ready = child_line(&render(&b, 140, 24));
    assert!(ready.contains(&grew), "{ready}");
}

#[test]
fn growth_order_rebuilds_when_predecessor_appears() {
    let (temp, mut b) = browser(2);
    let (id, file) = complete_scan(temp.path(), "20251201T000000Z-00000000", 900_000, 4096);
    b.snapshots = vec![(id, file.clone(), 900_000, Some(4096))];
    let aside = file.with_extension("aside");
    std::fs::rename(&file, &aside).unwrap();
    b.prepare(140, 24).unwrap();
    b.state.sort = state::Sort::Growth;
    b.load().unwrap();
    assert!(
        b.rows[0].0.path.as_bytes().ends_with(b"00000001"),
        "{:?}",
        b.rows[0].0.path
    );
    assert!(b.rows.iter().all(|(_, delta)| delta.is_none()));
    std::fs::rename(&aside, &file).unwrap();
    b.clock += history::COOLDOWN;
    b.prepare(140, 24).unwrap();
    assert!(
        b.rows[0].0.path.as_bytes().ends_with(b"00000002"),
        "{:?}",
        b.rows[0].0.path
    );
    assert_eq!(b.rows[0].1, Some(4096));
    b.pages.growth.as_mut().unwrap().reverse();
    let sentinel = b.pages.growth.clone();
    b.state.selected = 1;
    b.prepare(140, 24).unwrap();
    assert_eq!(b.state.selected, 1);
    assert_eq!(b.pages.growth, sentinel);
    // Second None→Some against the Growth cache still held above. Resetting
    // `old` is what makes this Name prepare open again; `is_none` is red on a
    // transition that leaves that cache in place.
    b.old = None;
    b.prev_open_failed_at = None;
    b.state.sort = state::Sort::Name;
    b.prepare(140, 24).unwrap();
    assert!(b.pages.growth.is_none());
    b.state.sort = state::Sort::Growth;
    b.load().unwrap();
    assert!(
        b.rows[0].0.path.as_bytes().ends_with(b"00000002"),
        "{:?}",
        b.rows[0].0.path
    );
}

#[test]
fn growth_prepare_skips_rebuild_when_live_rows_present() {
    let (temp, mut b) = browser(3);
    let (id, file) = complete_scan(temp.path(), "20251201T000000Z-00000000", 900_000, 0);
    b.snapshots = vec![(id, file.clone(), 900_000, Some(0))];
    let aside = file.with_extension("aside");
    std::fs::rename(&file, &aside).unwrap();
    b.prepare(140, 24).unwrap();
    b.state.sort = state::Sort::Growth;
    b.pages.live = Some(b.rows.clone());
    b.pages.live_order = Some(state::Sort::Growth);
    b.pages.live_at = Some(b.clock);
    b.pages.live_growth.clear();
    b.state.selected = 2;
    b.main_selected = 2;
    std::fs::rename(&aside, &file).unwrap();
    b.clock += history::COOLDOWN;
    let rows = b.rows.clone();
    b.prepare(140, 24).unwrap();
    assert_eq!(b.state.selected, 2);
    assert_eq!(b.main_selected, 2);
    assert_eq!(b.rows, rows);
    assert!(b.pages.growth.is_none());
    assert!(b.old.is_some());
    assert!(b.pages.live.is_some());
}

#[test]
fn growth_prepare_resets_main_to_first_page() {
    let (temp, mut b) = browser_by_size(1_005);
    let (id, file) = complete_scan(temp.path(), "20251201T000000Z-00000000", 900_000, 0);
    b.snapshots = vec![(id, file.clone(), 900_000, Some(0))];
    let aside = file.with_extension("aside");
    std::fs::rename(&file, &aside).unwrap();
    b.prepare(140, 24).unwrap();
    b.state.sort = state::Sort::Growth;
    b.load().unwrap();
    assert_eq!(b.rows.len(), PAGE);
    b.page(false).unwrap();
    assert_eq!(b.pages.offset, 1_000);
    assert_eq!(b.state.pane, Pane::Main);
    std::fs::rename(&aside, &file).unwrap();
    b.clock += history::COOLDOWN;
    b.prepare(140, 24).unwrap();
    assert_eq!(b.pages.offset, 0);
    assert_eq!(b.state.selected, 0);
    assert_eq!(b.main_selected, 0);
    assert_eq!(b.rows.len(), PAGE);
    assert_eq!(b.rows[0].0.id, 1005);
    assert_eq!(b.rows[0].1, Some(i128::from(1005) * 4096));
    assert_eq!(b.pages.growth.as_ref().map(Vec::len), Some(1_005));
    assert!(b.rows.iter().all(|(row, _)| row.name != b"."));
}

#[test]
fn growth_transition_keeps_off_main_subject() {
    let (temp, mut b) = browser(2);
    let (id, file) = complete_scan(temp.path(), "20251201T000000Z-00000000", 900_000, 4096);
    b.snapshots = vec![(id, file.clone(), 900_000, Some(4096))];
    let aside = file.with_extension("aside");
    std::fs::rename(&file, &aside).unwrap();
    b.prepare(140, 24).unwrap();
    b.state.sort = state::Sort::Growth;
    b.load().unwrap();
    b.state.selected = 0;
    b.pane(Pane::Info).unwrap();
    assert!(
        b.subject().0.path.as_bytes().ends_with(b"00000001"),
        "{:?}",
        b.subject().0.path
    );
    b.state.selected = 1;
    let selected = b.state.selected;
    std::fs::rename(&aside, &file).unwrap();
    b.clock += history::COOLDOWN;
    b.prepare(140, 24).unwrap();
    assert!(
        b.subject().0.path.as_bytes().ends_with(b"00000001"),
        "{:?}",
        b.subject().0.path
    );
    assert_eq!(b.state.selected, selected);
}

#[test]
fn picker_binds_predecessor_of_picked_snapshot() {
    let (temp, mut b) = browser(1);
    let (a_id, a_file) = complete_scan(temp.path(), "20251101T000000Z-00000000", 800_000, 0);
    let (b_id, b_file) = complete_scan(temp.path(), "20251201T000000Z-00000000", 900_000, 8_192);
    let c_id = b.snapshot.meta().scan_id.to_string();
    let c_started = b.snapshot.meta().started_at;
    let c_file = b.snapshot_file.clone();
    b.old = Some(Snapshot::open(&b_file).unwrap());
    b.snapshots = vec![
        (a_id, a_file, 800_000, Some(0)),
        (b_id.clone(), b_file, 900_000, Some(8_192)),
        (c_id, c_file, c_started, Some(4_096)),
    ];
    b.pane(Pane::Picker).unwrap();
    b.state.selected = 1;
    b.enter().unwrap();
    b.deltas(24, false).unwrap();
    let line = child_line(&render(&b, 120, 24));
    let against_a = crate::render::tree::signed_bytes(i128::from(8_192) - i128::from(0));
    assert_eq!(b.snapshot.meta().scan_id.to_string(), b_id);
    assert_eq!(against_a, "+8.00 KiB");
    assert!(line.contains(&against_a), "{line}");
}

#[test]
fn size_column_is_right_aligned_at_both_widths() {
    let (_temp, mut b) = browser(3);
    b.rows[0].0.t_disk = 72_241_886;
    for (width, height) in [(80, 24), (140, 40)] {
        let screen = render(&b, width, height);
        assert!(screen.contains("68.9 MiB"), "{screen}");
        let end = screen
            .lines()
            .find_map(|line| {
                (line.starts_with('│') && line.contains("68.9 MiB") && line.contains('/'))
                    .then(|| token_end_column(line, "68.9 MiB"))
            })
            .unwrap()
            .unwrap();
        let mut saw_peer = false;
        for line in screen
            .lines()
            .filter(|line| line.starts_with('│') && line.contains("4.00 KiB") && line.contains('/'))
        {
            if line.contains("68.9 MiB") {
                continue;
            }
            assert_eq!(token_end_column(line, "4.00 KiB"), Some(end), "{line}");
            saw_peer = true;
        }
        assert!(saw_peer, "{screen}");
        let own = if width >= 120 {
            screen.lines().find(|line| line.contains("./ (files here)"))
        } else {
            screen
                .lines()
                .find(|line| line.contains("0 B") && line.contains('.'))
        }
        .expect("own row");
        assert_eq!(token_end_column(own, "0 B"), Some(end), "{own}");
        if width >= 120 {
            assert!(screen.contains("0000000"), "{screen}");
        }
    }
}

#[test]
fn own_row_dashes_the_four_snapshot_cells() {
    let (_temp, b) = browser(3);
    let wide = render(&b, 160, 48);
    let own = wide
        .lines()
        .find(|line| line.contains("./ (files here)"))
        .unwrap();
    assert!(own.contains("12.0 KiB"), "{own}");
    assert!(own.matches(" -").count() >= 4, "{own}");
    let narrow = render(&b, 80, 24);
    assert!(!narrow.contains("./ (files here)"), "{narrow}");
    let own = narrow.lines().find(|line| line.contains("0 B")).unwrap();
    assert!(own.matches(" -").count() >= 4, "{own}");
}

#[test]
fn no_match_copy_depends_on_editing() {
    let (_temp, mut b) = browser(3);
    b.state.filter = "zzz".into();
    b.boundary(false).unwrap();
    assert!(b.rows.is_empty());
    assert_eq!(b.own_position(), usize::MAX);
    let idle = render(&b, 80, 24);
    assert!(
        idle.contains("no matches for \"zzz\" · Esc clears the filter"),
        "{idle}"
    );
    let idle_row = idle
        .lines()
        .position(|line| line.contains("no matches for"))
        .unwrap();
    assert_eq!(idle_row, 10, "centred in the table\n{idle}");
    b.state.editing = true;
    let editing = render(&b, 80, 24);
    assert!(
        editing.contains("no matches for \"zzz\" yet · Enter or Esc to finish"),
        "{editing}"
    );
    let editing_row = editing
        .lines()
        .position(|line| line.contains("no matches for"))
        .unwrap();
    assert_eq!(editing_row, 10, "centred in the table\n{editing}");
}

#[test]
fn info_and_help_do_not_use_the_table_marker() {
    let (_temp, mut b) = browser(1);
    b.pane(Pane::Help).unwrap();
    let help = render(&b, 80, 24);
    assert!(!help.contains('▶'), "{help}");
    b.theme = theme::Theme::select(true, false, None, Some("linux"), Some("C"));
    let ascii = render(&b, 80, 24);
    assert!(!ascii.contains("> "), "{ascii}");
    b.theme = theme::Theme::plain();
    b.pane(Pane::Info).unwrap();
    let mut lines = vec!["INFO-FIRST-LINE".to_string()];
    for index in 1..40 {
        lines.push(format!("INFO-LINE-{index:02}"));
    }
    b.text = lines.join("\n");
    b.state.selected = 30;
    let info = render(&b, 80, 24);
    assert!(!info.contains("INFO-FIRST-LINE"), "{info}");
    assert!(info.contains("INFO-LINE-30"), "{info}");
    assert!(!info.contains('▶'), "{info}");
}

fn buffer_of(b: &Browser, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(ratatui::backend::TestBackend::new(width, height)).unwrap();
    terminal.draw(|frame| draw::draw(frame, b)).unwrap();
    terminal.backend().buffer().clone()
}

fn row_text(buffer: &ratatui::buffer::Buffer, y: u16) -> String {
    (0..buffer.area.width)
        .map(|x| buffer[(x, y)].symbol())
        .collect()
}

fn row_containing(buffer: &ratatui::buffer::Buffer, needle: &str) -> u16 {
    (0..buffer.area.height)
        .find(|y| row_text(buffer, *y).contains(needle))
        .unwrap_or_else(|| panic!("missing {needle}"))
}

#[test]
fn rgb_frame_paints_navy_and_a_coral_rising_mark() {
    use ratatui::style::{Color, Modifier};
    let (_temp, mut b) = browser(3);
    b.theme = theme::Theme::select(
        true,
        false,
        Some("truecolor"),
        Some("xterm-256color"),
        Some("C.UTF-8"),
    );
    b.state.selected = 0;
    b.rows[1].1 = Some(4_096);
    let buffer = buffer_of(&b, 80, 24);
    let mut navy = false;
    let mut coral = false;
    let mut teal_marker = false;
    for cell in &buffer.content {
        if cell.bg == Color::Rgb(7, 21, 38) && cell.symbol() == " " {
            navy = true;
        }
        if cell.symbol() == "▲" && cell.fg == Color::Rgb(255, 123, 107) {
            coral = true;
        }
        if cell.symbol() == "▾" {
            assert_eq!(cell.fg, Color::Rgb(0x2e, 0xc4, 0xb6), "sort marker");
            teal_marker = true;
        }
    }
    assert!(teal_marker, "expected a teal sort marker");
    assert!(navy, "expected a blank navy cell");
    assert!(coral, "expected an unselected coral rising mark");
    let legend = row_containing(&buffer, "ebbing");
    let footer = row_containing(&buffer, "quit");
    let last = buffer.area.width - 1;
    assert_eq!(buffer[(last, legend)].bg, Color::Rgb(7, 21, 38));
    assert_eq!(buffer[(last, footer)].bg, Color::Rgb(7, 21, 38));
    let title = row_containing(&buffer, "depth");
    let title_cell = (0..buffer.area.width)
        .map(|x| &buffer[(x, title)])
        .find(|cell| cell.symbol() == "d")
        .expect("depth title");
    assert_eq!(title_cell.fg, Color::Rgb(0x2e, 0xc4, 0xb6));
    assert!(title_cell.modifier.contains(Modifier::BOLD));
    let border = &buffer[(0, title + 1)];
    assert_eq!(border.symbol(), "│");
    assert_eq!(border.fg, Color::Rgb(0x1d, 0x3d, 0x57));
    b.theme = theme::Theme::select(true, false, None, Some("xterm-256color"), Some("C.UTF-8"));
    let indexed = buffer_of(&b, 80, 24);
    let legend = row_containing(&indexed, "ebbing");
    let footer = row_containing(&indexed, "quit");
    assert_eq!(indexed[(last, legend)].bg, Color::Indexed(233));
    assert_eq!(indexed[(last, footer)].bg, Color::Indexed(233));
}

#[test]
fn main_selected_stays_on_the_depth_row_across_panes() {
    let (_temp, mut b) = browser(10);
    let config = tidemark_core::config::Config::default();
    for _ in 0..3 {
        assert!(!b.on_key(crossterm::event::KeyCode::Down, &config).unwrap());
    }
    assert_eq!(b.state.selected, 3);
    assert_eq!(b.main_selected, 3);
    assert_eq!(b.child_at(3).unwrap().0.id, 4);
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    assert_eq!(b.main_selected, 3);
    let info = render(&b, 140, 40);
    assert!(info.contains("strata · 00000004/"), "{info}");
    assert!(info.contains("soundings · 00000004/"), "{info}");
    b.on_key(crossterm::event::KeyCode::Char('/'), &config)
        .unwrap();
    assert!(b.state.editing);
    assert_eq!(b.state.selected, 3);
    assert_eq!(b.main_selected, 3);
    b.state.editing = false;
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Left, &config).unwrap();
    assert_eq!(b.state.pane, Pane::Main);
    assert_eq!(b.state.selected, 3);
    assert_eq!(b.main_selected, 3);
    assert_eq!(b.pages.offset, 0);
    let back = render(&b, 140, 40);
    assert!(back.contains("strata · 00000004/"), "{back}");
    assert!(!back.contains("strata · 00000001/"), "{back}");
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Esc, &config).unwrap();
    assert_eq!(b.state.selected, 0);
    assert_eq!(b.main_selected, 0);
    assert_eq!(b.pages.offset, 0);
    let escaped = render(&b, 140, 40);
    assert!(escaped.contains("strata · 00000001/"), "{escaped}");
    assert!(!escaped.contains("strata · 00000004/"), "{escaped}");
}

#[test]
fn files_pane_formats_mtime_and_keeps_the_retained_timestamp() {
    let (_temp, mut b) = browser(0);
    let temp = ::test_support::private_tempdir().unwrap();
    let mut rows = vec![b.dir.clone()];
    rows[0].own_files = 1;
    let meta = fixture::meta(rows[0].path.clone(), 1_000_000);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&rows).unwrap();
    writer
        .put_files(&[FileRow {
            id: 0,
            dir_id: 0,
            name: Some(RawName::new(b"root".to_vec()).unwrap()),
            ino: 2,
            disk: 4_096,
            apparent: 4_096,
            mtime: 100,
            nlink: 1,
        }])
        .unwrap();
    let file = writer.finish(&meta).unwrap();
    b.snapshot = Snapshot::open(&file).unwrap();
    b.dir = b.snapshot.root().unwrap();
    b.pane(Pane::Files).unwrap();
    let header = b.text.lines().next().unwrap();
    assert!(header.contains("SIZE"), "{header}");
    assert!(header.contains("MTIME"), "{header}");
    assert!(header.contains("PATH"), "{header}");
    assert!(b.text.contains("01-01 00:01"), "{}", b.text);
    assert!(!b.text.contains("1970-01-01 00:01:40Z"), "{}", b.text);
    b.close_pane();
    b.state.selected = b.own_position();
    b.enter().unwrap();
    assert!(b.text.contains("1970-01-01 00:01:40Z"), "{}", b.text);
}

#[test]
fn files_footer_names_the_snapshot_while_refresh_runs() {
    let (_temp, mut b) = browser(0);
    b.state.selected = b.own_position();
    b.enter().unwrap();
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    b.refresh_elapsed_ms = 2_500;
    let screen = render(&b, 160, 24);
    assert!(
        screen.contains("Snapshot files; does not include changes since snapshot"),
        "{screen}"
    );
    assert!(
        screen.contains("Live refresh running; Esc cancels"),
        "{screen}"
    );
    assert!(screen.contains('⠙'), "{screen}");
    assert!(screen.contains("2s"), "{screen}");
    drop(tx);
}

#[test]
fn poll_refresh_keeps_the_info_cursor_and_the_selected_directory() {
    let (_temp, mut b) = browser(10);
    let config = tidemark_core::config::Config::default();
    for _ in 0..3 {
        b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    }
    let selected_id = b.child_at(3).unwrap().0.id;
    let selected_path = b.child_at(3).unwrap().0.path.clone();
    assert_eq!(selected_id, 4);
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    assert_eq!(b.state.pane, Pane::Info);
    assert_eq!(b.state.selected, 2);
    assert_eq!(b.main_selected, 3);
    let mut children = b.rows.clone();
    let position = children
        .iter()
        .position(|(row, _)| row.id == selected_id)
        .unwrap();
    let chosen = children.remove(position);
    children.insert(0, chosen);
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    tx.send(Ok(LiveResult {
        root: b.dir.clone(),
        children,
        sort: b.state.sort,
        growth: Default::default(),
    }))
    .unwrap();
    b.poll_refresh().unwrap();
    assert!(b.refresh.is_none());
    assert_eq!(b.state.selected, 2, "info cursor");
    assert_eq!(b.child_at(b.main_selected).unwrap().0.path, selected_path);
    assert!(
        !b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
    let info = render(&b, 140, 40);
    assert!(info.contains("strata · 00000004/"), "{info}");
    assert!(info.contains("soundings · 00000004/"), "{info}");
    let gone: Vec<_> = b
        .pages
        .live
        .as_ref()
        .unwrap()
        .iter()
        .filter(|(row, _)| row.id != selected_id)
        .cloned()
        .collect();
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    tx.send(Ok(LiveResult {
        root: b.dir.clone(),
        children: gone,
        sort: b.state.sort,
        growth: Default::default(),
    }))
    .unwrap();
    b.poll_refresh().unwrap();
    assert_eq!(b.state.selected, 2, "info cursor after a missing directory");
    assert_eq!(b.main_selected, 0);
    assert!(
        b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
    let after = render(&b, 140, 40);
    assert!(!after.contains("strata · 00000004/"), "{after}");
}

#[test]
fn refresh_does_not_keep_a_sibling_that_inherited_the_deleted_id() {
    let (_temp, mut b) = browser(3);
    let config = tidemark_core::config::Config::default();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    let selected_id = b.child_at(1).unwrap().0.id;
    let selected_path = b.child_at(1).unwrap().0.path.clone();
    assert_eq!(selected_id, 2);
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    assert_eq!(b.state.selected, 1);
    let mut children = b.rows.clone();
    children.retain(|(row, _)| row.path != selected_path);
    let sibling_path = {
        let sibling = children
            .iter_mut()
            .find(|(row, _)| row.id == selected_id + 1)
            .unwrap();
        sibling.0.id = selected_id;
        sibling.0.path.clone()
    };
    assert!(children
        .iter()
        .any(|(row, _)| row.id == selected_id && row.path == sibling_path));
    assert!(children.iter().all(|(row, _)| row.path != selected_path));
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    tx.send(Ok(LiveResult {
        root: b.dir.clone(),
        children,
        sort: b.state.sort,
        growth: Default::default(),
    }))
    .unwrap();
    b.poll_refresh().unwrap();
    assert_eq!(b.state.selected, 1, "info cursor");
    assert_eq!(b.main_selected, 0);
    assert_ne!(b.child_at(b.main_selected).unwrap().0.path, sibling_path);
    assert!(
        b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
    let screen = render(&b, 140, 40);
    assert!(screen.contains("strata · 00000001/"), "{screen}");
    assert!(!screen.contains("strata · 00000003/"), "{screen}");
}

#[test]
fn refresh_follows_the_path_when_an_earlier_page_reuses_its_id() {
    let (_temp, mut b) = browser(1);
    let target = b.child_at(0).unwrap().0.clone();
    let mut children = Vec::with_capacity(1_001);
    for index in 0..1_000 {
        let id = if index == 0 {
            target.id
        } else {
            10_000 + index
        };
        children.push((
            fixture::row(
                id,
                Some(0),
                format!("/fixture/x{index:04}").as_bytes(),
                4_096,
            ),
            None,
        ));
    }
    let mut real = target.clone();
    real.id = 42;
    children.push((real, None));
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    tx.send(Ok(LiveResult {
        root: b.dir.clone(),
        children,
        sort: b.state.sort,
        growth: Default::default(),
    }))
    .unwrap();
    b.poll_refresh().unwrap();
    assert_eq!(b.pages.offset, 1_000);
    assert_eq!(b.child_at(b.main_selected).unwrap().0.path, target.path);
    assert_ne!(b.child_at(b.main_selected).unwrap().0.id, target.id);
    assert!(
        !b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
}

#[test]
fn files_message_shows_a_finished_and_a_failed_refresh() {
    let (_temp, mut b) = browser(1);
    b.pane(Pane::Files).unwrap();
    b.state.selected = 3;
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    tx.send(Ok(LiveResult {
        root: b.dir.clone(),
        children: b.rows.clone(),
        sort: b.state.sort,
        growth: Default::default(),
    }))
    .unwrap();
    b.poll_refresh().unwrap();
    assert!(b.refresh.is_none());
    assert_eq!(b.state.selected, 3, "files cursor");
    let done = render(&b, 160, 24);
    assert!(
        done.contains("Snapshot files; does not include changes since snapshot"),
        "{done}"
    );
    assert!(
        done.contains("live values; delta against snapshot"),
        "{done}"
    );
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    tx.send(Err(anyhow::anyhow!("boom refresh failed")))
        .unwrap();
    b.poll_refresh().unwrap();
    assert!(b.refresh.is_none());
    assert_eq!(b.state.selected, 3, "files cursor after a failure");
    let failed = render(&b, 160, 24);
    assert!(
        failed.contains("Snapshot files; does not include changes since snapshot"),
        "{failed}"
    );
    assert!(failed.contains("boom refresh failed"), "{failed}");
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    b.message = "Live refresh already running".into();
    let already = render(&b, 160, 24);
    assert!(
        already.contains("Live refresh already running"),
        "{already}"
    );
    assert!(
        already.contains("Live refresh running; Esc cancels"),
        "{already}"
    );
    assert!(
        already.contains("Snapshot files; does not include changes since snapshot"),
        "{already}"
    );
    drop(tx);
}

#[test]
fn files_message_keeps_the_result_ahead_of_the_disclaimer_at_80_columns() {
    let (_temp, mut b) = browser(1);
    b.pane(Pane::Files).unwrap();
    b.message = "live values; delta against snapshot; selected directory is gone".into();
    let gone = render(&b, 80, 24);
    let line = gone
        .lines()
        .find(|line| line.contains("selected directory is gone"))
        .unwrap_or_else(|| panic!("{gone}"));
    assert!(
        line.contains("live values; delta against snapshot"),
        "{line}"
    );
    if let Some(disclaimer) = line.find("Snapshot") {
        assert!(
            line.find("selected directory is gone").unwrap() < disclaimer,
            "{line}"
        );
    }
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    b.message.clear();
    let running = render(&b, 80, 24);
    let line = running
        .lines()
        .find(|line| line.contains("Esc cancels"))
        .unwrap_or_else(|| panic!("{running}"));
    if let Some(disclaimer) = line.find("Snapshot") {
        assert!(line.find("Esc cancels").unwrap() < disclaimer, "{line}");
    }
    drop(tx);
}

#[test]
fn sort_from_info_keeps_the_directory_and_the_info_cursor() {
    let (_temp, mut b) = browser(10);
    let config = tidemark_core::config::Config::default();
    for _ in 0..3 {
        b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    }
    let path = b.child_at(3).unwrap().0.path.clone();
    assert!(path.display_lossy().ends_with("00000004"), "{path:?}");
    let mut live = b.rows.clone();
    for (row, _) in &mut live {
        if row.path == path {
            row.t_reclaimable = 1;
        } else if row.id == 1 {
            row.t_reclaimable = 9_000_000;
        }
    }
    b.pages.live = Some(live);
    b.pages.live_order = Some(b.state.sort);
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    assert_eq!(b.state.pane, Pane::Info);
    assert_eq!(b.state.selected, 2);
    assert_eq!(b.main_selected, 3);
    b.on_key(crossterm::event::KeyCode::Char('s'), &config)
        .unwrap();
    assert_eq!(b.state.pane, Pane::Info);
    assert_eq!(b.state.sort, state::Sort::Reclaim);
    assert_eq!(b.state.selected, 2, "info cursor");
    assert_eq!(b.child_at(b.main_selected).unwrap().0.path, path);
    assert_ne!(b.main_selected, 3);
    let info = render(&b, 140, 40);
    assert!(info.contains("strata · 00000004/"), "{info}");
    assert!(info.contains("soundings · 00000004/"), "{info}");
    b.on_key(crossterm::event::KeyCode::Char('f'), &config)
        .unwrap();
    b.state.selected = 4;
    b.on_key(crossterm::event::KeyCode::Char('s'), &config)
        .unwrap();
    assert_eq!(b.state.pane, Pane::Files);
    assert_eq!(b.state.selected, 4, "files cursor");
    assert_eq!(b.child_at(b.main_selected).unwrap().0.path, path);
}

fn page_to_rank_at_least_1000(b: &mut Browser) {
    let config = tidemark_core::config::Config::default();
    b.on_key(crossterm::event::KeyCode::PageDown, &config)
        .unwrap();
    assert!(b.pages.offset >= 1_000, "offset {}", b.pages.offset);
}

#[test]
fn sort_from_info_keeps_a_child_ranked_past_the_first_snapshot_page() {
    let (_temp, mut b) = browser(2000);
    let config = tidemark_core::config::Config::default();
    page_to_rank_at_least_1000(&mut b);
    let path = b.child_at(b.main_selected).unwrap().0.path.clone();
    assert_ne!(b.child_at(0).unwrap().0.id, 1);
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    assert_eq!(b.state.selected, 1);
    b.on_key(crossterm::event::KeyCode::Char('s'), &config)
        .unwrap();
    assert_eq!(b.state.pane, Pane::Info);
    assert_eq!(b.state.sort, state::Sort::Reclaim);
    assert_eq!(b.state.selected, 1, "info cursor");
    assert!(b.pages.offset >= 1_000, "offset {}", b.pages.offset);
    assert_eq!(b.child_at(b.main_selected).unwrap().0.path, path);
    assert!(
        !b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
    while b.state.sort != state::Sort::Growth {
        b.on_key(crossterm::event::KeyCode::Char('s'), &config)
            .unwrap();
        assert_eq!(b.child_at(b.main_selected).unwrap().0.path, path);
        assert!(b.pages.offset >= 1_000, "offset {}", b.pages.offset);
        assert_eq!(b.state.selected, 1, "info cursor");
        assert!(
            !b.message.contains("selected directory is gone"),
            "{}",
            b.message
        );
    }
    assert!(b.pages.growth.is_some());
}

#[test]
fn sort_from_info_keeps_the_own_row_past_the_first_page() {
    let (_temp, mut b) = browser(2000);
    let config = tidemark_core::config::Config::default();
    b.on_key(crossterm::event::KeyCode::End, &config).unwrap();
    assert!(b.pages.offset >= 1_000, "offset {}", b.pages.offset);
    assert_eq!(b.main_selected, b.own_position());
    assert_ne!(b.own_position(), usize::MAX);
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    let info_cursor = b.state.selected;
    b.on_key(crossterm::event::KeyCode::Char('s'), &config)
        .unwrap();
    assert_eq!(b.state.sort, state::Sort::Reclaim);
    assert_eq!(b.state.pane, Pane::Info);
    assert_eq!(b.state.selected, info_cursor);
    assert_eq!(b.main_selected, b.own_position());
    assert_ne!(b.own_position(), usize::MAX);
    assert!(b.pages.offset >= 1_000, "offset {}", b.pages.offset);
    assert!(
        !b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
    // Files ties on t_files and then name, so `./` moves to the front. The later
    // sorts put it back after every child. Growth must read pages.growth, not name order.
    for _ in 0..3 {
        b.on_key(crossterm::event::KeyCode::Char('s'), &config)
            .unwrap();
        assert_eq!(b.state.selected, info_cursor);
        assert_eq!(b.main_selected, b.own_position());
        assert_ne!(b.own_position(), usize::MAX);
        assert!(
            !b.message.contains("selected directory is gone"),
            "{}",
            b.message
        );
    }
    assert_eq!(b.state.sort, state::Sort::Growth);
    assert!(b.pages.growth.is_some());
    assert!(b.pages.offset >= 1_000, "offset {}", b.pages.offset);
    assert_eq!(b.main_selected, b.own_position());
}

#[test]
fn sort_into_growth_keeps_a_child_the_growth_cache_ranks_past_1000() {
    let (_temp, mut b) = browser(2000);
    let config = tidemark_core::config::Config::default();
    let target = b.child_at(0).unwrap().0.path.clone();
    let mut all = Vec::new();
    loop {
        all.extend(b.rows.clone());
        if b.pages.offset + b.rows.len() >= b.pages.total {
            break;
        }
        let offset = b.pages.offset;
        b.page(false).unwrap();
        assert_ne!(b.pages.offset, offset);
    }
    assert_eq!(all.len(), 2_000);
    let index = all.iter().position(|(row, _)| row.path == target).unwrap();
    let chosen = all.remove(index);
    all.push(chosen);
    b.boundary(false).unwrap();
    b.pages.growth = Some(all);
    b.state.sort = state::Sort::Newest;
    b.state.selected = 0;
    b.main_selected = 0;
    assert_eq!(b.child_at(0).unwrap().0.path, target);
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    b.on_key(crossterm::event::KeyCode::Char('s'), &config)
        .unwrap();
    assert_eq!(b.state.sort, state::Sort::Growth);
    assert_eq!(b.state.pane, Pane::Info);
    assert_eq!(b.state.selected, 1, "info cursor");
    assert_eq!(b.pages.offset, 1_000);
    assert_eq!(b.child_at(b.main_selected).unwrap().0.path, target);
    assert_ne!(
        b.pages.growth.as_ref().unwrap().first().unwrap().0.path,
        target
    );
    assert!(
        !b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
}

#[test]
fn left_and_backspace_from_info_keep_the_deep_page() {
    let (_temp, mut b) = browser(2000);
    let config = tidemark_core::config::Config::default();
    page_to_rank_at_least_1000(&mut b);
    for _ in 0..4 {
        b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    }
    assert!(b.state.selected >= 4);
    let deep = b.child_at(b.main_selected).unwrap().0.path.clone();
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Char('s'), &config)
        .unwrap();
    assert!(b.pages.offset >= 1_000);
    assert!(b.main_selected > 0, "row {}", b.main_selected);
    assert_eq!(b.child_at(b.main_selected).unwrap().0.path, deep);
    let offset = b.pages.offset;
    let selected = b.main_selected;
    let cursor = b.pages.cursor.clone();
    let first = b.rows.first().unwrap().0.id;
    b.on_key(crossterm::event::KeyCode::Left, &config).unwrap();
    assert_eq!(b.state.pane, Pane::Main);
    assert_eq!(b.pages.offset, offset);
    assert_eq!(b.pages.cursor, cursor);
    assert_eq!(b.rows.first().unwrap().0.id, first);
    assert_eq!(b.state.selected, selected);
    assert_eq!(b.main_selected, selected);
    assert_eq!(b.child_at(b.state.selected).unwrap().0.path, deep);
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    b.on_key(crossterm::event::KeyCode::Backspace, &config)
        .unwrap();
    assert_eq!(b.state.pane, Pane::Main);
    assert_eq!(b.pages.offset, offset);
    assert_eq!(b.pages.cursor, cursor);
    assert_eq!(b.rows.first().unwrap().0.id, first);
    assert_eq!(b.state.selected, selected);
    assert_eq!(b.main_selected, selected);
    assert_eq!(b.child_at(b.state.selected).unwrap().0.path, deep);
}

#[test]
fn own_row_after_exactly_one_thousand_children_keeps_their_page() {
    // rank == filtered_len == 1000 pulls ./ back onto the full page. Offset 1000 is the empty page.
    assert_eq!(super::own_page_offset(1_000, 1_000), 0);
    assert_eq!(super::own_page_offset(2_000, 2_000), 1_000);
    // ./ after the first 1,000 of a longer list is an ordinary page boundary.
    assert_eq!(super::own_page_offset(1_000, 1_500), 1_000);
    assert_eq!(super::own_page_offset(999, 999), 0);
    assert_eq!(super::own_page_offset(0, 0), 0);
    let (_temp, mut b) = browser(1_000);
    let config = tidemark_core::config::Config::default();
    b.on_key(crossterm::event::KeyCode::End, &config).unwrap();
    assert_eq!(b.pages.offset, 0);
    assert_eq!(b.rows.len(), 1_000);
    assert_eq!(b.main_selected, b.own_position());
    assert_ne!(b.own_position(), usize::MAX);
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    b.on_key(crossterm::event::KeyCode::Char('s'), &config)
        .unwrap();
    assert_eq!(b.state.pane, Pane::Info);
    assert_eq!(b.pages.offset, 0, "empty trailing page");
    assert_eq!(b.rows.len(), 1_000);
    assert_eq!(b.main_selected, b.own_position());
    assert_ne!(b.own_position(), usize::MAX);
    assert!(
        !b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
}

#[test]
fn refresh_on_main_selects_the_own_row_and_the_child() {
    let config = tidemark_core::config::Config::default();
    let (_temp, mut b) = browser(3);
    for _ in 0..3 {
        b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    }
    assert_eq!(b.state.pane, Pane::Main);
    assert_eq!(b.state.selected, b.own_position());
    assert_ne!(b.state.selected, 0);
    let mut root = b.dir.clone();
    root.own_charged = 5_000;
    let mut children = b.rows.clone();
    children[0].0.t_disk = 9_000;
    children[1].0.t_disk = 8_000;
    children[2].0.t_disk = 1_000;
    install_refresh(&mut b, root, children);
    assert_eq!(b.state.pane, Pane::Main);
    assert_eq!(b.own_position(), 2);
    assert_eq!(b.state.selected, 2);
    assert_eq!(b.main_selected, 2);
    assert!(
        !b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );

    let (_temp, mut b) = browser(3);
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    assert_eq!(b.state.selected, 2);
    let path = b.child_at(2).unwrap().0.path.clone();
    let mut children = b.rows.clone();
    let chosen = children.remove(2);
    children.insert(1, chosen);
    children[0].0.t_disk = 9_000;
    children[1].0.t_disk = 8_000;
    children[2].0.t_disk = 7_000;
    let root = b.dir.clone();
    install_refresh(&mut b, root, children);
    let index = b.child_display_index(&path).unwrap();
    assert_eq!(index, 1);
    assert_eq!(b.state.pane, Pane::Main);
    assert_eq!(b.state.selected, index);
    assert_eq!(b.main_selected, index);
    assert_eq!(b.child_at(b.state.selected).unwrap().0.path, path);
    assert!(
        !b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
}

#[test]
fn enter_on_own_keeps_the_report_out_of_the_column_header() {
    let (_temp, mut b) = browser(1);
    let config = tidemark_core::config::Config::default();
    b.on_key(crossterm::event::KeyCode::End, &config).unwrap();
    assert_eq!(b.state.selected, b.own_position());
    b.on_key(crossterm::event::KeyCode::Enter, &config).unwrap();
    assert_eq!(b.state.pane, Pane::Files);
    let first = b.text.lines().next().unwrap();
    assert!(first.starts_with("Snapshot "), "{first}");
    assert!(!super::files_column_header(first), "{first}");
    assert!(b.column_header().is_none());
    let lines = b.text.lines().count();
    assert!(lines > 1, "{}", b.text);
    assert_eq!(b.count(), lines);
    assert!(b.text.contains("PATH  SIZE  APPARENT  MTIME"), "{}", b.text);
    assert!(
        b.text.contains("does not include changes since"),
        "{}",
        b.text
    );
    assert!(b.text.contains("charged to this directory"), "{}", b.text);
    let screen = render(&b, 120, 24);
    assert!(highlighted_line(&screen).contains("Snapshot"), "{screen}");
    assert!(screen.contains("PATH  SIZE  APPARENT  MTIME"), "{screen}");
    b.on_key(crossterm::event::KeyCode::End, &config).unwrap();
    assert_eq!(b.state.selected, lines - 1);
    let last = render(&b, 120, 24);
    assert!(
        highlighted_line(&last).contains("common ancestor"),
        "{last}"
    );
    assert!(!highlighted_line(&last).contains("Snapshot"), "{last}");
}

#[test]
fn load_ranked_page_follows_sort_direction_and_the_step_limit() {
    let (_temp, mut b) = browser_by_size(2_500);
    b.state.sort = state::Sort::Name;
    b.load_ranked_page(1_000).unwrap();
    assert_eq!(b.pages.offset, 1_000);
    assert_eq!(b.rows.first().unwrap().0.id, 1_001);
    b.state.sort = state::Sort::Size;
    b.load_ranked_page(1_000).unwrap();
    assert_eq!(b.pages.offset, 1_000);
    assert_eq!(b.rows.first().unwrap().0.id, 1_500);

    let deep = b
        .snapshot
        .dir_by_path(&RawPath::new(b"/fixture/00002200".to_vec()).unwrap())
        .unwrap()
        .unwrap();
    b.state.sort = state::Sort::Name;
    b.pages.total = 0;
    b.load_ranked_page(2_000).unwrap();
    assert_eq!(b.pages.offset, 1_000, "step limit stops the walk");
    assert!(b.rows.iter().all(|row| row.0.id != deep.id));
    assert!(b.child_display_index(&deep.path).is_none());
    b.pages.total = 0;
    b.state.pane = Pane::Main;
    b.state.selected = 7;
    b.main_selected = 7;
    let missing = b.settle_child_target(&deep.path).unwrap();
    assert!(missing);
    assert_eq!(b.state.selected, 0);
    assert_eq!(b.main_selected, 0);
}

#[test]
fn end_and_arrows_on_an_empty_list_stay_at_zero() {
    let (_temp, mut b) = browser(0);
    let config = tidemark_core::config::Config::default();
    b.on_key(crossterm::event::KeyCode::Char('l'), &config)
        .unwrap();
    assert_eq!(b.count(), 0);
    b.state.selected = 4;
    b.on_key(crossterm::event::KeyCode::End, &config).unwrap();
    assert_eq!(b.state.selected, 0);
    b.state.selected = 4;
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    assert_eq!(b.state.selected, 0);
    b.state.selected = 4;
    b.on_key(crossterm::event::KeyCode::Up, &config).unwrap();
    assert_eq!(b.state.selected, 0);
    b.on_key(crossterm::event::KeyCode::Char('f'), &config)
        .unwrap();
    assert_eq!(b.count(), 0, "{}", b.text);
    b.state.selected = 4;
    b.on_key(crossterm::event::KeyCode::End, &config).unwrap();
    assert_eq!(b.state.selected, 0);
    b.state.selected = 4;
    b.on_key(crossterm::event::KeyCode::PageDown, &config)
        .unwrap();
    assert_eq!(b.state.selected, 0);
}

#[test]
fn depth_title_keeps_the_filter_when_the_path_is_long() {
    let (_temp, mut b) = browser(1);
    b.dir.path = RawPath::new(format!("/{}", "p".repeat(180)).into_bytes()).unwrap();
    b.state.filter = "zzz".into();
    let title = draw::depth_title(&b, 80);
    let text: String = title
        .spans
        .iter()
        .map(|span| span.content.as_ref())
        .collect();
    let cols: usize = title
        .spans
        .iter()
        .map(|span| crate::render::table::display_width(span.content.as_ref()))
        .sum();
    assert!(cols <= 80, "{cols} {text}");
    assert!(text.contains("filter \"zzz\""), "{text}");
    assert!(text.contains("sort size"), "{text}");
    assert!(text.contains("..."), "{text}");
    let screen = render(&b, 80, 24);
    let line = screen
        .lines()
        .find(|line| line.contains("depth") && line.contains("sort "))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(line.contains("filter \"zzz\""), "{line}");
}

#[test]
fn esc_from_info_during_live_refresh_returns_to_the_first_child() {
    let (_temp, mut b) = browser(1);
    let config = tidemark_core::config::Config::default();
    let target = b.child_at(0).unwrap().0.clone();
    let mut children = Vec::with_capacity(1_001);
    for index in 0..1_000 {
        children.push((
            fixture::row(
                10_000 + index,
                Some(0),
                format!("/fixture/x{index:04}").as_bytes(),
                8_192,
            ),
            None,
        ));
    }
    children.push((target.clone(), None));
    b.pages.live = Some(children);
    b.pages.live_order = Some(b.state.sort);
    b.state.filter = String::from_utf8(target.name.clone()).unwrap();
    b.boundary(false).unwrap();
    assert_eq!(b.rows.len(), 1);
    assert_eq!(b.pages.offset, 0);
    assert_eq!(b.child_at(0).unwrap().0.path, target.path);
    b.main_selected = 0;
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    let (tx, rx) = crossbeam_channel::bounded(1);
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    b.refresh = Some(Refresh {
        cancel: cancel.clone(),
        rx,
        worker: None,
        started: std::time::Instant::now(),
    });
    b.on_key(crossterm::event::KeyCode::Esc, &config).unwrap();
    assert!(cancel.load(std::sync::atomic::Ordering::Acquire));
    assert_eq!(b.state.pane, Pane::Main);
    assert!(b.state.filter.is_empty());
    assert_eq!(b.pages.offset, 0);
    assert_eq!(b.state.selected, 0);
    assert_eq!(b.main_selected, 0);
    let first = &b.child_at(0).unwrap().0.path;
    assert_ne!(first, &target.path);
    assert!(
        first.display_lossy().ends_with("x0000"),
        "{first:?} offset {}",
        b.pages.offset
    );
    drop(tx);
}

#[test]
fn sort_from_info_reports_a_directory_that_is_really_gone() {
    let (_temp, mut b) = browser(4);
    let config = tidemark_core::config::Config::default();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    let path = b.child_at(1).unwrap().0.path.clone();
    b.on_key(crossterm::event::KeyCode::Char('i'), &config)
        .unwrap();
    let mut children = b.rows.clone();
    children.retain(|(row, _)| row.path != path);
    b.pages.live = Some(children);
    b.pages.live_order = Some(b.state.sort);
    b.on_key(crossterm::event::KeyCode::Char('s'), &config)
        .unwrap();
    assert_eq!(b.state.pane, Pane::Info);
    assert_eq!(b.state.sort, state::Sort::Reclaim);
    assert_eq!(b.main_selected, 0);
    assert_ne!(
        b.child_at(b.main_selected).map(|(row, _)| row.path.clone()),
        Some(path)
    );
    assert!(
        b.message.contains("selected directory is gone"),
        "{}",
        b.message
    );
}

#[test]
fn header_shortens_the_path_to_the_leftover_budget() {
    let glyphs = theme::Theme::plain().glyphs;
    let path = format!("/{}", "p".repeat(180));
    let identity = "20260101T000000Z-00000000 · walk · 1m 39s ago · steady";
    let right = "root high-water 68.9 MiB";
    let (brand, shown) = draw::fit_header(120, identity, right, &path, glyphs);
    assert!(!brand);
    let shown = shown.expect("path kept");
    let cols = draw::header_cols(&shown);
    assert!(cols > 12, "{shown} is {cols} columns");
    let fixed = draw::header_cols(identity) + 1 + draw::header_cols(right);
    assert!(cols + 2 + fixed <= 120, "cols {cols} fixed {fixed}");
    let compact_right = "hw 68.9 MiB";
    let (narrow_brand, narrow) = draw::fit_header(80, identity, compact_right, &path, glyphs);
    assert!(!narrow_brand);
    let narrow = narrow.expect("short high-water label leaves room for the path");
    let fixed = draw::header_cols(identity) + 1 + draw::header_cols(compact_right);
    assert!(draw::header_cols(&narrow) >= 8);
    assert!(draw::header_cols(&narrow) + 2 + fixed <= 80);

    let (_temp, mut b) = browser(1);
    b.dir.path = RawPath::new(path.into_bytes()).unwrap();
    b.snapshots
        .push((String::new(), Default::default(), 0, Some(72_246_886)));
    let screen = render(&b, 80, 24);
    let header = screen.lines().next().unwrap();
    assert!(header.contains("hw 68.9 MiB"), "{header}");
    assert!(header.starts_with("/p"), "{header}");
    assert!(!header.contains("root high-water"), "{header}");
}

#[test]
fn pane_titles_stay_inside_the_width_and_strata_spell_one_age() {
    let (_temp, mut b) = browser(1);
    b.state.filter = "n".repeat(80);
    for width in [12usize, 20, 40, 80] {
        let depth = draw::depth_title(&b, width);
        let cols: usize = depth
            .spans
            .iter()
            .map(|span| crate::render::table::display_width(span.content.as_ref()))
            .sum();
        assert!(cols <= width, "depth {cols} > {width}: {depth:?}");
        for pane in [Pane::Help, Pane::Files, Pane::Picker] {
            b.state.pane = pane;
            let title = draw::list_title(&b, width);
            let cols: usize = title
                .spans
                .iter()
                .map(|span| crate::render::table::display_width(span.content.as_ref()))
                .sum();
            assert!(cols <= width, "{pane:?} {cols} > {width}: {title:?}");
        }
    }
    let long = "n".repeat(80);
    for prefix in ["strata", "soundings"] {
        let heading = draw::panel_heading(prefix, "·", &long, 40);
        let cols = crate::render::table::display_width(&heading);
        assert!(cols <= 38, "{prefix} {cols} > 38: {heading}");
    }
    assert_eq!(
        draw::panel_heading("strata", "·", "/fixture (whole dir)", 40),
        "strata · /fixture (whole dir)"
    );
    b.state.filter.clear();
    b.state.pane = Pane::Main;
    let unicode = render(&b, 140, 40);
    assert!(unicode.contains("≥365d"), "{unicode}");
    assert!(!unicode.contains(">=365d"), "{unicode}");
    b.theme = theme::Theme::select(false, false, None, Some("linux"), Some("C"));
    let ascii = render(&b, 140, 40);
    assert!(ascii.contains(">=365d"), "{ascii}");
    assert!(!ascii.contains("≥365d"), "{ascii}");
}

#[test]
fn strata_sizes_share_a_column_and_soundings_fit_at_80() {
    let (_temp, mut b) = browser(3);
    let wide = render(&b, 140, 40);
    let full = wide.lines().find(|line| line.contains("<1d")).unwrap();
    let empty = wide.lines().find(|line| line.contains("1-7d")).unwrap();
    assert_eq!(
        full.find("4.00 KiB"),
        empty.find("0 B"),
        "full {full}\nempty {empty}"
    );
    b.rows[0].0.t_reclaimable = b.rows[0].0.t_disk;
    b.main_selected = 0;
    let narrow = render(&b, 80, 30);
    let line = narrow
        .lines()
        .find(|line| line.contains("reclaimable if removed"))
        .unwrap();
    assert!(line.contains("reclaimable if removed 100%"), "{line}");
    assert!(line.ends_with('│'), "{line}");
}

fn highlighted_line(screen: &str) -> &str {
    screen
        .lines()
        .find(|line| line.contains('▶'))
        .unwrap_or_else(|| panic!("no highlighted row\n{screen}"))
}

#[test]
fn enter_on_the_second_highlighted_link_opens_that_set() {
    let temp = ::test_support::private_tempdir().unwrap();
    let root = fixture::row(0, None, b"/fixture", 24_576);
    let meta = fixture::meta(root.path.clone(), 1_000_000);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&[root]).unwrap();
    writer
        .put_link_sets(&[
            LinkSetRow {
                ino: 303,
                nlink: 1,
                found: 1,
                complete: true,
                disk: 12_288,
                apparent: 12_288,
                mtime: 100,
                lca_dir_id: 0,
            },
            LinkSetRow {
                ino: 202,
                nlink: 1,
                found: 1,
                complete: true,
                disk: 8_192,
                apparent: 8_192,
                mtime: 100,
                lca_dir_id: 0,
            },
            LinkSetRow {
                ino: 101,
                nlink: 1,
                found: 1,
                complete: true,
                disk: 4_096,
                apparent: 4_096,
                mtime: 100,
                lca_dir_id: 0,
            },
        ])
        .unwrap();
    let file = writer.finish(&meta).unwrap();
    let (_temp, mut b) = browser(0);
    b.snapshot = Snapshot::open(&file).unwrap();
    b.snapshot_file = file;
    b.dir = b.snapshot.root().unwrap();
    let config = tidemark_core::config::Config::default();
    b.on_key(crossterm::event::KeyCode::Char('l'), &config)
        .unwrap();
    assert_eq!(b.links, vec![303, 202, 101]);
    assert_eq!(b.count(), b.links.len());
    let screen = render(&b, 120, 24);
    let header = screen
        .lines()
        .find(|line| line.contains("INSTANCES"))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(!header.contains('▶'), "header is a list row\n{header}");
    assert!(highlighted_line(&screen).contains("303"), "{screen}");
    b.on_key(crossterm::event::KeyCode::End, &config).unwrap();
    assert_eq!(b.state.selected, b.links.len() - 1);
    let last = render(&b, 120, 24);
    assert!(highlighted_line(&last).contains("101"), "{last}");
    assert!(!highlighted_line(&last).contains("303"), "{last}");
    b.on_key(crossterm::event::KeyCode::Home, &config).unwrap();
    b.on_key(crossterm::event::KeyCode::Down, &config).unwrap();
    assert_eq!(b.state.selected, 1);
    let second = render(&b, 120, 24);
    let row = highlighted_line(&second);
    assert!(row.contains("202"), "{row}");
    assert!(!row.contains("303") && !row.contains("101"), "{row}");
    b.on_key(crossterm::event::KeyCode::Enter, &config).unwrap();
    assert_eq!(b.state.pane, Pane::Instances);
    assert_eq!(b.pages.inode, Some(202));
}

#[test]
fn files_column_header_is_not_a_selectable_row() {
    let (_temp, mut b) = browser(0);
    let temp = ::test_support::private_tempdir().unwrap();
    let mut rows = vec![b.dir.clone()];
    rows[0].own_files = 2;
    let meta = fixture::meta(rows[0].path.clone(), 1_000_000);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&rows).unwrap();
    writer
        .put_files(&[
            FileRow {
                id: 0,
                dir_id: 0,
                name: Some(RawName::new(b"old".to_vec()).unwrap()),
                ino: 2,
                disk: 0,
                apparent: 0,
                mtime: 100,
                nlink: 1,
            },
            FileRow {
                id: 1,
                dir_id: 0,
                name: Some(RawName::new(b"huge".to_vec()).unwrap()),
                ino: 3,
                disk: 72_241_886,
                apparent: 72_241_886,
                mtime: 100,
                nlink: 1,
            },
        ])
        .unwrap();
    let file = writer.finish(&meta).unwrap();
    b.snapshot = Snapshot::open(&file).unwrap();
    b.dir = b.snapshot.root().unwrap();
    let config = tidemark_core::config::Config::default();
    b.on_key(crossterm::event::KeyCode::Char('f'), &config)
        .unwrap();
    let paths: Vec<String> = b
        .text
        .lines()
        .skip(1)
        .map(|line| line.rsplit_once("  ").unwrap().1.to_string())
        .collect();
    assert_eq!(paths.len(), 2, "{}", b.text);
    assert_eq!(b.count(), paths.len());
    let screen = render(&b, 120, 24);
    let header = screen
        .lines()
        .find(|line| line.contains("MTIME") && line.contains("PATH"))
        .unwrap_or_else(|| panic!("{screen}"));
    assert!(!header.contains('▶'), "header is a list row\n{header}");
    assert!(highlighted_line(&screen).contains(&paths[0]));
    b.on_key(crossterm::event::KeyCode::End, &config).unwrap();
    assert_eq!(b.state.selected, 1);
    let last = render(&b, 120, 24);
    assert!(highlighted_line(&last).contains(&paths[1]), "{last}");
    assert!(!highlighted_line(&last).contains("MTIME"), "{last}");
}

#[test]
fn picker_dates_pad_so_the_total_column_lines_up() {
    let (_temp, mut b) = browser(1);
    let now = 1_781_524_800;
    b.clock = now;
    let file = b.snapshot_file.clone();
    b.snapshots = vec![
        ("newer-scan".into(), file.clone(), now, Some(4_096)),
        ("older-scan".into(), file, 0, Some(8_192)),
    ];
    b.pane(Pane::Picker).unwrap();
    let screen = render(&b, 120, 24);
    let newer = screen
        .lines()
        .find(|line| line.contains("newer-scan"))
        .unwrap_or_else(|| panic!("{screen}"));
    let older = screen
        .lines()
        .find(|line| line.contains("older-scan"))
        .unwrap_or_else(|| panic!("{screen}"));
    let newer_at = newer.find("4.00 KiB").unwrap() - newer.find("newer-scan").unwrap();
    let older_at = older.find("8.00 KiB").unwrap() - older.find("older-scan").unwrap();
    assert_eq!(newer_at, older_at, "newer {newer}\nolder {older}");
}

#[test]
fn links_rows_line_up_under_the_header() {
    let temp = ::test_support::private_tempdir().unwrap();
    let root = fixture::row(0, None, b"/fixture", 8_388_608);
    let meta = fixture::meta(root.path.clone(), 1_000_000);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&[root]).unwrap();
    writer
        .put_link_sets(&[
            LinkSetRow {
                ino: 100,
                nlink: 2,
                found: 2,
                complete: true,
                disk: 4_096,
                apparent: 4_096,
                mtime: 100,
                lca_dir_id: 0,
            },
            LinkSetRow {
                ino: 7,
                nlink: 1,
                found: 1,
                complete: true,
                disk: 0,
                apparent: 0,
                mtime: 100,
                lca_dir_id: 0,
            },
        ])
        .unwrap();
    writer
        .put_link_instances(&[
            LinkInstanceRow {
                ino: 100,
                dir_id: 0,
                name: Some(RawName::new(b"left".to_vec()).unwrap()),
            },
            LinkInstanceRow {
                ino: 100,
                dir_id: 0,
                name: Some(RawName::new(b"right".to_vec()).unwrap()),
            },
            LinkInstanceRow {
                ino: 7,
                dir_id: 0,
                name: Some(RawName::new(b"only".to_vec()).unwrap()),
            },
        ])
        .unwrap();
    let file = writer.finish(&meta).unwrap();
    let (_temp, mut b) = browser(0);
    b.snapshot = Snapshot::open(&file).unwrap();
    b.snapshot_file = file;
    b.dir = b.snapshot.root().unwrap();
    b.pane(Pane::Links).unwrap();
    let lines: Vec<&str> = b.text.lines().collect();
    let header = lines[0];
    assert!(header.starts_with("INO"), "{header}");
    let kib = lines.iter().find(|line| line.contains("4.00 KiB")).unwrap();
    let zero = lines.iter().find(|line| line.contains("0 B")).unwrap();
    let size_end = header.find("SIZE").unwrap() + "SIZE".len();
    assert_eq!(
        kib.find("4.00 KiB").unwrap() + "4.00 KiB".len(),
        size_end,
        "{kib}"
    );
    assert_eq!(zero.find("0 B").unwrap() + "0 B".len(), size_end, "{zero}");
    let instances = header.find("INSTANCES").unwrap();
    assert_eq!(kib.find('⚓'), Some(instances), "{kib}");
    assert_eq!(kib.matches('⚓').count(), 1, "{kib}");
    assert_eq!(kib.matches("2").count(), 1, "{kib}");
    assert!(!kib.contains("instance"), "{kib}");
}

#[test]
fn files_rows_line_up_under_the_header() {
    let (_temp, mut b) = browser(0);
    let temp = ::test_support::private_tempdir().unwrap();
    let mut rows = vec![b.dir.clone()];
    rows[0].own_files = 2;
    let meta = fixture::meta(rows[0].path.clone(), 1_000_000);
    let mut writer = tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
    writer.put_dirs(&rows).unwrap();
    writer
        .put_files(&[
            FileRow {
                id: 0,
                dir_id: 0,
                name: Some(RawName::new(b"old".to_vec()).unwrap()),
                ino: 2,
                disk: 0,
                apparent: 0,
                mtime: 100,
                nlink: 1,
            },
            FileRow {
                id: 1,
                dir_id: 0,
                name: Some(RawName::new(b"huge".to_vec()).unwrap()),
                ino: 3,
                disk: 72_241_886,
                apparent: 72_241_886,
                mtime: 1_800_000_000,
                nlink: 1,
            },
        ])
        .unwrap();
    let file = writer.finish(&meta).unwrap();
    b.snapshot = Snapshot::open(&file).unwrap();
    b.dir = b.snapshot.root().unwrap();
    b.clock = 1_800_000_000;
    b.pane(Pane::Files).unwrap();
    let older = draw::fmt_when(100, b.clock);
    let newer = draw::fmt_when(1_800_000_000, b.clock);
    assert_ne!(older.chars().count(), newer.chars().count());
    let lines: Vec<&str> = b.text.lines().collect();
    let header = lines[0];
    let zero = lines
        .iter()
        .copied()
        .find(|line| line.contains("0 B"))
        .unwrap();
    let huge = lines
        .iter()
        .copied()
        .find(|line| line.contains("68.9 MiB"))
        .unwrap();
    let size_end = header.find("SIZE").unwrap() + "SIZE".len();
    assert_eq!(zero.find("0 B").unwrap() + "0 B".len(), size_end, "{zero}");
    assert_eq!(
        huge.find("68.9 MiB").unwrap() + "68.9 MiB".len(),
        size_end,
        "{huge}"
    );
    assert_eq!(header.find("MTIME"), zero.find(&older), "{header}\n{zero}");
    assert_eq!(zero.find(&older), huge.find(&newer), "{zero}\n{huge}");
    assert_eq!(header.find("PATH"), zero.find('/'), "{header}\n{zero}");
    assert_eq!(zero.find('/'), huge.find('/'), "{zero}\n{huge}");
}

fn remedy_catalog_case(broad: bool, count: i64) -> String {
    let state = ::test_support::private_tempdir().unwrap();
    let live = ::test_support::private_tempdir().unwrap();
    let subject = live.path().join("data/new");
    std::fs::create_dir_all(&subject).unwrap();
    let narrow = RawPath::from_path(&live.path().join("data")).unwrap();
    let path = RawPath::from_path(&subject).unwrap();
    let mut ids = Vec::new();
    for index in 0..count {
        let root = if broad && index == 1 {
            RawPath::new(b"/".to_vec()).unwrap()
        } else {
            narrow.clone()
        };
        let mut meta = fixture::meta(root.clone(), 1_000_000 + index * 100);
        meta.scan_id =
            serde_json::from_str(&format!("\"20260101T000000Z-{:08}\"", index + 1)).unwrap();
        let dir = state
            .path()
            .join("snapshots")
            .join(tidemark_store::catalog::root_slug(&root));
        let mut rows = vec![fixture::row(0, None, root.as_bytes(), 0)];
        rows[0].subtree_end = u32::from(index == 1);
        if index == 1 {
            rows.push(fixture::row(1, Some(0), path.as_bytes(), 0));
        }
        let mut writer = tidemark_store::writer::SnapshotWriter::create(&dir, &meta).unwrap();
        writer.put_dirs(&rows).unwrap();
        writer.finish(&meta).unwrap();
        ids.push(meta.scan_id.to_string());
    }
    let catalog = tidemark_store::catalog::Catalog::open(state.path(), true).unwrap();
    catalog.rebuild_from_files().unwrap();
    let mut config = tidemark_core::config::Config::default();
    config.general.state_dir = state.path().to_path_buf();
    let context = crate::Context {
        config,
        config_path: "/unused".into(),
        json: false,
        color: false,
    };
    let (_, default) = crate::cmd::snapshots::select(&context, Some(&subject), None).unwrap();
    assert_eq!(default.meta().scan_id.to_string(), ids[count as usize - 1]);
    assert!(default.dir_by_path(&path).unwrap().is_none());
    let (_, old) = crate::cmd::snapshots::select(&context, Some(&subject), Some(&ids[0])).unwrap();
    crate::cmd::snapshots::missing_path(&context, &path, &old).to_string()
}

#[test]
fn post_tui2_remedy_requires_working_default() {
    for broad in [false, true] {
        let message = remedy_catalog_case(broad, 3);
        assert!(
            message.contains("use --snapshot 20260101T000000Z-00000002"),
            "{message}"
        );
        assert!(!message.contains("drop --snapshot"), "{message}");
    }
}

#[test]
fn post_tui2_remedy_search_is_capped() {
    let message = remedy_catalog_case(false, 25);
    assert!(message.contains("run: tidemark scan"), "{message}");
    assert!(!message.contains("contains this path"), "{message}");
}

#[test]
fn post_tuititle_full_legend_at_80_columns() {
    let mut failures = Vec::new();
    for sort in [
        state::Sort::Size,
        state::Sort::Reclaim,
        state::Sort::Files,
        state::Sort::Newest,
        state::Sort::Growth,
        state::Sort::Name,
    ] {
        for live in [false, true] {
            for filter in [String::new(), "x".repeat(60)] {
                let (_temp, mut b) = browser(3);
                b.state.sort = sort;
                if live {
                    let root = b.dir.clone();
                    let children = b.rows.clone();
                    install_refresh(&mut b, root, children);
                }
                b.state.filter = filter.clone();
                let screen = render(&b, 80, 24);
                // Tide's narrow legend is its own row. The anchor is two columns,
                // so the screen helper inserts one continuation space after it.
                let legend =
                    "⚓  linked outside  ⚠ errors  ▲ rising  ▼ ebbing  LEVEL share of charged";
                if screen.matches(legend).count() != 1 {
                    failures.push(format!(
                        "{sort:?} live={live} filter={}\n{screen}",
                        filter.len()
                    ));
                }
            }
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn post_tuititle_filter_budget_preserves_status_and_wide_text() {
    for sort in [
        state::Sort::Size,
        state::Sort::Reclaim,
        state::Sort::Files,
        state::Sort::Newest,
        state::Sort::Growth,
        state::Sort::Name,
    ] {
        for live in [false, true] {
            for filter in ["x".repeat(60), "界\\\n".repeat(10)] {
                let (_temp, mut b) = browser(3);
                b.state.sort = sort;
                if live {
                    let root = b.dir.clone();
                    let children = b.rows.clone();
                    install_refresh(&mut b, root, children);
                }
                b.state.filter = filter.clone();
                for editing in [false, true] {
                    b.state.editing = editing;
                    for width in [80, 160] {
                        let screen = render(&b, width, 24);
                        let title = screen
                            .lines()
                            .find(|line| line.contains("depth") && line.contains("sort "))
                            .unwrap_or_else(|| panic!("no depth title\n{screen}"));
                        assert!(
                            title.contains(&format!("sort {}", crate::render::enum_label(sort))),
                            "{title}"
                        );
                        let live_mark = format!(" {} live", b.theme.glyphs.sep);
                        assert_eq!(title.contains(&live_mark), live, "{title}");
                        if width == 80 {
                            assert!(title.contains("..."), "{title}");
                        } else {
                            // The screen helper serializes the blank continuation cell
                            // after each double-width character as an extra space.
                            let displayed = super::escape(filter.as_bytes()).replace('界', "界 ");
                            assert!(title.contains(&displayed), "{title}");
                        }
                        let legend = if width >= 110 {
                            "⚓  anchored by hard links outside   ⚠ scan errors   ▲ rising   ▼ ebbing   LEVEL = share of charged bytes"
                        } else {
                            "⚓  linked outside  ⚠ errors  ▲ rising  ▼ ebbing  LEVEL share of charged"
                        };
                        assert_eq!(screen.matches(legend).count(), 1, "{screen}");
                        assert_eq!(b.state.filter, filter);
                    }
                }
            }
        }
    }
}
