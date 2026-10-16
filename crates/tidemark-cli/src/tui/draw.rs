//! Owned by: W7-A
//! Deterministic Tide drawing without filesystem or clock access.

use super::state::Pane;
use super::theme::{Glyphs, Level, Role, Theme};
use super::{Browser, CatalogRow};
use crate::render::{size, table};
use ratatui::{
    layout::{Constraint, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Cell, List, ListItem, ListState, Paragraph, Row, Table, TableState},
    Frame,
};
use tidemark_core::snapshot::DirRow;

/// Same UTC civil fields as `size::timestamp`, without the seconds or the `Z`.
pub(super) fn civil(ts: i64) -> (i64, i64, i64, i64, i64) {
    let z = ts.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let mut year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let mut month = mp + if mp < 10 { 3 } else { -9 };
    year += i64::from(month <= 2);
    if month <= 0 {
        month += 12;
    }
    let second = ts.rem_euclid(86_400);
    (year, month, day, second / 3600, (second / 60) % 60)
}

/// `MM-DD HH:MM` in the same UTC year as `now`, otherwise `YYYY-MM-DD`.
/// Zero and the empty-maximum sentinel have no useful timestamp to display.
pub(super) fn fmt_when(ts: i64, now: i64) -> String {
    let (year, month, day, hour, minute) = civil(ts);
    let (now_year, _, _, _, _) = civil(now);
    if year == now_year {
        format!("{month:02}-{day:02} {hour:02}:{minute:02}")
    } else {
        format!("{year:04}-{month:02}-{day:02}")
    }
}

/// NEWEST cell: an unset subtree maximum (0, or the empty-maximum sentinel
/// `i64::MIN`) renders as `-`; every other time goes through [`fmt_when`].
pub(super) fn fmt_newest(ts: i64, now: i64) -> String {
    if ts == 0 || ts == i64::MIN {
        return "-".into();
    }
    fmt_when(ts, now)
}

/// Compact age: `35s`, `1m 39s`, `9h 47m`, or `3d 4h`.
pub(super) fn fmt_age(secs: i64) -> String {
    let secs = u64::try_from(secs.max(0)).unwrap_or(u64::MAX);
    let days = secs / 86_400;
    let hours = (secs % 86_400) / 3_600;
    let minutes = (secs % 3_600) / 60;
    let seconds = secs % 60;
    if days > 0 {
        format!("{days}d {hours}h")
    } else if hours > 0 {
        format!("{hours}h {minutes}m")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

/// File counts that always fit in six columns. Decimal steps, integer-rounded.
pub(super) fn fmt_files(n: u64) -> String {
    const STEPS: &[(u128, u128, u32, &str)] = &[
        (10_000, 1, 0, ""),
        (99_950, 1_000, 1, "k"),
        (999_500, 1_000, 0, "k"),
        (9_995_000, 1_000_000, 2, "M"),
        (99_950_000, 1_000_000, 1, "M"),
        (999_500_000, 1_000_000, 0, "M"),
        (9_995_000_000, 1_000_000_000, 2, "G"),
        (99_950_000_000, 1_000_000_000, 1, "G"),
        (999_500_000_000, 1_000_000_000, 0, "G"),
        (9_995_000_000_000, 1_000_000_000_000, 2, "T"),
        (99_950_000_000_000, 1_000_000_000_000, 1, "T"),
        (999_500_000_000_000, 1_000_000_000_000, 0, "T"),
        (9_995_000_000_000_000, 1_000_000_000_000_000, 2, "P"),
        (99_950_000_000_000_000, 1_000_000_000_000_000, 1, "P"),
        (999_500_000_000_000_000, 1_000_000_000_000_000, 0, "P"),
        (9_995_000_000_000_000_000, 1_000_000_000_000_000_000, 2, "E"),
        (
            99_950_000_000_000_000_000,
            1_000_000_000_000_000_000,
            1,
            "E",
        ),
        (
            999_500_000_000_000_000_000,
            1_000_000_000_000_000_000,
            0,
            "E",
        ),
    ];
    let value = u128::from(n);
    for &(below, scale, decimals, suffix) in STEPS {
        if value < below {
            return fmt_scaled(value, scale, decimals, suffix);
        }
    }
    fmt_scaled(value, 1_000_000_000_000_000_000, 1, "E")
}

fn fmt_scaled(value: u128, scale: u128, decimals: u32, suffix: &str) -> String {
    let pow = 10u128.pow(decimals);
    let rounded = (value * pow + scale / 2) / scale;
    if decimals == 0 {
        format!("{rounded}{suffix}")
    } else {
        let ip = rounded / pow;
        let fp = rounded % pow;
        format!("{ip}.{fp:0width$}{suffix}", width = decimals as usize)
    }
}

/// Cells of a LEVEL bar owned by each visible weight.
///
/// The denominator is `dir_charged`. Visible rows together own
/// `sum(weights) / dir_charged` of the bar; largest remainder hands out only
/// those cells. The rest of the track stays empty. When the weights already sum
/// to `dir_charged`, the bar fills. An empty directory is all zeros.
pub(super) fn level_cells(dir_charged: u64, weights: &[u64], width: usize) -> Vec<usize> {
    if dir_charged == 0 || width == 0 || weights.is_empty() {
        return vec![0; weights.len()];
    }
    let dir = u128::from(dir_charged);
    let width_u = u128::try_from(width).unwrap_or(u128::from(u16::MAX));
    let visible: u128 = weights.iter().map(|w| u128::from(*w)).sum();
    if visible == 0 {
        return vec![0; weights.len()];
    }
    let owned = usize::try_from(visible * width_u / dir)
        .unwrap_or(width)
        .min(width);
    let mut floors = Vec::with_capacity(weights.len());
    let mut remainders = Vec::with_capacity(weights.len());
    let mut assigned = 0usize;
    for (index, weight) in weights.iter().enumerate() {
        let product = u128::from(*weight) * width_u;
        let cells = usize::try_from(product / dir).unwrap_or(width).min(width);
        assigned = assigned.saturating_add(cells);
        floors.push(cells);
        remainders.push((product % dir, index));
    }
    if assigned > owned {
        return split_cells(weights, owned);
    }
    let mut left = owned - assigned;
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (_, index) in remainders {
        if left == 0 {
            break;
        }
        if floors[index] < width {
            floors[index] += 1;
            left -= 1;
        }
    }
    floors
}

/// Largest-remainder split of `seats` across `weights`. Used when a weight exceeds the directory.
fn split_cells(weights: &[u64], seats: usize) -> Vec<usize> {
    let total: u128 = weights.iter().map(|w| u128::from(*w)).sum();
    if total == 0 || seats == 0 {
        return vec![0; weights.len()];
    }
    let seats_u = u128::try_from(seats).unwrap_or(u128::from(u16::MAX));
    let mut floors = Vec::with_capacity(weights.len());
    let mut remainders = Vec::with_capacity(weights.len());
    let mut assigned = 0usize;
    for (index, weight) in weights.iter().enumerate() {
        let product = u128::from(*weight) * seats_u;
        let cells = usize::try_from(product / total).unwrap_or(seats).min(seats);
        assigned = assigned.saturating_add(cells);
        floors.push(cells);
        remainders.push((product % total, index));
    }
    let mut left = seats.saturating_sub(assigned.min(seats));
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (_, index) in remainders {
        if left == 0 {
            break;
        }
        if floors[index] < seats {
            floors[index] += 1;
            left -= 1;
        }
    }
    floors
}

pub(super) fn draw(frame: &mut Frame<'_>, b: &Browser) {
    let area = frame.area();
    paint_background(frame, area, b.theme);
    if area.width < 80 || area.height < 24 {
        draw_too_small(frame, area, b.theme);
        return;
    }
    let show_panels = area.height >= 30;
    let show_chart = chart_allowed(area);
    let footer_height = footer_height(b, usize::from(area.width));
    let chunks = regions(area, show_chart, show_panels, footer_height);
    let mut index = 0;
    if show_chart {
        let motif = if b.theme.glyphs.unicode {
            "▁▂▃▄▃▂▁ "
        } else {
            "~~~ ~~~ "
        };
        frame.render_widget(
            on_background(
                b.theme,
                Line::from(motif.repeat(usize::from(area.width).div_ceil(8)))
                    .style(style_fg(b.theme, Role::Wave)),
            ),
            chunks[index],
        );
        index += 1;
    }
    frame.render_widget(
        on_background(b.theme, header_line(b, usize::from(area.width))),
        chunks[index],
    );
    index += 1;
    if show_chart {
        draw_chart(frame, chunks[index], b);
        index += 1;
    }
    draw_body(frame, chunks[index], b);
    index += 1;
    frame.render_widget(
        on_background(b.theme, legend_line(b.theme, area.width)),
        chunks[index],
    );
    index += 1;
    if show_panels {
        draw_panels(frame, chunks[index], b);
        index += 1;
    }
    frame.render_widget(
        on_background(b.theme, footer_lines(b, usize::from(area.width))),
        chunks[index],
    );
    index += 1;
    if show_chart {
        let rule = if b.theme.glyphs.unicode { "▔" } else { "-" };
        frame.render_widget(
            on_background(
                b.theme,
                Line::from(rule.repeat(usize::from(area.width)))
                    .style(style_fg(b.theme, Role::Wave)),
            ),
            chunks[index],
        );
        index += 1;
    }
    let message = clip_cols(&message_line(b), usize::from(area.width));
    frame.render_widget(
        on_background(b.theme, Line::from(message).style(base_style(b.theme))),
        chunks[index],
    );
}

/// Paragraph whose every cell, including the unused tail of the row, carries the theme background.
fn on_background<'a>(theme: Theme, content: impl Into<ratatui::text::Text<'a>>) -> Paragraph<'a> {
    Paragraph::new(content).style(base_style(theme))
}

