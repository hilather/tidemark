//! Owned by: W7-A
//! Selected-directory tide regressions using distinct, valid immutable snapshots.

use super::*;

const GIB: u64 = 1 << 30;
const CHILD: u64 = 417 * GIB / 10;

fn series() -> (tempfile::TempDir, Browser) {
    let (temp, mut b) = browser(1);
    let mut catalog = Vec::new();
    for index in 1..=7 {
        let mut rows = vec![
            fixture::row(0, None, b"/fixture", (174 + index) * GIB),
            fixture::row(1, Some(0), b"/fixture/projects", CHILD - (7 - index) * GIB),
            fixture::row(2, Some(1), b"/fixture/projects/deep", 32 * GIB / 10),
            fixture::row(3, Some(0), b"/fixture/logs", index * GIB),
        ];
        rows[0].own_charged = 0;
        rows[0].own_apparent = 0;
        rows[1].own_charged = 0;
        rows[1].own_apparent = 0;
        rows[1].subtree_end = 2;
        rows[2].depth = 2;
        if index % 2 == 0 {
            for row in &mut rows[1..] {
                row.id += 1;
                row.subtree_end += 1;
                if row.parent_id != Some(0) {
                    row.parent_id = row.parent_id.map(|id| id + 1);
                }
            }
            rows[0].subtree_end += 1;
            rows.insert(1, fixture::row(1, Some(0), b"/fixture/added-earlier", 1));
        }
        let mut meta = fixture::meta(rows[0].path.clone(), 1_760_000_000 + index as i64 * 3600);
        // The generic row helper uses 100 seconds after the epoch. Capture fixtures
        // need real Unix seconds near their snapshot, with a distinct subtree max.
        for row in &mut rows {
            row.newest_mtime = meta.started_at - i64::from(row.id) * 600;
            row.mtime = row.newest_mtime - 3600;
        }
        meta.scan_id = serde_json::from_str(&format!("\"20260101T000000Z-{index:08}\"")).unwrap();
        let mut writer =
            tidemark_store::writer::SnapshotWriter::create(temp.path(), &meta).unwrap();
        writer.put_dirs(&rows).unwrap();
        let projects = rows
            .iter()
            .find(|r| r.path.as_bytes() == b"/fixture/projects")
            .unwrap();
        writer
            .put_annotations(&[tidemark_core::snapshot::AnnotationRow {
                dir_id: projects.id,
                key: "owner".into(),
                value: "projects".into(),
            }])
            .unwrap();
        let file = writer.finish(&meta).unwrap();
        catalog.push((
            meta.scan_id.to_string(),
            file,
            meta.started_at,
            Some(rows[0].t_disk),
        ));
    }
    b.snapshot_file = catalog.last().unwrap().1.clone();
    b.snapshot = Snapshot::open(&b.snapshot_file).unwrap();
    b.clock = b.snapshot.meta().finished_at;
    b.dir = b.snapshot.root().unwrap();
    b.snapshots = catalog;
    b.load().unwrap();
    (temp, b)
}

#[test]
fn newest_fixture_seconds_survive_snapshot_and_live_rows() {
    let (_temp, mut b) = series();
    let started_at = b.snapshot.meta().started_at;
    for (path, id, expected) in [
        (b"/fixture/projects".as_slice(), 1, "10-09 15:43"),
        (b"/fixture/logs".as_slice(), 3, "10-09 15:23"),
    ] {
        let row = b
            .snapshot
            .dir_by_path(&RawPath::new(path.to_vec()).unwrap())
            .unwrap()
            .unwrap();
        assert_eq!(row.newest_mtime, started_at - id * 600);
        assert_ne!(row.newest_mtime, row.mtime, "NEWEST is the subtree maximum");
        let text = render(&b, 140, 40);
        let line = text
            .lines()
            .find(|line| {
                line.contains(&format!(
                    "{}/",
                    row.path.file_name().unwrap().display_lossy()
                ))
            })
            .unwrap();
        assert!(line.contains(expected), "{line}");
    }

    let root = b.dir.clone();
    let mut rows = b.rows.clone();
    rows[0].0.mtime = 100;
    rows[0].0.newest_mtime = started_at + 3600;
    install_refresh(&mut b, root, rows);
    b.poll_refresh().unwrap();
    let text = render(&b, 140, 40);
    let line = text
        .lines()
        .find(|line| line.contains("projects/"))
        .unwrap();
    assert!(line.contains("10-09 16:53"), "{line}");
}

#[test]
fn newest_zero_and_missing_render_as_dash_in_saved_and_live_rows() {
    let (_temp, mut b) = series();
    for live in [false, true] {
        let mut rows = b.rows.clone();
        rows[0].0.newest_mtime = 0;
        rows[1].0.newest_mtime = i64::MIN;
        if live {
            let root = b.dir.clone();
            install_refresh(&mut b, root, rows);
            b.poll_refresh().unwrap();
        } else {
            b.rows = rows;
        }
        let text = render(&b, 140, 40);
        for name in ["projects/", "logs/"] {
            let row = text.lines().find(|line| line.contains(name)).unwrap();
            // FILES, NEWEST, TIDE are the final three columns.
            assert_eq!(
                row.trim_end_matches('│').split_whitespace().rev().nth(1),
                Some("-"),
                "{row}"
            );
        }
    }
}

