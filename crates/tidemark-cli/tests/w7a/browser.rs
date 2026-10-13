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
    let temp = tempfile::tempdir().unwrap();
    let mut rows = vec![fixture::row(0, None, b"/fixture", u64::from(count) * 4096)];
    rows[0].subtree_end = count;
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
        old: None,
        dir,
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        now: 1_000_100,
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
        let Some(next) = b.state.next else {
            break;
        };
        b.state.page_start = next;
        b.load().unwrap();
    }
    ids.sort_unstable();
    assert_eq!(ids, (1..=2500).collect::<Vec<_>>());
}
#[test]
fn descend_ascend_filter_sort_and_bounds() {
    let (_temp, mut b) = browser(10);
    b.state.filter = "00000005".into();
    assert_eq!(b.visible().len(), 1);
    b.enter().unwrap();
    assert_eq!(b.dir.id, 5);
    assert_eq!(b.rows.len(), 0);
    let parent = b.snapshot.dir(b.dir.parent_id.unwrap()).unwrap();
    b.navigate(parent).unwrap();
    b.state.filter.clear();
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
        assert!(first.contains("[own files]"));
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
    let temp = tempfile::tempdir().unwrap();
    let name = RawName::new([b"\x1b\n\xff".as_slice(), &[b'x'; 300]].concat()).unwrap();
    let path = RawPath::new(b"/fixture".to_vec()).unwrap().join(&name);
    let mut rows = vec![
        fixture::row(0, None, b"/fixture", 8192),
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
        old: None,
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        now: 1_000_100,
    };
    b.load().unwrap();
    for pane in [Pane::Main, Pane::Info, Pane::Files, Pane::Links] {
        if pane == Pane::Info {
            b.navigate(b.snapshot.dir(1).unwrap()).unwrap();
        } else if b.dir.id != 0 {
            b.navigate(b.snapshot.root().unwrap()).unwrap();
        }
        b.pane(pane).unwrap();
        let output = render(&b, 160, 48);
        assert!(!output.contains('\x1b'));
        assert!(!output.contains('\u{fffd}'));
        if pane != Pane::Links {
            assert!(output.contains("\\x1B\\x0A\\xFF"));
        }
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
    let temp = tempfile::tempdir().unwrap();
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
        old: None,
        state: State::new(1),
        rows: vec![],
        text: String::new(),
        links: vec![],
        snapshots: vec![],
        message: String::new(),
        now: 1_000_100,
    };
    for step in 0..10 {
        let start = std::time::Instant::now();
        b.load().unwrap();
        let elapsed = start.elapsed();
        println!(
            "dirs={count} step={step} load_page_ms={:.3}",
            elapsed.as_secs_f64() * 1000.0
        );
        assert!(elapsed < std::time::Duration::from_millis(100));
        let Some(next) = b.state.next else {
            break;
        };
        b.state.page_start = next;
    }
}