fn paint_background(frame: &mut Frame<'_>, area: Rect, theme: Theme) {
    frame.render_widget(Block::new().style(base_style(theme)), area);
}

fn times_mark(glyphs: Glyphs) -> &'static str {
    if glyphs.unicode {
        "×"
    } else {
        "x"
    }
}

fn delta_mark(glyphs: Glyphs) -> &'static str {
    if glyphs.unicode {
        "Δ"
    } else {
        "d"
    }
}

/// Footer, message, and no-match joiner. ASCII uses a hyphen, not `·`.
fn item_joiner(glyphs: Glyphs) -> &'static str {
    if glyphs.unicode {
        " · "
    } else {
        " - "
    }
}

fn draw_too_small(frame: &mut Frame<'_>, area: Rect, theme: Theme) {
    let times = times_mark(theme.glyphs);
    let text = format!(
        "terminal too small\nneed 80{times}24, have {}{times}{}\nminimum 80x24",
        area.width, area.height
    );
    let width = 36.min(area.width);
    let height = 5.min(area.height);
    let rect = center(area, width, height);
    frame.render_widget(
        Paragraph::new(text).block(pane_block(theme, Line::from("tide")).style(base_style(theme))),
        rect,
    );
}

fn regions(area: Rect, chart: bool, panels: bool, footer: u16) -> std::rc::Rc<[Rect]> {
    let mut constraints = Vec::new();
    if chart {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(1));
    if chart {
        constraints.push(Constraint::Length(9));
    }
    constraints.push(Constraint::Min(1));
    constraints.push(Constraint::Length(1));
    if panels {
        constraints.push(Constraint::Length(11));
    }
    constraints.push(Constraint::Length(footer));
    if chart {
        constraints.push(Constraint::Length(1));
    }
    constraints.push(Constraint::Length(1));
    Layout::vertical(constraints).split(area)
}

fn chart_allowed(area: Rect) -> bool {
    area.width >= 100 && area.height >= 34
}

fn base_style(theme: Theme) -> Style {
    match theme.level {
        Level::Mono => Style::default(),
        Level::Basic => Style::default()
            .fg(theme.color(Role::Text))
            .bg(Color::Reset),
        _ => Style::default()
            .fg(theme.color(Role::Text))
            .bg(theme.color(Role::Bg)),
    }
}

fn style_fg(theme: Theme, role: Role) -> Style {
    match theme.level {
        Level::Mono => Style::default(),
        _ => Style::default().fg(theme.color(role)),
    }
}

fn dim_style(theme: Theme) -> Style {
    style_fg(theme, Role::Dim).add_modifier(Modifier::DIM)
}

fn pane_block(theme: Theme, title: Line<'_>) -> Block<'_> {
    Block::bordered()
        .border_style(style_fg(theme, Role::Edge))
        .title_style(style_fg(theme, Role::Teal).add_modifier(Modifier::BOLD))
        .style(base_style(theme))
        .title(title)
}

fn header_line(b: &Browser, width: usize) -> Line<'static> {
    let theme = b.theme;
    let glyphs = theme.glyphs;
    let meta = b.snapshot.meta();
    let age = fmt_age(b.now.saturating_sub(meta.finished_at).max(0));
    let status = if meta.degraded { "degraded" } else { "steady" };
    let mode = crate::render::enum_label(meta.mode);
    let identity = format!(
        "{} {} {mode} {} {age} ago {} {status}",
        meta.scan_id, glyphs.sep, glyphs.sep, glyphs.sep
    );
    let high = high_water_text(&b.snapshots);
    let mut high_label = "| root high-water ";
    let mut right = format!("{high_label}{high}");
    let path = b.dir.path.display_lossy();
    if fit_header(width, &identity, &right, &path, glyphs)
        .1
        .is_none()
    {
        high_label = "hw ";
        right = format!("{high_label}{high}");
    }
    let (show_brand, path) = fit_header(width, &identity, &right, &path, glyphs);
    let mut spans = Vec::new();
    if show_brand {
        spans.push(Span::styled(
            glyphs.wave.to_string(),
            style_fg(theme, Role::Teal),
        ));
        spans.push(Span::styled(
            " tidemark  ".to_string(),
            style_fg(theme, Role::Text),
        ));
    }
    if let Some(path) = path {
        spans.push(Span::styled(path, style_fg(theme, Role::Text)));
        spans.push(Span::raw("  "));
    }
    spans.push(Span::styled(
        format!(
            "{} {} {mode} {} {age} ago {} ",
            meta.scan_id, glyphs.sep, glyphs.sep, glyphs.sep
        ),
        style_fg(theme, Role::Text),
    ));
    let status_style = if meta.degraded {
        style_fg(theme, Role::Coral).add_modifier(Modifier::BOLD)
    } else {
        style_fg(theme, Role::Teal)
    };
    spans.push(Span::styled(status.to_string(), status_style));
    let used = spans
        .iter()
        .map(|span| header_cols(span.content.as_ref()))
        .sum::<usize>();
    let gap = width.saturating_sub(used + header_cols(&right));
    spans.push(Span::raw(" ".repeat(gap)));
    spans.push(Span::styled(
        high_label.to_string(),
        style_fg(theme, Role::Dim),
    ));
    spans.push(Span::styled(
        high,
        style_fg(theme, Role::Sand).add_modifier(Modifier::BOLD),
    ));
    Line::from(spans).style(base_style(theme))
}