#[test]
fn reported_chart_total_follows_highlighted_directory() {
    let (_temp, mut b) = series();
    pump(&mut b);
    let screen = render(&b, 140, 40);
    let chart = screen
        .lines()
        .skip(2)
        .take(9)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(chart.contains("/fixture/projects"), "{chart}");
    assert!(chart.contains("41.7 GiB"), "{chart}");
    assert!(!chart.contains("181 GiB"), "{chart}");
    b.on_key(KeyCode::Down, &Default::default()).unwrap();
    pump(&mut b);
    let screen = render(&b, 140, 40);
    assert!(screen.contains("tide · /fixture/logs · disk"), "{screen}");
}

#[test]
fn reported_info_follows_highlight_and_descended_first_row() {
    let (_temp, mut b) = series();
    b.on_key(KeyCode::Char('i'), &Default::default()).unwrap();
    assert!(b.text.starts_with("/fixture/projects\n"), "{}", b.text);
    assert!(b.text.contains("Disk 41.7 GiB"), "{}", b.text);
    b.close_pane();
    b.on_key(KeyCode::Enter, &Default::default()).unwrap();
    b.on_key(KeyCode::Char('i'), &Default::default()).unwrap();
    assert!(b.text.starts_with("/fixture/projects/deep\n"), "{}", b.text);
    assert!(b.text.contains("Disk 3.20 GiB"), "{}", b.text);
    b.close_pane();
    b.on_key(KeyCode::Backspace, &Default::default()).unwrap();
    b.on_key(KeyCode::Char('i'), &Default::default()).unwrap();
    assert!(b.text.starts_with("/fixture/projects\n"), "{}", b.text);
}

#[test]
fn failed_info_frame_keeps_running_and_retries_until_the_read_succeeds() {
    let (temp, mut b) = series();
    b.pane(Pane::Info).unwrap();
    let previous_text = b.text.clone();
    let previous_subject = b.info_subject.clone();

    // Rename the annotations table in a private copy without changing SQLite page offsets.
    // This preserves readable metadata/directories but makes the real Info query fail.
    let mut bytes = std::fs::read(&b.snapshot_file).unwrap();
    let name = b"annotations";
    let mut replacements = 0;
    for offset in 0..=bytes.len() - name.len() {
        if &bytes[offset..offset + name.len()] == name {
            bytes[offset] = b'x';
            replacements += 1;
        }
    }
    assert!(replacements > 0);
    let broken = temp.path().join("missing-annotations.db");
    std::fs::write(&broken, bytes).unwrap();
    b.snapshot = Snapshot::open(&broken).unwrap();
    b.data_revision += 1;
    for _ in 0..2 {
        b.message.clear();
        b.prepare_frame(80, 24);
        assert!(b.message.contains("annotations"), "{}", b.message);
        assert_eq!(b.state.pane, Pane::Info);
        assert_eq!(b.info_subject, previous_subject);
        assert_eq!(b.text, previous_text);
        assert!(render(&b, 80, 24).contains("annotations"));
    }

    // Preparing another pane does not establish that the Info read recovered.
    let info_error = b.message.clone();
    b.state.pane = Pane::Main;
    b.prepare_frame(80, 24);
    assert_eq!(b.message, info_error);
    b.state.pane = Pane::Info;

    // No subject/revision change: the next frame must retry the failed read.
    b.snapshot = Snapshot::open(&b.snapshot_file).unwrap();
    b.prepare_frame(80, 24);
    assert_eq!(
        b.info_subject,
        Some((b.subject().0.path.clone(), b.data_revision))
    );
    assert_eq!(b.text.matches("owner: projects").count(), 1);
    assert!(b.message.is_empty(), "{}", b.message);

    // A message from another action must survive a successful Info retry.
    b.snapshot = Snapshot::open(&broken).unwrap();
    b.data_revision += 1;
    b.prepare_frame(80, 24);
    assert!(b.message.contains("annotations"), "{}", b.message);
    b.message = "Live refresh already running".into();
    b.snapshot = Snapshot::open(&b.snapshot_file).unwrap();
    b.prepare_frame(80, 24);
    assert_eq!(
        b.info_subject,
        Some((b.subject().0.path.clone(), b.data_revision))
    );
    assert_eq!(b.message, "Live refresh already running");
    assert!(!b.on_key(KeyCode::Esc, &Default::default()).unwrap());
    assert_eq!(b.state.pane, Pane::Main);
}

