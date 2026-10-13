//! Owned by: W7-A
//! Deterministic terminal drawing without filesystem or clock access.

use super::{Browser, Pane};
use crate::render::{size, table};
use ratatui::{
    layout::{Constraint, Layout},
    style::{Modifier, Style},
    widgets::{Block, List, ListItem, ListState, Paragraph},
    Frame,
};

pub(super) fn draw(frame: &mut Frame<'_>, b: &Browser) {
    let area = frame.area();
    if area.width < 80 || area.height < 24 {
        frame.render_widget(Paragraph::new("terminal too small (minimum 80x24)"), area);
        return;
    }
    let chunks = Layout::vertical([
        Constraint::Length(4),
        Constraint::Min(1),
        Constraint::Length(4),
    ])
    .split(area);
    let meta = b.snapshot.meta();
    let header = format!(
        "{} {:?} age {}s degraded={}\n{}",
        super::escape(meta.scan_id.to_string().as_bytes()),
        meta.mode,
        b.now.saturating_sub(meta.finished_at).max(0),
        meta.degraded,
        b.dir.path.display_lossy()
    );
    frame.render_widget(Paragraph::new(header).block(Block::bordered()), chunks[0]);
    let title = format!(
        "{:?} | {:?} | filter {}",
        b.state.pane,
        b.state.sort,
        super::escape(b.state.filter.as_bytes())
    );
    let items: Vec<ListItem<'_>> = match b.state.pane {
        Pane::Main => {
            let name_width = usize::from(area.width).saturating_sub(66).max(12);
            let mut lines = Vec::new();
            for (row, delta) in b.visible() {
                let name = format!(
                    "{}/{}{}",
                    super::escape(&row.name),
                    if row.t_disk > row.t_reclaimable {
                        "H"
                    } else {
                        ""
                    },
                    if row.t_errors > 0 { "!" } else { "" }
                );
                let share = (u128::from(row.t_charged) * 5 / u128::from(b.dir.t_charged.max(1)))
                    .min(5) as usize;
                lines.push(ListItem::new(format!(
                    "{:<name_width$} {:5} {:>7} {:>7} {:>7} {:>6} {:>10} {}",
                    table::shorten(&name, name_width),
                    "#".repeat(share),
                    size::bytes(row.t_disk),
                    size::bytes(row.t_reclaimable),
                    size::bytes(row.t_disk.saturating_sub(row.t_reclaimable)),
                    row.t_files,
                    row.newest_mtime,
                    delta
                        .map(crate::render::tree::signed_bytes)
                        .unwrap_or_else(|| "-".into())
                )));
            }
            let own_share = (u128::from(b.dir.own_charged) * 5 / u128::from(b.dir.t_charged.max(1)))
                .min(5) as usize;
            lines.push(ListItem::new(format!(
                "{:<name_width$} {:5} {:>7} {:>7} {:>7} {:>6} {:>10} -",
                "[own files]",
                "#".repeat(own_share),
                size::bytes(b.dir.own_charged),
                "-",
                "-",
                b.dir.own_files,
                "-"
            )));
            lines
        }
        Pane::Picker => b
            .snapshots
            .iter()
            .map(|(id, _, _)| ListItem::new(super::escape(id.as_bytes())))
            .collect(),
        _ => b
            .text
            .lines()
            .map(|line| {
                ListItem::new(table::shorten(
                    line,
                    usize::from(area.width).saturating_sub(2),
                ))
            })
            .collect(),
    };
    let title = if b.pages.live.is_some() {
        format!("{title} | live (Δ snapshot)")
    } else {
        title
    };
    let title = if b.state.pane == Pane::Main {
        format!("{title} | Name Share SIZE RECLAIM SHARED FILES NEWEST Δ")
    } else {
        title
    };
    let list = List::new(items)
        .block(Block::bordered().title(title))
        .highlight_style(Style::default().add_modifier(Modifier::REVERSED))
        .highlight_symbol("> ");
    let mut selection = ListState::default().with_selected(Some(b.state.selected));
    frame.render_stateful_widget(list, chunks[1], &mut selection);
    frame.render_widget(
        Paragraph::new(format!(
            "{}\nq quit | ? help | PgDn next page | {}",
            b.message,
            if b.state.editing {
                "type filter, Enter finish"
            } else {
                "read-only"
            }
        ))
        .block(Block::bordered()),
        chunks[2],
    );
}