/// Header column budget. Ratatui draws U+00B7 and U+224B (≋) in one cell;
/// `table::display_width` counts every non-ASCII scalar as two.
pub(super) fn header_cols(text: &str) -> usize {
    let narrow = text
        .chars()
        .filter(|ch| matches!(ch, '\u{00B7}' | '\u{224B}'))
        .count();
    table::display_width(text).saturating_sub(narrow)
}

pub(super) fn fit_header(
    width: usize,
    identity: &str,
    right: &str,
    path: &str,
    glyphs: Glyphs,
) -> (bool, Option<String>) {
    let brand = header_cols(glyphs.wave) + header_cols(" tidemark  ");
    let gap = 1;
    let fixed = header_cols(identity) + gap + header_cols(right);
    let mut show_brand = true;
    let mut shown: Option<String> = Some(path.to_string());
    loop {
        let path_width = shown.as_ref().map_or(0, |text| header_cols(text) + 2);
        let brand_width = if show_brand { brand } else { 0 };
        if brand_width + path_width + fixed <= width {
            break;
        }
        if show_brand {
            show_brand = false;
            continue;
        }
        if let Some(text) = shown.as_ref() {
            // Two spaces follow the path. A fragment under 8 columns reads as a
            // bare dot, so the path is dropped instead of shortened that far.
            let leftover = width.saturating_sub(fixed + 2);
            if leftover >= 8 && header_cols(text) > leftover {
                let shortened = table::shorten(text, leftover);
                if header_cols(&shortened) < header_cols(text) {
                    shown = Some(shortened);
                    continue;
                }
            }
        }
        if shown.take().is_some() {
            continue;
        }
        break;
    }
    (show_brand, shown)
}

fn high_water_text(snapshots: &[CatalogRow]) -> String {
    match snapshots.iter().filter_map(|row| row.3).max() {
        Some(bytes) => size::bytes(bytes),
        None => "-".into(),
    }
}

fn legend_line(theme: Theme, width: u16) -> Line<'static> {
    let g = theme.glyphs;
    let text = if width >= 110 {
        format!(
            "{a} anchored by hard links outside   {w} scan errors   {up} rising   {down} ebbing   LEVEL = share of charged bytes",
            a = g.anchor,
            w = g.warn,
            up = g.rise,
            down = g.ebb
        )
    } else {
        format!(
            "{a} linked outside  {w} errors  {up} rising  {down} ebbing  LEVEL share of charged",
            a = g.anchor,
            w = g.warn,
            up = g.rise,
            down = g.ebb
        )
    };
    Line::from(Span::styled(
        clip_cols(&text, usize::from(width)),
        dim_style(theme),
    ))
    .style(base_style(theme))
}

fn footer_height(b: &Browser, width: usize) -> u16 {
    u16::try_from(footer_plain(b, width).len())
        .unwrap_or(1)
        .max(1)
}

fn footer_plain(b: &Browser, width: usize) -> Vec<String> {
    let joiner = item_joiner(b.theme.glyphs);
    if b.state.editing {
        return vec![format!("type filter{joiner}Enter finish{joiner}Esc finish")];
    }
    let g = b.theme.glyphs;
    let items = [
        format!("{} move", g.key_move),
        format!("{} descend", g.key_descend),
        format!("{} up", g.key_up),
        "s sort".to_string(),
        "/ filter".to_string(),
        "i info".to_string(),
        "f files".to_string(),
        "l links".to_string(),
        "p snapshots".to_string(),
        "R live".to_string(),
        "? help".to_string(),
        "q quit".to_string(),
    ];
    wrap_items(&items, width, joiner)
}

fn wrap_items(items: &[String], width: usize, joiner: &str) -> Vec<String> {
    let joiner_len = joiner.chars().count();
    let mut rows = Vec::new();
    let mut current = String::new();
    for item in items {
        let extra = item.chars().count() + usize::from(!current.is_empty()) * joiner_len;
        if !current.is_empty() && current.chars().count() + extra > width && rows.is_empty() {
            rows.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push_str(joiner);
        }
        current.push_str(item);
    }
    if !current.is_empty() {
        rows.push(current);
    }
    if rows.is_empty() {
        rows.push(String::new());
    }
    rows
}

fn footer_lines(b: &Browser, width: usize) -> Vec<Line<'static>> {
    let joiner = item_joiner(b.theme.glyphs);
    if b.state.editing {
        return vec![Line::from(vec![
            Span::styled("type filter".to_string(), dim_style(b.theme)),
            Span::styled(joiner.to_string(), dim_style(b.theme)),
            Span::styled("Enter".to_string(), key_style(b.theme)),
            Span::styled(" finish".to_string(), dim_style(b.theme)),
            Span::styled(joiner.to_string(), dim_style(b.theme)),
            Span::styled("Esc".to_string(), key_style(b.theme)),
            Span::styled(" finish".to_string(), dim_style(b.theme)),
        ])
        .style(base_style(b.theme))];
    }
    let g = b.theme.glyphs;
    let items = [
        (g.key_move, "move"),
        (g.key_descend, "descend"),
        (g.key_up, "up"),
        ("s", "sort"),
        ("/", "filter"),
        ("i", "info"),
        ("f", "files"),
        ("l", "links"),
        ("p", "snapshots"),
        ("R", "live"),
        ("?", "help"),
        ("q", "quit"),
    ];
    let plain = footer_plain(b, width);
    let mut lines = Vec::new();
    let mut remaining = items.as_slice();
    for row in &plain {
        let mut spans = Vec::new();
        let mut used = String::new();
        while let Some((key, verb)) = remaining.first() {
            let piece = format!("{key} {verb}");
            let next = if used.is_empty() {
                piece.clone()
            } else {
                format!("{used}{joiner}{piece}")
            };
            if !used.is_empty() && next.chars().count() > row.chars().count() {
                break;
            }
            if !used.is_empty() {
                spans.push(Span::styled(joiner.to_string(), dim_style(b.theme)));
            }
            spans.push(Span::styled((*key).to_string(), key_style(b.theme)));
            spans.push(Span::styled(format!(" {verb}"), dim_style(b.theme)));
            used = next;
            remaining = &remaining[1..];
        }
        lines.push(Line::from(spans).style(base_style(b.theme)));
    }
    lines
}

fn key_style(theme: Theme) -> Style {
    style_fg(theme, Role::Sand).add_modifier(Modifier::BOLD)
}