#[test]
fn chart_title_and_delta_keep_separate_display_budgets() {
    let (_temp, mut b) = series();
    pump(&mut b);
    for path in [
        format!("/{}", "p".repeat(180)),
        format!("/{}", "海".repeat(90)),
    ] {
        b.rows[0].0.path = RawPath::new(path.into_bytes()).unwrap();
        for width in [100, 140] {
            let screen = buffer(&b, width, 40);
            let mut title = String::new();
            let mut x = 0;
            while x < width {
                let glyph = screen[(x, 2)].symbol();
                title.push_str(glyph);
                // Ratatui reserves a second cell for the wide fixture character.
                x += if glyph == "海" { 2 } else { 1 };
            }
            let suffix = " · disk 41.7 GiB";
            let left_end = title.find(suffix).expect("complete subject title") + suffix.len();
            let left_start = title.find("tide · ").unwrap();
            let delta_start = title.find(" Δ ").expect("complete delta label");
            let delta_end = title.rfind('┐').unwrap();
            let left = &title[left_start..left_end];
            let right = &title[delta_start..delta_end];
            assert!(right.contains("+1.00 GiB since previous ▲"), "{title}");
            assert!(title[left_end..delta_start].contains('─'), "{title}");
            assert!(
                crate::render::table::display_width(left)
                    + crate::render::table::display_width(right)
                    + 3
                    <= usize::from(width - 2),
                "conservative title budgets overlap: {title}"
            );
        }
    }
}

#[test]
fn reported_zero_axis_and_visible_wave_crest() {
    let (_temp, mut b) = series();
    pump(&mut b);
    let screen = render(&b, 140, 40);
    assert!(!screen.contains("axis from"), "{screen}");
    assert!(
        screen.lines().nth(7).unwrap().starts_with(" │0"),
        "{screen}"
    );
    assert!(!screen.lines().nth(10).unwrap().contains("0 B"), "{screen}");
    assert!(
        screen.contains("▆▆▇▇▇▇████▇▇▇▇▆▆"),
        "full-height crest missing\n{screen}"
    );
}

fn pump(b: &mut Browser) {
    b.prepare(140, 40).unwrap();
    b.history.settle(b.clock);
}

fn buffer(b: &Browser, width: u16, height: u16) -> ratatui::buffer::Buffer {
    let mut terminal = Terminal::new(TestBackend::new(width, height)).unwrap();
    terminal.draw(|f| draw::draw(f, b)).unwrap();
    terminal.backend().buffer().clone()
}

