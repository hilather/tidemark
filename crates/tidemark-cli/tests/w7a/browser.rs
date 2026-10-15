//! Owned by: W7-A
//! In-module browser tests exercising the real store and drawing path.

#[path = "fixture.rs"]
mod fixture;
use super::*;
use ratatui::{backend::TestBackend, Terminal};
use tidemark_core::{
    path::{RawName, RawPath},
    snapshot::{FileRow, LinkInstanceRow, LinkSetRow},
};

fn browser(count: u32) -> (tempfile::TempDir, Browser) {
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
        dir,
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        now: 1_000_100,
        pages: Pages::default(),
        refresh: None,
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
        assert!(first.contains("degraded=false"));
        insta::with_settings!({snapshot_path => "snapshots"}, { insta::assert_snapshot!(format!("main_{width}_{height}"),first); });
    }
}
#[test]
fn too_small() {
    let (_temp, b) = browser(1);
    assert!(render(&b, 79, 23).contains("terminal too small"));
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
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        now: 1_000_100,
        pages: Pages::default(),
        refresh: None,
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
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        now: 1_000_100,
        pages: Pages::default(),
        refresh: None,
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
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        now: 1_000_100,
        pages: Pages::default(),
        refresh: None,
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
    assert!(b.text.contains("own files as charged"));
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
        assert!(text.contains("walk age"), "{text}");
        assert!(text.contains("main | size | filter"), "{text}");
        assert!(!text.contains("Walk") && !text.contains("Main | Size"));
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
        assert!(!screen.contains("live values; delta"));
    }
}

fn install_refresh(b: &mut Browser, root: DirRow, children: Vec<(DirRow, Option<i128>)>) {
    let (tx, rx) = crossbeam_channel::bounded(1);
    b.refresh = Some(Refresh {
        cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
        rx,
        worker: None,
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
fn live_refresh_from_later_page_restores_own_row() {
    let (_temp, mut b) = browser(2500);
    b.state.sort = state::Sort::Name;
    b.boundary(false).unwrap();
    b.page(false).unwrap();
    assert!(b.pages.cursor.is_some());
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
    assert_eq!(b.pages.offset, 0);
    assert_eq!(b.own_position(), 0);
    assert!(b.pages.cursor.is_none());
    assert!(!b.pages.reverse);
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

fn selected_missing_path(partial: bool) -> String {
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
    row.subtree_end = 1;
    let mut writer = tidemark_store::writer::SnapshotWriter::create(&dir, &newer).unwrap();
    writer
        .put_dirs(&[row, fixture::row(1, Some(0), path.as_bytes(), 0)])
        .unwrap();
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
    assert!(latest.dir_by_path(&path).unwrap().is_some());
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
    let message = selected_missing_path(false);
    assert!(message.contains("selected complete snapshot"));
    assert!(!message.contains("no complete snapshot"), "{message}");
}

#[test]
fn missing_path_partial_snapshot_is_labelled_partial() {
    let message = selected_missing_path(true);
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