/// The short result comes before the Files disclaimer. `clip_cols` then
/// shortens the disclaimer before it cuts the result, so 80 columns keep
/// `Esc cancels` and `selected directory is gone`.
fn message_line(b: &Browser) -> String {
    const RUNNING: &str = "Live refresh running; Esc cancels";
    const FILES: &str = "Snapshot files; does not include changes since snapshot";
    const ALREADY: &str = "Live refresh already running";
    let on_files = b.state.pane == Pane::Files;
    let running = b.refresh.is_some();
    if !on_files && !running {
        return b.message.clone();
    }
    let mut parts = Vec::new();
    if running {
        parts.push(RUNNING);
    }
    let extra = b.message.as_str();
    let include_extra = if running {
        extra == ALREADY
    } else {
        on_files && !extra.is_empty()
    };
    if include_extra {
        parts.push(extra);
    }
    if on_files {
        parts.push(FILES);
    }
    parts.join(item_joiner(b.theme.glyphs))
}

fn draw_body(frame: &mut Frame<'_>, area: Rect, b: &Browser) {
    if b.state.pane == Pane::Main {
        if no_match(b) {
            let joiner = item_joiner(b.theme.glyphs);
            let text = if b.state.editing {
                format!(
                    "no matches for \"{}\" yet{joiner}Enter or Esc to finish",
                    super::escape(b.state.filter.as_bytes())
                )
            } else {
                format!(
                    "no matches for \"{}\"{joiner}Esc clears the filter",
                    super::escape(b.state.filter.as_bytes())
                )
            };
            // The block keeps the whole table. Blank lines put the sentence on the middle row.
            let inner = area.height.saturating_sub(2);
            let pad = usize::from(inner.saturating_sub(1) / 2);
            frame.render_widget(
                Paragraph::new(format!("{}{text}", "\n".repeat(pad)))
                    .centered()
                    .block(pane_block(b.theme, depth_title(b, usize::from(area.width)))),
                area,
            );
        } else {
            draw_table(frame, area, b);
        }
        return;
    }
    if b.state.pane == Pane::Picker {
        draw_picker(frame, area, b);
        return;
    }
    draw_text_list(frame, area, b);
}

fn no_match(b: &Browser) -> bool {
    !b.state.filter.is_empty() && b.rows.is_empty() && b.own_position() == usize::MAX
}

fn draw_table(frame: &mut Frame<'_>, area: Rect, b: &Browser) {
    let wide = area.width >= 120;
    let name_width = name_column_width(area.width, wide);
    let level_width = if wide { 12 } else { 6 };
    let visible = b.visible();
    let mut items: Vec<TableItem<'_>> = visible
        .iter()
        .map(|item| {
            let (row, delta) = &**item;
            TableItem::Child(row, *delta)
        })
        .collect();
    if b.own_position() != usize::MAX {
        items.insert(b.own_position(), TableItem::Own);
    }
    let weights: Vec<u64> = items
        .iter()
        .map(|item| match item {
            TableItem::Child(row, _) => row.t_charged,
            TableItem::Own => b.dir.own_charged,
        })
        .collect();
    let cells = level_cells(b.dir.t_charged, &weights, level_width);
    let rows: Vec<Row<'_>> = items
        .iter()
        .zip(cells)
        .map(|(item, level)| match item {
            TableItem::Child(row, delta) => data_row(
                b,
                row,
                *delta,
                (level, level_width),
                name_width,
                wide,
                false,
            ),
            TableItem::Own => own_row(b, level, level_width, name_width, wide),
        })
        .collect();
    let header = header_row(b, wide);
    let table = Table::new(rows, column_widths(wide))
        .header(header)
        .block(pane_block(b.theme, depth_title(b, usize::from(area.width))))
        .column_spacing(1)
        .style(base_style(b.theme))
        .row_highlight_style(row_highlight(b.theme))
        .highlight_symbol(Span::styled(
            format!("{} ", b.theme.glyphs.marker),
            style_fg(b.theme, Role::Teal),
        ));
    let mut state = TableState::default().with_selected(Some(b.state.selected));
    frame.render_stateful_widget(table, area, &mut state);
}

fn column_widths(wide: bool) -> Vec<Constraint> {
    if wide {
        vec![
            Constraint::Fill(1),
            Constraint::Length(12),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(9),
            Constraint::Length(7),
            Constraint::Length(11),
            Constraint::Length(11),
        ]
    } else {
        vec![
            Constraint::Fill(1),
            Constraint::Length(6),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(8),
            Constraint::Length(6),
            Constraint::Length(11),
            Constraint::Length(11),
        ]
    }
}

fn name_column_width(area_width: u16, wide: bool) -> usize {
    let inner = usize::from(area_width.saturating_sub(2));
    let highlight = 2;
    if wide {
        inner.saturating_sub(highlight + 77 + 8)
    } else {
        inner.saturating_sub(highlight + 58 + 7)
    }
}

enum TableItem<'a> {
    Child(&'a DirRow, Option<i128>),
    Own,
}

fn header_row(b: &Browser, wide: bool) -> Row<'static> {
    let mut labels = vec!["NAME", "LEVEL", "SIZE"];
    if wide {
        labels.push("APPARENT");
    }
    labels.extend(["RECLAIM", "SHARED", "FILES", "NEWEST", "TIDE"]);
    let active = sort_column(b.state.sort);
    let marker = if b.state.sort == super::state::Sort::Name {
        b.theme.glyphs.sort_asc
    } else {
        b.theme.glyphs.sort_desc
    };
    let marker_style = style_fg(b.theme, Role::Teal).remove_modifier(Modifier::DIM);
    let cells: Vec<Cell<'static>> = labels
        .into_iter()
        .map(|label| {
            let numeric = !matches!(label, "NAME" | "LEVEL");
            let line = if label == active {
                let mut line = Line::from(vec![
                    Span::styled(label.to_string(), dim_style(b.theme)),
                    Span::styled(marker.to_string(), marker_style),
                ]);
                if numeric {
                    line = line.right_aligned();
                }
                line
            } else {
                let text = label.to_string();
                if numeric {
                    Line::from(text).right_aligned()
                } else {
                    Line::from(text)
                }
            };
            Cell::from(line)
        })
        .collect();
    Row::new(cells).style(dim_style(b.theme))
}

fn sort_column(sort: super::state::Sort) -> &'static str {
    match sort {
        super::state::Sort::Name => "NAME",
        super::state::Sort::Size => "SIZE",
        super::state::Sort::Reclaim => "RECLAIM",
        super::state::Sort::Files => "FILES",
        super::state::Sort::Newest => "NEWEST",
        super::state::Sort::Growth => "TIDE",
    }
}