#[test]
fn zero_axis_clamp_and_every_fractional_crest() {
    assert_eq!(
        [0, 25, 50, 100].map(|v| draw::eighths(v, 100, 5)),
        [0, 10, 20, 40]
    );
    assert_eq!(draw::eighths(u64::MAX, 1, 5), 40);
    assert_eq!(draw::eighths(u64::MAX, u64::MAX, 5), 40);
    assert_eq!(draw::eighths(1, u64::MAX, 5), 0);
    for h in [0, 1, 5] {
        assert_eq!(draw::eighths(0, 0, h), 0);
        assert_eq!(draw::eighths(100, 100, h), h * 8);
        for e in 1..=8 * h {
            let fill = (e - 1) % 8 + 1;
            let base = (e - 1) / 8 * 8;
            let levels: Vec<_> = (0..16).map(|c| draw::crest(fill, c)).collect();
            assert_eq!(levels.iter().max(), Some(&fill));
            assert!(levels.iter().all(|level| *level >= 1 && base + level <= e));
            assert_eq!(levels, levels.iter().rev().copied().collect::<Vec<_>>());
            assert!(levels.iter().all(|level| fill - level <= 2));
            let runs: Vec<_> = levels
                .iter()
                .enumerate()
                .filter_map(|(i, level)| (i == 0 || *level != levels[i - 1]).then_some(*level))
                .collect();
            let maxima = runs
                .iter()
                .enumerate()
                .filter(|(i, level)| {
                    (*i == 0 || **level > runs[*i - 1])
                        && (*i + 1 == runs.len() || **level > runs[*i + 1])
                })
                .count();
            assert_eq!(maxima, 1, "one maximum run, never a sawtooth: {levels:?}");
            if fill == 1 {
                assert_eq!(levels, vec![1; 16]);
            }
        }
    }

    let (_temp, mut b) = series();
    pump(&mut b);
    for e in 1..=8 {
        b.history.samples.fill(Some(history::Sample::Known(40)));
        b.history.samples[0] = Some(history::Sample::Known(e));
        let screen = buffer(&b, 140, 40);
        let mut levels = Vec::new();
        for x in 4..20 {
            for y in 3..7 {
                assert_eq!(screen[(x, y)].symbol(), " ", "e={e}, x={x}, y={y}");
            }
            let level = [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"]
                .iter()
                .position(|glyph| *glyph == screen[(x, 7)].symbol())
                .expect("eighth-block cell");
            assert!(
                (1..=e as usize).contains(&level),
                "e={e}, x={x}, level={level}"
            );
            levels.push(level);
        }
        assert_eq!(levels.iter().max(), Some(&(e as usize)));
        if e == 1 {
            assert_eq!(levels, vec![1; 16]);
        } else {
            assert!(levels.iter().any(|level| *level < e as usize));
        }
    }
}

#[test]
fn previous_level_tick_uses_the_plot_scale_and_known_comparison_only() {
    use history::Sample::{Known, MissingPath, Unavailable};
    let (_temp, mut b) = series();
    pump(&mut b);
    for width in [100, 140] {
        let tick_x = if width == 100 { 79 } else { 117 };
        for (previous, y) in [
            (0, 7),
            (1, 7),
            (8, 7),
            (9, 6),
            (16, 6),
            (17, 5),
            (32, 4),
            (33, 3),
            (40, 3),
        ] {
            b.history.samples.fill(Some(Known(40)));
            b.history.samples[5] = Some(Known(previous));
            let screen = buffer(&b, width, 40);
            for row in 3..=7 {
                assert_eq!(
                    screen[(tick_x, row)].symbol(),
                    if row == y { "─" } else { " " },
                    "width={width}, previous={previous}, row={row}"
                );
            }
        }
        for missing in [None, Some(MissingPath), Some(Unavailable("test"))] {
            b.history.samples[5] = missing;
            let screen = buffer(&b, width, 40);
            assert!((3..=7).all(|row| screen[(tick_x, row)].symbol() == " "));
        }
    }
    // Live compares against the opened snapshot, even when the older predecessor differs.
    b.history.samples.fill(Some(Known(40)));
    b.history.samples[6] = Some(Known(20));
    b.pages.live = Some(b.rows.clone());
    b.pages.live_at = Some(b.clock);
    b.rows[0].0.t_disk = 80;
    let screen = buffer(&b, 140, 40);
    assert_eq!(screen[(117, 6)].symbol(), "─"); // 20/80 * 40 = 10 eighths.
    assert_eq!(screen[(117, 5)].symbol(), " "); // Not the older 40-byte sample.
    b.history.samples[6] = Some(MissingPath);
    assert!((3..=7).all(|y| buffer(&b, 140, 40)[(117, y)].symbol() == " "));
}

#[test]
fn latest_bar_alone_is_bright_in_every_palette_with_and_without_live() {
    use ratatui::style::Modifier;
    let (_temp, mut b) = series();
    pump(&mut b);
    for live in [false, true] {
        if live {
            b.pages.live = Some(b.rows.clone());
            b.pages.live_at = Some(b.clock);
        }
        for (no_color, color, term, level) in [
            (false, Some("truecolor"), "xterm", theme::Level::Rgb),
            (false, None, "xterm-256color", theme::Level::Indexed),
            (false, None, "xterm", theme::Level::Basic),
            (true, None, "xterm", theme::Level::Mono),
        ] {
            b.theme = theme::Theme::select(true, no_color, color, Some(term), Some("C.UTF-8"));
            assert_eq!(b.theme.level, level);
            let screen = buffer(&b, 140, 40);
            for index in 0..7 {
                let x = 4 + index * 19;
                let latest = index == 6;
                for y in 3..=7 {
                    if screen[(x, y)].symbol() == " " {
                        continue;
                    }
                    assert_eq!(
                        screen[(x, y)].fg,
                        b.theme.color(if latest {
                            theme::Role::Foam
                        } else {
                            theme::Role::Teal
                        })
                    );
                    assert_eq!(
                        screen[(x, y)].modifier.contains(Modifier::BOLD),
                        latest && matches!(level, theme::Level::Basic | theme::Level::Mono)
                    );
                }
            }
        }
    }
}

#[test]
fn renderer_pins_crest_coordinates_colors_labels_and_zero_slots() {
    use history::Sample::Known;
    use ratatui::style::{Color, Modifier};
    let (_temp, mut b) = series();
    pump(&mut b);
    b.theme = theme::Theme::select(
        true,
        false,
        Some("truecolor"),
        Some("xterm"),
        Some("C.UTF-8"),
    );
    b.history.samples = [0, 25, 50, 100, 1, 100, 100]
        .map(|v| Some(Known(v)))
        .to_vec();
    let screen = buffer(&b, 140, 40);
    assert_eq!(screen[(1, 2)].symbol(), "┌");
    for y in 3..=7 {
        assert_eq!(screen[(4, y)].symbol(), " ");
    }
    assert_eq!(screen[(29, 6)].symbol(), "▂"); // 25/100 = 10 eighths at centre.
    assert_eq!(screen[(24, 6)].symbol(), "▁");
    assert_eq!(screen[(48, 5)].symbol(), "▄"); // 50/100 = 20 eighths.
    assert_eq!(screen[(67, 3)].symbol(), "█");
    assert_eq!(screen[(63, 3)].symbol(), "▇");
    assert_eq!(screen[(61, 3)].symbol(), "▆");
    assert_eq!(screen[(61, 3)].fg, Color::Rgb(0x2e, 0xc4, 0xb6));
    assert_eq!(screen[(61, 4)].fg, Color::Rgb(0x2e, 0xc4, 0xb6));
    assert_eq!(screen[(118, 4)].fg, Color::Rgb(0x9e, 0xf0, 0xe6));
    assert!(!screen[(118, 3)].modifier.contains(Modifier::BOLD));
    for y in 3..=9 {
        for x in 2..138 {
            assert_eq!(screen[(x, y)].bg, Color::Rgb(7, 21, 38));
        }
    }
    let text = render(&b, 140, 40);
    assert!(text.lines().nth(8).unwrap().contains('●'));
    assert!(text.lines().nth(9).unwrap().contains("1 B")); // tiny positive keeps its value.
    assert!(!text
        .lines()
        .skip(3)
        .take(5)
        .any(|line| line.contains("100 B")));
    assert!(!text.lines().nth(10).unwrap().contains("0 B"));
    b.history.samples.fill(Some(Known(0)));
    let zero = buffer(&b, 140, 40);
    for y in 3..=7 {
        for x in 4..134 {
            assert_eq!(
                zero[(x, y)].symbol(),
                if (x, y) == (117, 7) { "─" } else { " " }
            );
        }
    }
}

#[test]
fn history_matches_paths_when_ids_change_and_older_opened_scans_stay_last() {
    use history::Sample::Known;
    let (_temp, mut b) = series();
    pump(&mut b);
    assert_eq!(
        b.history.samples,
        (1..=7)
            .map(|i| Some(Known(CHILD - (7 - i) * GIB)))
            .collect::<Vec<_>>()
    );
    b.on_key(KeyCode::Enter, &Default::default()).unwrap();
    pump(&mut b);
    assert!(b
        .history
        .samples
        .iter()
        .all(|s| *s == Some(Known(32 * GIB / 10))));
    b.on_key(KeyCode::Backspace, &Default::default()).unwrap();
    b.on_key(KeyCode::Char('p'), &Default::default()).unwrap();
    let subject = b.subject().0.path.clone();
    b.on_key(KeyCode::Down, &Default::default()).unwrap();
    assert_eq!(b.subject().0.path, subject);
    b.state.selected = 2;
    b.enter().unwrap();
    pump(&mut b);
    assert_eq!(b.history.window.len(), 3);
    assert_eq!(
        b.history.window.last().unwrap().id,
        b.snapshot.meta().scan_id.as_str()
    );
    assert!(render(&b, 140, 40).contains("since previous"));
}

#[test]
fn own_no_match_and_backspace_follow_the_cursor_without_a_selection_stack() {
    use history::Sample::Known;
    let (_temp, mut b) = series();
    b.on_key(KeyCode::End, &Default::default()).unwrap();
    pump(&mut b);
    assert!(b.subject().1);
    assert_eq!(b.history.samples.last(), Some(&Some(Known(181 * GIB))));
    let text = render(&b, 140, 40);
    assert!(text.contains("whole directory"));
    assert!(text.contains("(files here)"));
    b.on_key(KeyCode::Char('i'), &Default::default()).unwrap();
    assert!(b.text.contains("Disk 181 GiB"));
    assert!(!b.text.contains("[own files]"));
    b.close_pane();
    b.state.filter = "nothing".into();
    b.boundary(false).unwrap();
    pump(&mut b);
    assert!(b.subject().1);
    assert_eq!(b.history.samples.last(), Some(&Some(Known(181 * GIB))));
    b.state.filter.clear();
    b.state.sort = state::Sort::Name;
    b.boundary(false).unwrap();
    // Name sorting puts ./ at row 0; select projects explicitly, then ascend to ./.
    let index = b
        .child_display_index(&RawPath::new(b"/fixture/projects".to_vec()).unwrap())
        .unwrap();
    b.state.selected = index;
    b.main_selected = index;
    b.on_key(KeyCode::Enter, &Default::default()).unwrap();
    b.on_key(KeyCode::Backspace, &Default::default()).unwrap();
    pump(&mut b);
    assert_eq!(b.main_selected, 0);
    assert!(b.subject().1);
    b.on_key(KeyCode::Char('i'), &Default::default()).unwrap();
    assert!(b.text.contains("Disk 181 GiB"));
}

#[test]
fn live_endpoint_preserves_saved_history_scales_above_max_and_stamps_completion_clock() {
    use history::Sample::Known;
    let (_temp, mut b) = series();
    pump(&mut b);
    let saved = b.history.samples.clone();
    let mut rows = b.rows.clone();
    rows[0].0.t_disk = CHILD * 2;
    rows[0].0.id = 900;
    b.clock = 2_000_000_000;
    let root = b.dir.clone();
    install_refresh(&mut b, root, rows);
    b.poll_refresh().unwrap();
    pump(&mut b);
    assert_eq!(b.pages.live_at, Some(b.clock));
    assert_eq!(b.history.samples, saved);
    assert_eq!(b.history.samples.last(), Some(&Some(Known(CHILD))));
    let text = render(&b, 140, 40);
    assert!(text.contains("◆ live"));
    assert!(text.contains("vs snapshot"));
    assert!(text.contains("83.4 GiB"));
    let screen = buffer(&b, 140, 40);
    assert_eq!(screen[(124, 3)].symbol(), "█"); // live max: full height at centre.
    assert_eq!(screen[(105, 5)].symbol(), "▄"); // saved current: exactly half height.
    b.on_key(KeyCode::Char('i'), &Default::default()).unwrap();
    assert!(b.text.contains("Disk 83.4 GiB"));
    assert!(
        b.text.contains("owner: projects"),
        "live id must resolve to the saved path"
    );
    b.close_pane();
    b.on_key(KeyCode::Enter, &Default::default()).unwrap();
    pump(&mut b);
    assert!(b.pages.live.is_none());
    assert!(!render(&b, 140, 40).contains("◆ live"));
}

#[test]
fn live_only_path_does_not_borrow_saved_ids_or_seed_a_snapshot() {
    use history::Sample::MissingPath;
    let (_temp, mut b) = series();
    let mut live = b.rows.clone();
    live[0].0.path = RawPath::new(b"/fixture/only-live".to_vec()).unwrap();
    live[0].0.name = b"only-live".to_vec(); // Deliberately reuses a saved directory id.
    let root = b.dir.clone();
    install_refresh(&mut b, root, live);
    b.poll_refresh().unwrap();
    pump(&mut b);
    assert!(b
        .history
        .samples
        .iter()
        .all(|sample| *sample == Some(MissingPath)));
    let text = render(&b, 140, 40);
    assert!(text.contains("◆ live"));
    assert!(!text.contains("vs snapshot"));
    b.on_key(KeyCode::Char('i'), &Default::default()).unwrap();
    assert!(b.text.starts_with("/fixture/only-live\n"));
    assert!(!b.text.contains("Link totals"));
    assert!(!b.text.contains("owner: projects"));
}

#[test]
fn pruned_predecessor_in_picker_keeps_browser_open_and_suppresses_delta() {
    let (_temp, mut b) = series();
    std::fs::remove_file(&b.snapshots[4].1).unwrap();
    b.pane(Pane::Picker).unwrap();
    b.state.selected = 5;
    b.enter().unwrap();
    pump(&mut b);
    assert!(b.old.is_none());
    assert!(b.message.contains("comparison unavailable"));
    assert!(matches!(
        b.history.samples[4],
        Some(history::Sample::Unavailable(_))
    ));
    let text = render(&b, 140, 40);
    assert!(text.contains("n/a"));
    assert!(!text.contains("since previous"));
}

#[test]
fn picker_comparison_message_clears_when_predecessor_opens() {
    let (_temp, mut b) = series();
    let file = b.snapshots[4].1.clone();
    let aside = file.with_extension("aside");
    std::fs::rename(&file, &aside).unwrap();
    b.pane(Pane::Picker).unwrap();
    b.state.selected = 5;
    b.enter().unwrap();
    assert!(b.old.is_none());
    assert_eq!(
        b.message,
        "Snapshot switched; preceding comparison unavailable"
    );
    std::fs::rename(&aside, &file).unwrap();
    b.clock += history::COOLDOWN;
    b.prepare(140, 40).unwrap();
    b.deltas(24, false).unwrap();
    assert_eq!(b.message, "");
    assert!(b.old.is_some());
    let grew = crate::render::tree::signed_bytes(1 << 30);
    let text = render(&b, 140, 40);
    assert!(text.contains(&grew), "{text}");
    b.old = None;
    b.prev_open_failed_at = None;
    b.message = "Path absent in selected snapshot".into();
    b.prepare(140, 40).unwrap();
    assert_eq!(b.message, "Path absent in selected snapshot");
    assert!(b.old.is_some());
}

#[test]
fn chart_snapshots_cover_selections_missing_live_and_fallbacks() {
    let (_temp, mut b) = series();
    pump(&mut b);
    for (name, width, height) in [
        ("seven", 140, 40),
        ("threshold", 100, 34),
        ("compact", 80, 24),
    ] {
        let text = render(&b, width, height);
        insta::with_settings!({snapshot_path => "snapshots"}, { insta::assert_snapshot!(format!("tide_{name}"), text); });
    }
    b.on_key(KeyCode::Down, &Default::default()).unwrap();
    pump(&mut b);
    insta::with_settings!({snapshot_path => "snapshots"}, { insta::assert_snapshot!("tide_logs", render(&b, 140, 40)); });
    b.history.samples[2] = Some(history::Sample::MissingPath);
    b.history.samples[5] = Some(history::Sample::Unavailable("cannot open snapshot"));
    insta::with_settings!({snapshot_path => "snapshots"}, { insta::assert_snapshot!("tide_missing", render(&b, 140, 40)); });
    let root = b.dir.clone();
    let rows = b.rows.clone();
    install_refresh(&mut b, root, rows);
    b.poll_refresh().unwrap();
    pump(&mut b);
    insta::with_settings!({snapshot_path => "snapshots"}, { insta::assert_snapshot!("tide_live", render(&b, 140, 40)); });
    for (term, lang, level) in [
        ("xterm", "C.UTF-8", theme::Level::Basic),
        ("xterm-256color", "C.UTF-8", theme::Level::Indexed),
        ("linux", "C", theme::Level::Basic),
    ] {
        b.theme = theme::Theme::select(true, false, None, Some(term), Some(lang));
        assert_eq!(b.theme.level, level);
        let screen = buffer(&b, 140, 40);
        assert_eq!(screen[(118, 7)].fg, b.theme.color(theme::Role::Foam));
        if term == "linux" {
            let text = render(&b, 140, 40);
            let chart: String = text.lines().skip(2).take(9).collect();
            assert!(chart.is_ascii());
            assert!(chart.contains("+ live"));
            insta::with_settings!({snapshot_path => "snapshots"}, { insta::assert_snapshot!("tide_ascii", text); });
        }
    }
}

fn buffer_rows(screen: &ratatui::buffer::Buffer) -> Vec<String> {
    (0..screen.area.height)
        .map(|y| {
            (0..screen.area.width)
                .map(|x| screen[(x, y)].symbol())
                .collect()
        })
        .collect()
}

/// Every cell is ASCII. A box-drawing scalar (U+2500–U+257F) is rejected on its own.
fn assert_ascii_cells(screen: &ratatui::buffer::Buffer) {
    for y in 0..screen.area.height {
        for x in 0..screen.area.width {
            let symbol = screen[(x, y)].symbol();
            if symbol.is_ascii() {
                continue;
            }
            let ch = symbol.chars().next().unwrap();
            assert!(
                !('\u{2500}'..='\u{257F}').contains(&ch),
                "box drawing U+{:04X} {symbol:?} at ({x}, {y})",
                u32::from(ch)
            );
            panic!("non-ascii ({x}, {y}) {symbol:?} U+{:04X}", u32::from(ch));
        }
    }
}

/// The pane corner sits in the cell immediately before its title, including a centered overlay.
fn assert_plus_corner(screen: &ratatui::buffer::Buffer, title: &str) {
    let found = buffer_rows(screen).iter().any(|row| {
        row.find(title)
            .is_some_and(|index| row[..index].ends_with('+'))
    });
    assert!(found, "missing + corner before {title}");
}

#[test]
fn ascii_chrome_is_ascii() {
    let (_temp, mut b) = series();
    b.theme = theme::Theme::select(true, false, None, Some("linux"), Some("C"));
    pump(&mut b);
    let main = buffer(&b, 140, 40);
    assert_ascii_cells(&main);
    assert_plus_corner(&main, "depth");
    b.pane(Pane::Help).unwrap();
    let help = buffer(&b, 80, 24);
    assert_ascii_cells(&help);
    assert_plus_corner(&help, "help . ");
    let overlay = buffer(&b, 79, 23);
    assert_ascii_cells(&overlay);
    assert_plus_corner(&overlay, "tide");
    // Theme is already ASCII. `links` copies `glyphs.anchor` when the pane opens.
    for (pane, title) in [
        (Pane::Picker, "picker . "),
        (Pane::Info, "info . "),
        (Pane::Files, "Files | "),
        (Pane::Links, "links . "),
    ] {
        b.pane(pane).unwrap();
        let screen = buffer(&b, 80, 24);
        assert_ascii_cells(&screen);
        assert_plus_corner(&screen, title);
    }
}

#[test]
fn unicode_chrome_keeps_plain_borders() {
    let (_temp, mut b) = series();
    b.theme = theme::Theme::plain();
    pump(&mut b);
    let main = buffer(&b, 140, 40);
    assert_eq!(main[(1, 2)].symbol(), "┌");
    let rows = buffer_rows(&main);
    let depth = rows
        .iter()
        .position(|row| row.contains("┌depth"))
        .expect("depth title");
    assert!(rows[depth].contains("┌depth"));
    assert!(
        rows[depth + 1].starts_with('│'),
        "table edge {}",
        rows[depth + 1]
    );
    b.pane(Pane::Help).unwrap();
    let help = buffer_rows(&buffer(&b, 80, 24));
    let title = help
        .iter()
        .position(|row| row.contains("┌help"))
        .expect("help title");
    assert!(
        help[title + 1].starts_with('│'),
        "help edge {}",
        help[title + 1]
    );
}

#[test]
fn history_subject_tracks_page_boundaries_sorts_and_filter() {
    let (_temp, mut b) = browser_by_size(2500);
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
        b.main_selected = b.state.selected;
        for key in [
            KeyCode::End,
            KeyCode::Home,
            KeyCode::PageDown,
            KeyCode::Down,
            KeyCode::Up,
            KeyCode::PageUp,
        ] {
            b.on_key(key, &Default::default()).unwrap();
            pump(&mut b);
            let (row, _) = b.subject();
            assert_eq!(b.history.key.as_ref().unwrap().path, row.path);
            assert_eq!(
                b.history.samples.last(),
                Some(&Some(history::Sample::Known(row.t_disk)))
            );
        }
    }
    b.state.filter = "00002000".into();
    b.boundary(false).unwrap();
    pump(&mut b);
    assert!(b
        .history
        .key
        .as_ref()
        .unwrap()
        .path
        .as_bytes()
        .ends_with(b"00002000"));
}

#[test]
fn insufficient_history_null_catalog_totals_and_hidden_chart() {
    let (_temp, mut b) = series();
    for row in &mut b.snapshots {
        row.3 = None;
    }
    pump(&mut b);
    assert!(render(&b, 100, 34).contains("41.7 GiB"));
    assert!(!render(&b, 80, 24).contains("tide ·"));
    b.pane(Pane::Picker).unwrap();
    b.state.selected = 0;
    b.enter().unwrap();
    pump(&mut b);
    let text = render(&b, 140, 40);
    assert!(text.contains("Insufficient history"));
    assert!(text.contains("35.7 GiB disk"));

    let mut live = b.rows.clone();
    live[0].0.t_disk = CHILD * 2;
    let root = b.dir.clone();
    install_refresh(&mut b, root, live);
    b.poll_refresh().unwrap();
    pump(&mut b);
    assert_eq!(b.history.window.len(), 1);
    for width in [100, 140] {
        let text = render(&b, width, 40);
        assert!(
            text.contains("Insufficient history · 83.4 GiB disk"),
            "{text}"
        );
        assert!(!text.contains("◆ live"), "{text}");
        assert!(!text.contains("vs snapshot"), "{text}");
    }
}

#[test]
fn failed_and_cancelled_refresh_do_not_create_live_endpoints() {
    for cancelled in [false, true] {
        let (_temp, mut b) = series();
        let (tx, rx) = crossbeam_channel::bounded(1);
        b.refresh = Some(Refresh {
            cancel: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(cancelled)),
            rx,
            worker: None,
            started: std::time::Instant::now(),
        });
        tx.send(Err(anyhow::anyhow!("fixture refresh failure")))
            .unwrap();
        b.poll_refresh().unwrap();
        pump(&mut b);
        assert!(b.pages.live.is_none());
        assert!(b.pages.live_at.is_none());
        assert!(!render(&b, 140, 40).contains("◆ live"));
    }
}