fn data_row(
    b: &Browser,
    row: &DirRow,
    delta: Option<i128>,
    level: (usize, usize),
    name_width: usize,
    wide: bool,
    own: bool,
) -> Row<'static> {
    let (level, level_width) = level;
    let name = if own {
        if name_width >= 15 {
            "./ (files here)".to_string()
        } else {
            ".".to_string()
        }
    } else {
        child_name(
            b.theme.glyphs,
            &row.name,
            row.t_disk > row.t_reclaimable,
            row.t_errors > 0,
            name_width,
        )
    };
    let mut cells = vec![
        Cell::from(name_spans(
            b.theme,
            &name,
            !own && row.t_disk > row.t_reclaimable,
            !own && row.t_errors > 0,
        )),
        Cell::from(level_bar(b.theme, level, level_width)),
        Cell::from(right_size(size::bytes(if own {
            b.dir.own_charged
        } else {
            row.t_disk
        }))),
    ];
    if wide {
        cells.push(Cell::from(right_size(if own {
            size::bytes(b.dir.own_apparent)
        } else {
            size::bytes(row.t_apparent)
        })));
    }
    cells.push(numeric_cell(
        b.theme,
        if own {
            "-".into()
        } else {
            size::bytes(row.t_reclaimable)
        },
        Role::Teal,
        !own && row.t_reclaimable > 0,
    ));
    let shared = row.t_disk.saturating_sub(row.t_reclaimable);
    cells.push(numeric_cell(
        b.theme,
        if own { "-".into() } else { size::bytes(shared) },
        Role::Sand,
        !own && shared > 0,
    ));
    cells.push(Cell::from(right_text(fmt_files(if own {
        b.dir.own_files
    } else {
        row.t_files
    }))));
    cells.push(Cell::from(right_text(if own {
        "-".into()
    } else {
        fmt_newest(row.newest_mtime, b.now)
    })));
    cells.push(Cell::from(if own {
        right_text("-".into())
    } else {
        tide_line(b.theme, delta)
    }));
    Row::new(cells)
}

fn own_row(
    b: &Browser,
    level: usize,
    level_width: usize,
    name_width: usize,
    wide: bool,
) -> Row<'static> {
    data_row(
        b,
        &b.dir,
        None,
        (level, level_width),
        name_width,
        wide,
        true,
    )
}

fn child_name(glyphs: Glyphs, name: &[u8], anchor: bool, warn: bool, width: usize) -> String {
    let mut reserve = 0;
    if anchor {
        reserve += table::display_width(" ") + table::display_width(glyphs.anchor);
    }
    if warn {
        reserve += table::display_width(" ") + table::display_width(glyphs.warn);
    }
    let text = format!("{}/", super::escape(name));
    table::shorten(&text, width.saturating_sub(reserve))
}

fn name_spans(theme: Theme, shortened: &str, anchor: bool, warn: bool) -> Line<'static> {
    let mut spans = vec![Span::styled(
        shortened.to_string(),
        style_fg(theme, Role::Text),
    )];
    if anchor {
        spans.push(Span::styled(
            format!(" {}", theme.glyphs.anchor),
            style_fg(theme, Role::Sand),
        ));
    }
    if warn {
        spans.push(Span::styled(
            format!(" {}", theme.glyphs.warn),
            style_fg(theme, Role::Coral),
        ));
    }
    Line::from(spans)
}

fn level_bar(theme: Theme, filled: usize, width: usize) -> Line<'static> {
    let filled = filled.min(width);
    Line::from(vec![
        Span::styled(
            theme.glyphs.level_on.repeat(filled),
            style_fg(theme, Role::Sea),
        ),
        Span::styled(
            theme.glyphs.level_off.repeat(width - filled),
            style_fg(theme, Role::Edge),
        ),
    ])
}

fn right_size(text: String) -> Line<'static> {
    Line::from(text).right_aligned()
}

fn right_text(text: String) -> Line<'static> {
    Line::from(text).right_aligned()
}

fn numeric_cell(theme: Theme, text: String, role: Role, colored: bool) -> Cell<'static> {
    let style = if colored {
        style_fg(theme, role)
    } else {
        style_fg(theme, Role::Text)
    };
    Cell::from(Line::from(Span::styled(text, style)).right_aligned())
}

fn tide_line(theme: Theme, delta: Option<i128>) -> Line<'static> {
    let Some(delta) = delta else {
        return right_text("-".into());
    };
    if delta == 0 {
        return Line::from(Span::styled(
            format!("0 {}", theme.glyphs.sep),
            dim_style(theme),
        ))
        .right_aligned();
    }
    let (glyph, role) = if delta > 0 {
        (theme.glyphs.rise, Role::Coral)
    } else {
        (theme.glyphs.ebb, Role::Teal)
    };
    Line::from(vec![
        Span::styled(
            format!("{} ", crate::render::tree::signed_bytes(delta)),
            style_fg(theme, Role::Text),
        ),
        Span::styled(glyph.to_string(), style_fg(theme, role)),
    ])
    .right_aligned()
}

pub(super) fn depth_title(b: &Browser, width: usize) -> Line<'static> {
    let glyphs = b.theme.glyphs;
    let sort = crate::render::enum_label(b.state.sort);
    let prefix = format!("depth {} ", glyphs.sep);
    let sort_part = format!(" {} sort {sort}", glyphs.sep);
    let live = live_suffix(b);
    let path = b.dir.path.display_lossy();
    if b.state.filter.is_empty() {
        let suffix = format!("{sort_part}{live}");
        let budget = width.saturating_sub(
            table::display_width(&prefix).saturating_add(table::display_width(&suffix)),
        );
        let path = table::shorten(&path, budget);
        return Line::from(clip_cols(&format!("{prefix}{path}{suffix}"), width));
    }
    // Sort and the live marker stay. A long path shortens before the filter is dropped.
    // A path that still fits beside a non-empty filter keeps its full text.
    let filter = super::escape(b.state.filter.as_bytes());
    let overhead = table::display_width(&format!(" {} filter \"\"", glyphs.sep));
    let fixed = table::display_width(&prefix)
        + table::display_width(&sort_part)
        + table::display_width(&live);
    let available = width.saturating_sub(fixed);
    let (path_shown, body_budget) = if available <= overhead {
        (table::shorten(&path, available), 0)
    } else {
        let room = available - overhead;
        let path_natural = table::display_width(&path);
        let body_natural = table::display_width(&filter);
        let mut body_budget = body_natural.min(room);
        let mut path_budget = room - body_budget;
        if path_natural < room && path_budget < path_natural {
            path_budget = path_natural;
            body_budget = room - path_natural;
        }
        // Both are too long for the row. Keep a short path and leave the filter the rest.
        if path_budget == 0 && path_natural > 0 && body_budget > 1 {
            let floor = 8.min(room.saturating_sub(1));
            if floor > 0 {
                path_budget = floor;
                body_budget = room - floor;
            }
        }
        (table::shorten(&path, path_budget), body_budget)
    };
    let body = table::shorten(&filter, body_budget);
    let filter_part = if body.is_empty() {
        String::new()
    } else {
        format!(" {} filter \"{body}\"", glyphs.sep)
    };
    Line::from(clip_cols(
        &format!("{prefix}{path_shown}{sort_part}{filter_part}{live}"),
        width,
    ))
}

fn live_suffix(b: &Browser) -> String {
    let glyphs = b.theme.glyphs;
    if b.refresh.is_some() {
        let frame = b.refresh_elapsed_ms / 100 % 4;
        let spinner = glyphs.spinner[usize::try_from(frame).unwrap_or(0)];
        format!(
            " {} live {} {spinner} {}s",
            glyphs.sep,
            glyphs.refresh,
            b.refresh_elapsed_ms / 1000
        )
    } else if b.pages.live.is_some() {
        format!(" {} live", glyphs.sep)
    } else {
        String::new()
    }
}

/// Exact zero-based eighth-cell scaling, clamped even if a caller supplies a smaller maximum.
pub(super) fn eighths(value: u64, maximum: u64, height: u16) -> u16 {
    if maximum == 0 {
        return 0;
    }
    let limit = u128::from(height) * 8;
    u16::try_from((u128::from(value) * limit / u128::from(maximum)).min(limit)).unwrap_or(u16::MAX)
}

/// One shallow, symmetric crest across the 16-column bar, bounded by its true top.
pub(super) fn crest(fill: u16, column: u16) -> u16 {
    let inset = match column.min(15) {
        0..=1 | 14..=15 => 2,
        2..=5 | 10..=13 => 1,
        _ => 0,
    };
    fill.saturating_sub(inset).max(1).min(fill)
}

struct ChartSample<'a> {
    id: Option<&'a str>,
    started: i64,
    sample: Option<super::history::Sample>,
    current: bool,
    live: bool,
}

fn draw_chart(frame: &mut Frame<'_>, area: Rect, b: &Browser) {
    use super::history::Sample;
    // Mock coordinates: box at column 1; first bar at column 4, stride 19.
    let area = Rect {
        x: area.x + 1,
        width: area.width.saturating_sub(2),
        ..area
    };
    let (subject, whole) = b.subject();
    let live_at = b.pages.live.as_ref().and(b.pages.live_at);
    let mut samples: Vec<_> = b
        .history
        .window
        .iter()
        .enumerate()
        .map(|(index, entry)| ChartSample {
            id: Some(entry.id.as_str()),
            started: entry.started,
            sample: b.history.samples.get(index).copied().flatten(),
            current: entry.id == b.snapshot.meta().scan_id.as_str(),
            live: false,
        })
        .collect();
    let insufficient_history = samples.len() < 2;
    if let Some(started) = live_at.filter(|_| !insufficient_history) {
        samples.push(ChartSample {
            id: None,
            started,
            sample: Some(Sample::Known(subject.t_disk)),
            current: false,
            live: true,
        });
    }
    let count = usize::from(area.width.saturating_sub(3) / 19).min(super::history::SAMPLES);
    let displayed = &samples[samples.len().saturating_sub(count)..];
    let delta = chart_delta(b, displayed);
    let delta_label = delta.map(|value| {
        let glyph = if value > 0 {
            b.theme.glyphs.rise
        } else if value < 0 {
            b.theme.glyphs.ebb
        } else {
            ""
        };
        let context = if live_at.is_some() {
            "vs snapshot"
        } else {
            "since previous"
        };
        let role = if value > 0 {
            Role::Coral
        } else if value < 0 {
            Role::Teal
        } else {
            Role::Dim
        };
        Line::from(Span::styled(
            format!(
                " {} {} {context} {glyph} ",
                delta_mark(b.theme.glyphs),
                crate::render::tree::signed_bytes(value)
            ),
            style_fg(b.theme, role),
        ))
    });
    let delta_width = delta_label.as_ref().map_or(0, |label| {
        label
            .spans
            .iter()
            .map(|span| table::display_width(&span.content))
            .sum::<usize>()
    });
    let context = if whole {
        format!(" {} whole directory", b.theme.glyphs.sep)
    } else {
        String::new()
    };
    let suffix = format!(
        " {} disk {}{context}{}",
        b.theme.glyphs.sep,
        size::bytes(subject.t_disk),
        if live_at.is_some() { " / live" } else { "" }
    );
    let prefix = format!("tide {} ", b.theme.glyphs.sep);
    let budget = usize::from(area.width).saturating_sub(
        delta_width + table::display_width(&prefix) + table::display_width(&suffix) + 3,
    );
    let title = format!(
        "{prefix}{}{suffix}",
        table::shorten(&subject.path.display_lossy(), budget)
    );
    let mut block = pane_block(b.theme, Line::from(title));
    if !b.theme.glyphs.unicode {
        block = block.border_set(ratatui::symbols::border::Set {
            top_left: "+",
            top_right: "+",
            bottom_left: "+",
            bottom_right: "+",
            vertical_left: "|",
            vertical_right: "|",
            horizontal_top: "-",
            horizontal_bottom: "-",
        });
    }
    if let Some(label) = delta_label {
        block = block.title_top(label.right_aligned());
    }
    if let Some(reason) = displayed.iter().find_map(|s| match s.sample {
        Some(Sample::Unavailable(reason)) => Some(reason),
        _ => None,
    }) {
        block = block.title_bottom(
            Line::from(format!(" n/a: {reason} "))
                .style(dim_style(b.theme))
                .right_aligned(),
        );
    } else if displayed
        .iter()
        .any(|s| s.sample == Some(Sample::MissingPath))
    {
        block = block.title_bottom(
            Line::from(" absent path ")
                .style(dim_style(b.theme))
                .right_aligned(),
        );
    }
    frame.render_widget(block, area);
    if insufficient_history {
        let text = format!(
            "Insufficient history · {} disk",
            size::bytes(subject.t_disk)
        );
        let text = if b.theme.glyphs.unicode {
            text
        } else {
            text.replace('·', ".")
        };
        frame.render_widget(
            on_background(b.theme, text),
            Rect {
                x: area.x + 3,
                y: area.y + 2,
                width: area.width.saturating_sub(6),
                height: 1,
            },
        );
        return;
    }
    let height = area.height.saturating_sub(4);
    let maximum = displayed
        .iter()
        .filter_map(|s| s.sample.and_then(Sample::value))
        .max()
        .unwrap_or(0);
    if height == 0 {
        return;
    }
    // Zero stays at the foot of the plot, separate from the value labels/border.
    frame.buffer_mut()[(area.x + 1, area.y + height)]
        .set_symbol("0")
        .set_style(dim_style(b.theme));
    let baseline = chart_baseline(b, displayed);
    for (index, sample) in displayed.iter().enumerate() {
        let x = area.x + 3 + u16::try_from(index).unwrap_or(0) * 19;
        let value = sample.sample.and_then(Sample::value);
        let e = eighths(value.unwrap_or(0), maximum, height);
        let latest = index + 1 == displayed.len();
        let color = if latest { Role::Foam } else { Role::Teal };
        for row in 0..e.div_ceil(8) {
            let fill = (e - row * 8).min(8);
            let top = row + 1 == e.div_ceil(8);
            let y = area.y + height - row;
            for column in 0..16 {
                let level = if top { crest(fill, column) } else { fill };
                let glyph = if b.theme.glyphs.unicode {
                    [" ", "▁", "▂", "▃", "▄", "▅", "▆", "▇", "█"][usize::from(level)]
                } else if level < 4 {
                    "."
                } else if top {
                    "~"
                } else {
                    "#"
                };
                let mut style = style_fg(b.theme, color);
                if latest && matches!(b.theme.level, Level::Basic | Level::Mono) {
                    style = style.add_modifier(Modifier::BOLD);
                }
                frame.buffer_mut()[(x + column, y)]
                    .set_symbol(glyph)
                    .set_style(style);
            }
        }
        if latest {
            if let Some(previous) = baseline {
                let previous_eighths = eighths(previous, maximum, height);
                // A side tick leaves the crest intact, including when the baseline
                // is above a declining endpoint. Zero occupies the bottom plot row.
                let y = area.y + height - previous_eighths.saturating_sub(1) / 8;
                frame.buffer_mut()[(x - 1, y)]
                    .set_symbol(if b.theme.glyphs.unicode { "─" } else { "-" })
                    .set_style(dim_style(b.theme));
            }
        }
        let (_, month, day, hour, _) = civil(sample.started);
        let marker = if sample.live {
            if b.theme.glyphs.unicode {
                "◆"
            } else {
                "+"
            }
        } else if sample.current {
            b.theme.glyphs.open
        } else {
            ""
        };
        let stamp = if sample.live {
            format!("{marker} live")
        } else if sample.current {
            format!("{marker} {month:02}-{day:02} {hour:02}h")
        } else {
            format!("{month:02}-{day:02} {hour:02}h")
        };
        let label = match sample.sample {
            Some(Sample::Known(value)) => size::bytes(value),
            Some(Sample::MissingPath) => if b.theme.glyphs.unicode { "—" } else { "-" }.into(),
            Some(Sample::Unavailable(_)) => "n/a".into(),
            None => if b.theme.glyphs.unicode { "…" } else { "..." }.into(),
        };
        let style = if latest {
            key_style(b.theme)
        } else {
            style_fg(b.theme, Role::Dim)
        };
        for (offset, text) in [stamp, label].into_iter().enumerate() {
            frame.render_widget(
                Paragraph::new(text)
                    .centered()
                    .style(base_style(b.theme).patch(style)),
                Rect {
                    x,
                    y: area.y + height + 1 + offset as u16,
                    width: 16,
                    height: 1,
                },
            );
        }
    }
}