/// Export the exact deterministic series into an explicitly requested isolated state directory.
/// No files are retained in ordinary test runs.
#[test]
fn export_visual_fixture_when_requested() {
    use tidemark_store::catalog::{Catalog, ScanEntry};
    let Some(destination) = std::env::var_os("TIDEMARK_TIDE_CAPTURE_DIR") else {
        return;
    };
    let destination = std::path::PathBuf::from(destination);
    assert!(destination.starts_with("/tmp"));
    assert!(!destination.exists(), "choose a fresh isolated directory");
    std::fs::create_dir(&destination).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o700)).unwrap();
    let root = destination.join("tree");
    std::fs::create_dir_all(root.join("projects/deep")).unwrap();
    std::fs::create_dir_all(root.join("logs")).unwrap();
    let raw_root = RawPath::from_path(&root).unwrap();
    let state = destination.join("state");
    let catalog = Catalog::open(&state, true).unwrap();
    let files = state
        .join("snapshots")
        .join(tidemark_store::catalog::root_slug(&raw_root));
    let (_temp, b) = series();
    for entry in &b.snapshots {
        let original = Snapshot::open(&entry.1).unwrap();
        let mut meta = original.meta().clone();
        meta.root = raw_root.clone();
        meta.root_display = raw_root.display_lossy();
        let mut rows = Vec::new();
        for id in 0..=original.root().unwrap().subtree_end {
            let mut row = original.dir(id).unwrap();
            row.path = RawPath::new(
                [
                    raw_root.as_bytes(),
                    &row.path.as_bytes()[b"/fixture".len()..],
                ]
                .concat(),
            )
            .unwrap();
            row.path_hash = row.path.hash64();
            if id == 0 {
                row.name = raw_root.as_bytes().to_vec();
            }
            rows.push(row);
        }
        // Logs exists only in newer saved scans, giving the visual missing-path case.
        if entry.2 < b.snapshots[3].2 {
            rows.retain(|r| !r.path.as_bytes().ends_with(b"/logs"));
        }
        let mut writer = tidemark_store::writer::SnapshotWriter::create(&files, &meta).unwrap();
        writer.put_dirs(&rows).unwrap();
        let file = writer.finish(&meta).unwrap();
        catalog
            .record_finish(&ScanEntry {
                scan_id: meta.scan_id.to_string(),
                root: raw_root.clone(),
                mode: meta.mode,
                started_at: meta.started_at,
                finished_at: Some(meta.finished_at),
                status: "complete".into(),
                degraded: false,
                file: Some(file),
                t_disk: entry.3,
                t_inodes: Some(3),
                t_dirs: Some(rows.len() as u64),
                errors: Some(0),
                duration_ms: Some(1000),
            })
            .unwrap();
    }
    std::fs::write(
        destination.join("config.toml"),
        format!("[general]\nstate_dir = {:?}\n", state),
    )
    .unwrap();
    println!("visual fixture {}", destination.display());
}