fn chart_delta(b: &Browser, samples: &[ChartSample<'_>]) -> Option<i128> {
    let latest = samples.last()?.sample?.value()?;
    Some(i128::from(latest) - i128::from(chart_baseline(b, samples)?))
}

/// Use the same known, displayed predecessor for both the tick and delta.
fn chart_baseline(b: &Browser, samples: &[ChartSample<'_>]) -> Option<u64> {
    if samples.last()?.live {
        return samples.iter().find(|s| s.current)?.sample?.value();
    }
    let meta = b.snapshot.meta();
    let previous = super::previous_complete(&b.snapshots, meta.started_at, meta.scan_id.as_str())?;
    samples
        .iter()
        .find(|s| s.id == Some(previous.0.as_str()))?
        .sample?
        .value()
}

fn draw_panels(frame: &mut Frame<'_>, area: Rect, b: &Browser) {
    let chunks =
        Layout::horizontal([Constraint::Percentage(50), Constraint::Percentage(50)]).split(area);
    let (label, row) = panel_subject(b);
    frame.render_widget(
        strata_paragraph(b, &label, &row, usize::from(chunks[0].width)),
        chunks[0],
    );
    let label = if matches!(row, PanelRow::Own) {
        format!("{} (files here)", b.dir.path.display_lossy())
    } else {
        label
    };
    let soundings_width = usize::from(chunks[1].width);
    frame.render_widget(
        soundings_paragraph(
            b,
            &label,
            &row,
            soundings_width.saturating_sub(2),
            soundings_width,
        ),
        chunks[1],
    );
}

enum PanelRow<'a> {
    Own,
    Child(&'a DirRow),
    Dir,
}

fn panel_subject(b: &Browser) -> (String, PanelRow<'_>) {
    let own = b.own_position();
    if own != usize::MAX && b.main_selected == own {
        return (
            format!("{} (whole dir)", b.dir.path.display_lossy()),
            PanelRow::Own,
        );
    }
    if let Some((row, _)) = b.child_at(b.main_selected) {
        return (
            format!("{}/", super::escape(&row.name)),
            PanelRow::Child(row),
        );
    }
    (
        format!("{} (whole dir)", b.dir.path.display_lossy()),
        PanelRow::Dir,
    )
}

fn strata_paragraph(
    b: &Browser,
    label: &str,
    row: &PanelRow<'_>,
    pane_width: usize,
) -> Paragraph<'static> {
    let source = match row {
        PanelRow::Child(row) => *row,
        PanelRow::Own | PanelRow::Dir => &b.dir,
    };
    let buckets = [
        source.age0,
        source.age1,
        source.age2,
        source.age3,
        source.age4,
        source.age5,
    ];
    let seabed = if b.theme.glyphs.unicode {
        "≥365d"
    } else {
        ">=365d"
    };
    let labels = ["<1d", "1-7d", "7-30d", "30-90d", "90-365d", seabed];
    let max_bucket = buckets.iter().copied().max().unwrap_or(0).max(1);
    let track = 16usize;
    let mut lines = Vec::new();
    for (index, (name, bucket)) in labels.iter().zip(buckets).enumerate() {
        let track_u = u128::try_from(track).unwrap_or(u128::from(u16::MAX));
        let filled = usize::try_from(u128::from(bucket) * track_u / u128::from(max_bucket))
            .unwrap_or(0)
            .min(track);
        lines.push(Line::from(vec![
            Span::styled(format!("{name:<8} "), dim_style(b.theme)),
            Span::styled(
                b.theme.glyphs.bar.repeat(filled),
                Style::default().fg(b.theme.strata(index)),
            ),
            Span::styled(
                b.theme.glyphs.level_off.repeat(track - filled),
                dim_style(b.theme),
            ),
            Span::styled(
                format!(" {}", size::bytes(bucket)),
                style_fg(b.theme, Role::Text),
            ),
        ]));
    }
    lines.push(Line::from(Span::styled(
        format!("surface = newest {} seabed = {seabed}", b.theme.glyphs.sep),
        dim_style(b.theme),
    )));
    lines.push(Line::from(Span::styled(
        format!("errors {}", source.t_errors),
        if source.t_errors > 0 {
            style_fg(b.theme, Role::Coral)
        } else {
            dim_style(b.theme)
        },
    )));
    Paragraph::new(lines).block(pane_block(
        b.theme,
        Line::from(panel_heading(
            "strata",
            b.theme.glyphs.sep,
            label,
            pane_width,
        )),
    ))
}

fn soundings_paragraph(
    b: &Browser,
    label: &str,
    row: &PanelRow<'_>,
    inner_width: usize,
    pane_width: usize,
) -> Paragraph<'static> {
    let (files, inodes, dirs, disk, reclaim, shared) = match row {
        PanelRow::Own => (
            fmt_files(b.dir.own_files),
            "-".into(),
            "-".into(),
            size::bytes(b.dir.own_charged),
            "-".into(),
            "-".into(),
        ),
        PanelRow::Child(row) => sounding_values(row),
        PanelRow::Dir => sounding_values(&b.dir),
    };
    let mut lines = vec![
        metric_line(b.theme, "files", &files, Role::Text, false),
        metric_line(b.theme, "inodes", &inodes, Role::Text, false),
        metric_line(b.theme, "dirs", &dirs, Role::Text, false),
        metric_line(b.theme, "disk", &disk, Role::Text, false),
        metric_line(b.theme, "reclaim", &reclaim, Role::Teal, reclaim != "-"),
        metric_line(b.theme, "shared", &shared, Role::Sand, shared != "-"),
    ];
    match row {
        PanelRow::Own => {
            lines.push(Line::from(Span::styled(
                "reclaimable if removed  -",
                dim_style(b.theme),
            )));
            lines.push(Line::from(Span::styled(
                "kept alive elsewhere  -",
                dim_style(b.theme),
            )));
        }
        PanelRow::Child(row) => {
            lines.extend(share_bars(
                b.theme,
                row.t_disk,
                row.t_reclaimable,
                inner_width,
            ));
        }
        PanelRow::Dir => lines.extend(share_bars(
            b.theme,
            b.dir.t_disk,
            b.dir.t_reclaimable,
            inner_width,
        )),
    }
    Paragraph::new(lines).block(pane_block(
        b.theme,
        Line::from(panel_heading(
            "soundings",
            b.theme.glyphs.sep,
            label,
            pane_width,
        )),
    ))
}

/// Panel block title clipped to the pane, leaving the border a column on each side.
pub(super) fn panel_heading(prefix: &str, sep: &str, label: &str, pane_width: usize) -> String {
    clip_cols(
        &format!("{prefix} {sep} {label}"),
        pane_width.saturating_sub(2),
    )
}

fn sounding_values(row: &DirRow) -> (String, String, String, String, String, String) {
    let shared = row.t_disk.saturating_sub(row.t_reclaimable);
    (
        fmt_files(row.t_files),
        row.t_inodes.to_string(),
        row.t_dirs.to_string(),
        size::bytes(row.t_disk),
        size::bytes(row.t_reclaimable),
        size::bytes(shared),
    )
}

fn metric_line(theme: Theme, label: &str, value: &str, role: Role, colored: bool) -> Line<'static> {
    Line::from(vec![
        Span::styled(format!("{label:<8} "), dim_style(theme)),
        Span::styled(
            value.to_string(),
            if colored {
                style_fg(theme, role)
            } else {
                style_fg(theme, Role::Text)
            },
        ),
    ])
}

fn share_bars(theme: Theme, disk: u64, reclaim: u64, inner_width: usize) -> Vec<Line<'static>> {
    let shared = disk.saturating_sub(reclaim);
    vec![
        percent_line(
            theme,
            "reclaimable if removed",
            reclaim,
            disk,
            Role::Teal,
            inner_width,
        ),
        percent_line(
            theme,
            "kept alive elsewhere",
            shared,
            disk,
            Role::Sand,
            inner_width,
        ),
    ]
}

fn percent_line(
    theme: Theme,
    label: &str,
    part: u64,
    total: u64,
    role: Role,
    inner_width: usize,
) -> Line<'static> {
    let pct = u64::try_from((100 * u128::from(part)) / u128::from(total.max(1)))
        .unwrap_or(100)
        .min(100);
    let prefix = format!("{label} {pct}% ");
    let width = inner_width
        .saturating_sub(table::display_width(&prefix))
        .min(12);
    let cells = usize::try_from(pct).unwrap_or(0) * width / 100;
    Line::from(vec![
        Span::styled(prefix, dim_style(theme)),
        Span::styled(theme.glyphs.bar.repeat(cells), style_fg(theme, role)),
    ])
}

fn draw_picker(frame: &mut Frame<'_>, area: Rect, b: &Browser) {
    let open = b.snapshot.meta().scan_id.as_str();
    let items: Vec<ListItem<'_>> = b
        .snapshots
        .iter()
        .map(|row| {
            let mark = if row.0 == open {
                b.theme.glyphs.open
            } else {
                " "
            };
            // Same-year stamps are 11 columns and older stamps are 10. Pad so the total lines up.
            let when = format!("{:<11}", fmt_when(row.2, b.now));
            let total = row.3.map_or_else(|| "-".into(), size::bytes);
            ListItem::new(format!("{mark}  {}  {when}  {total}", row.0))
        })
        .collect();
    render_list(frame, area, b, items, true, None);
}

fn draw_text_list(frame: &mut Frame<'_>, area: Rect, b: &Browser) {
    let width = usize::from(area.width).saturating_sub(4);
    // Links and `f` Files keep the column title out of the selectable rows.
    let header = b.column_header().map(str::to_string);
    let items = b
        .text
        .lines()
        .skip(usize::from(header.is_some()))
        .map(|line| ListItem::new(table::shorten(line, width)))
        .collect();
    let cursor = !matches!(b.state.pane, Pane::Info | Pane::Help);
    render_list(frame, area, b, items, cursor, header.as_deref());
}

fn render_list(
    frame: &mut Frame<'_>,
    area: Rect,
    b: &Browser,
    items: Vec<ListItem<'_>>,
    cursor: bool,
    header: Option<&str>,
) {
    let title = list_title(b, usize::from(area.width));
    let symbol = if cursor {
        format!("{} ", b.theme.glyphs.marker)
    } else {
        String::new()
    };
    let list = List::new(items)
        .style(base_style(b.theme))
        .highlight_style(if cursor {
            row_highlight(b.theme)
        } else {
            Style::default()
        })
        .highlight_symbol(symbol);
    let mut state = ListState::default().with_selected(Some(b.state.selected));
    let Some(header) = header else {
        let list = list.block(pane_block(b.theme, title));
        frame.render_stateful_widget(list, area, &mut state);
        return;
    };
    let block = pane_block(b.theme, title);
    let inner = block.inner(area);
    frame.render_widget(block, area);
    if inner.width == 0 || inner.height == 0 {
        return;
    }
    // The list reserves the two-cell highlight (`▶ ` or `> `) on every row.
    let padded = format!("  {header}");
    let head = Rect { height: 1, ..inner };
    frame.render_widget(
        Paragraph::new(Line::from(Span::styled(
            table::shorten(&padded, usize::from(inner.width)),
            dim_style(b.theme),
        )))
        .style(base_style(b.theme)),
        head,
    );
    if inner.height == 1 {
        return;
    }
    let list_area = Rect {
        y: inner.y.saturating_add(1),
        height: inner.height - 1,
        ..inner
    };
    frame.render_stateful_widget(list, list_area, &mut state);
}

pub(super) fn list_title(b: &Browser, width: usize) -> Line<'static> {
    let glyphs = b.theme.glyphs;
    let path = if b.state.pane == Pane::Info {
        b.subject().0.path.display_lossy()
    } else {
        b.dir.path.display_lossy()
    };
    if b.state.pane == Pane::Files {
        let meta = b.snapshot.meta();
        let suffix = if b.refresh.is_some() || b.pages.live.is_some() {
            live_suffix(b)
        } else {
            String::new()
        };
        let base = format!(
            "Files | snapshot {} at {}",
            meta.scan_id,
            size::timestamp(meta.finished_at)
        );
        let budget = width.saturating_sub(2 + table::display_width(&suffix));
        let line = format!("{}{suffix}", clip_cols(&base, budget));
        return Line::from(clip_cols(&line, width));
    }
    let label = crate::render::enum_label(b.state.pane);
    let prefix = format!("{label} {} ", glyphs.sep);
    let budget = width.saturating_sub(table::display_width(&prefix));
    let path = table::shorten(&path, budget);
    Line::from(clip_cols(&format!("{prefix}{path}"), width))
}

fn row_highlight(theme: Theme) -> Style {
    if theme.level == Level::Mono {
        Style::default().add_modifier(Modifier::REVERSED)
    } else {
        Style::default()
            .bg(theme.color(Role::Deep))
            .add_modifier(Modifier::BOLD)
    }
}

fn center(area: Rect, width: u16, height: u16) -> Rect {
    let width = width.min(area.width);
    let height = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    }
}

fn clip_cols(text: &str, width: usize) -> String {
    if table::display_width(text) <= width {
        return text.to_string();
    }
    let mut out = String::new();
    for ch in text.chars() {
        let next = if ch.is_ascii() { 1 } else { 2 };
        if table::display_width(&out) + next > width {
            break;
        }
        out.push(ch);
    }
    out
}
