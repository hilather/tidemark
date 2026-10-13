//! Owned by: W7-A
//! Bounded snapshot browser, input driver and terminal restoration.

mod draw;
mod state;
mod terminal;
use crossterm::event::{self, Event, KeyCode, KeyEventKind};
use state::{Pane, State};
use tidemark_core::snapshot::DirRow;
use tidemark_store::reader::Snapshot;

const PAGE: usize = 1_000;
struct Browser {
    snapshot: Snapshot,
    old: Option<Snapshot>,
    dir: DirRow,
    state: State,
    rows: Vec<(DirRow, Option<i128>)>,
    text: String,
    links: Vec<u64>,
    snapshots: Vec<(String, std::path::PathBuf, i64)>,
    message: String,
    now: i64,
}
impl Browser {
    fn load(&mut self) -> anyhow::Result<()> {
        self.rows.clear();
        let mut cursor = self.state.page_start;
        // Preorder IDs and subtree_end are frozen store contracts. Each indexed
        // lookup skips an entire descendant subtree; never load all children.
        for _ in 0..PAGE {
            if cursor > self.dir.subtree_end {
                break;
            }
            let row = self.snapshot.dir(cursor)?;
            cursor = row.subtree_end.saturating_add(1);
            if row.parent_id != Some(self.dir.id) {
                return Err(anyhow::anyhow!("invalid child preorder range"));
            }
            let delta = self
                .old
                .as_ref()
                .map(|old| {
                    old.dir_by_path(&row.path).map(|previous| {
                        i128::from(row.t_disk) - i128::from(previous.map(|r| r.t_disk).unwrap_or(0))
                    })
                })
                .transpose()?;
            self.rows.push((row, delta));
        }
        self.state.next = (cursor <= self.dir.subtree_end).then_some(cursor);
        self.state.order(&mut self.rows);
        self.state.selected = 0;
        Ok(())
    }
    fn visible(&self) -> Vec<&(DirRow, Option<i128>)> {
        self.rows
            .iter()
            .filter(|(r, _)| r.path.display_lossy().contains(&self.state.filter))
            .collect()
    }
    fn count(&self) -> usize {
        match self.state.pane {
            Pane::Main => self.visible().len() + 1,
            Pane::Links => self.links.len(),
            Pane::Picker => self.snapshots.len(),
            _ => self.text.lines().count(),
        }
    }
    fn navigate(&mut self, dir: DirRow) -> anyhow::Result<()> {
        self.state.reset(dir.id.saturating_add(1));
        self.dir = dir;
        self.load()
    }
    fn pane(&mut self, pane: Pane) -> anyhow::Result<()> {
        self.state.pane = pane;
        self.state.selected = 0;
        self.text.clear();
        match pane {
            Pane::Info => {
                let r=&self.dir;
                self.text=format!("{}\nFiles {}  Inodes {}  Directories {}\nDisk {}  Reclaim {}  Shared {}\nAge histogram (charged bytes):\n<1d {}  1-7d {}  7-30d {}\n30-90d {}  90-365d {}  >=365d {}\nErrors {}\n",
                    r.path.display_lossy(),r.t_files,r.t_inodes,r.t_dirs,
                    crate::render::size::bytes(r.t_disk),crate::render::size::bytes(r.t_reclaimable),
                    crate::render::size::bytes(r.t_disk.saturating_sub(r.t_reclaimable)),
                    r.age0,r.age1,r.age2,r.age3,r.age4,r.age5,r.t_errors);
                for (key,value) in self.snapshot.annotations(r.id)? {
                    self.text.push_str(&format!("{}: {}\n",escape(key.as_bytes()),escape(value.as_bytes())));
                }
                for error in self.snapshot.errors_under(r.id,100)? {
                    self.text.push_str(&format!("{}\n",escape(format!("{error:?}").as_bytes())));
                }
                if let Ok(report)=tidemark_store::links::link_report(&self.snapshot,&r.path,None,0) {
                    self.text.push_str(&format!("Link totals: {} sets, {} instances, {} bytes; incomplete {}\n",
                        report.totals.sets,report.totals.instances,report.totals.disk,report.totals.incomplete_sets));
                } else { self.text.push_str("Link totals unavailable (instance storage required)\n"); }
            }
            Pane::Files => {
                let hits=self.snapshot.subtree_files(self.dir.id,&Default::default(),100)?;
                self.text=hits.iter().map(|hit|format!("{} {} {}\n",
                    crate::render::size::bytes(hit.row.disk),hit.row.mtime,
                    hit.path.as_ref().map(|p|p.display_lossy()).unwrap_or_else(||"[unresolved]".into()))).collect();
            }
            Pane::Links => {
                let hits=self.snapshot.link_sets_under(self.dir.id,&Default::default(),100)?;
                self.links=hits.iter().map(|h|h.row.ino).collect();
                self.text=hits.iter().map(|h|format!("{} {} H {} instances {}\n",h.row.ino,
                    crate::render::size::bytes(h.row.disk),h.instance_count,h.lca_path.display_lossy())).collect();
            }
            Pane::Help => self.text="Arrows move / descend / ascend; Enter descends\nPgDn next page; PgUp previous page; Home/End within page\ns sort page: size/reclaim/files/newest/growth/name\ni info; l links (Enter instances); f files; p snapshots\n/ filter current page; Escape clear/close pane; ? help; q quit\nRead-only browser: no delete, move, open or shell actions\nPages contain at most 1,000 direct children.\nR live refresh unavailable: shared helper is private.".into(),
            _ => {}
        }
        Ok(())
    }
    fn enter(&mut self) -> anyhow::Result<()> {
        match self.state.pane {
            Pane::Main => {
                if let Some((row, _)) = self.visible().get(self.state.selected).copied() {
                    let row = row.clone();
                    self.navigate(row)?;
                }
            }
            Pane::Links => {
                if let Some(ino) = self.links.get(self.state.selected) {
                    let mut lines = Vec::new();
                    for row in self.snapshot.link_instances(*ino)?.into_iter().take(1_000) {
                        let dir = self.snapshot.dir(row.dir_id)?;
                        let path = row
                            .name
                            .as_ref()
                            .map(|name| dir.path.join(name).display_lossy())
                            .unwrap_or_else(|| {
                                format!("{}/[unresolved name]", dir.path.display_lossy())
                            });
                        lines.push(path);
                    }
                    self.text = lines.join("\n");
                    self.message =
                        "First 1,000 stored instances; unresolved names are never looked up live"
                            .into();
                    self.state.pane = Pane::Instances;
                    self.state.selected = 0;
                }
            }
            Pane::Picker => {
                if let Some((_, file, _)) = self.snapshots.get(self.state.selected) {
                    let snapshot = Snapshot::open(file)?;
                    if let Some(dir) = snapshot.dir_by_path(&self.dir.path)? {
                        let old = self
                            .snapshots
                            .iter()
                            .filter(|(_, _, started)| *started < snapshot.meta().started_at)
                            .max_by_key(|(_, _, started)| *started)
                            .map(|(_, file, _)| Snapshot::open(file))
                            .transpose()?;
                        self.snapshot = snapshot;
                        self.old = old;
                        self.navigate(dir)?;
                        self.message =
                            "Snapshot switched; comparing with preceding complete snapshot".into();
                    } else {
                        self.message = "Path absent in selected snapshot".into();
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }
}
fn escape(bytes: &[u8]) -> String {
    tidemark_core::path::display_lossy_bytes(bytes)
}

pub(crate) fn run(args: &crate::cmd::tui::Args, context: &crate::Context) -> anyhow::Result<()> {
    use std::io::IsTerminal;
    if !std::io::stdout().is_terminal() || !std::io::stdin().is_terminal() {
        return Err(crate::CliError::Configuration(
            "tui requires terminal input and output".into(),
        )
        .into());
    }
    let (path, snapshot) =
        crate::cmd::snapshots::select(context, args.path.as_deref(), args.snapshot.as_deref())?;
    let dir = snapshot
        .dir_by_path(&path)?
        .ok_or_else(|| crate::CliError::Precondition("path not present in snapshot".into()))?;
    let catalog = tidemark_store::catalog::Catalog::open(&context.config.general.state_dir, false)?;
    let mut entries = catalog.list(Some(&snapshot.meta().root))?;
    entries.retain(|e| e.status == "complete" && e.file.is_some());
    entries.sort_by(|a, b| {
        b.started_at
            .cmp(&a.started_at)
            .then_with(|| a.scan_id.cmp(&b.scan_id))
    });
    let old = entries
        .iter()
        .find(|e| e.started_at < snapshot.meta().started_at)
        .and_then(|e| e.file.as_ref())
        .map(|file| Snapshot::open(file))
        .transpose()?;
    let snapshots = entries
        .into_iter()
        .filter_map(|e| e.file.map(|file| (e.scan_id, file, e.started_at)))
        .collect();
    let start = dir.id.saturating_add(1);
    let mut browser = Browser {
        snapshot,
        old,
        dir,
        state: State::new(start),
        rows: Vec::new(),
        text: String::new(),
        links: Vec::new(),
        snapshots,
        message: "Sort and filter apply to current 1,000-child page".into(),
        now: tidemark_core::time::unix_now(),
    };
    browser.load()?;
    let guard = terminal::Guard::enter()?;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
    loop {
        terminal.draw(|frame| draw::draw(frame, &browser))?;
        if guard.cancelled() {
            break;
        }
        if !event::poll(std::time::Duration::from_millis(100))? {
            continue;
        }
        let Event::Key(key) = event::read()? else {
            continue;
        };
        if key.kind == KeyEventKind::Release {
            continue;
        }
        if browser.state.editing {
            match key.code {
                KeyCode::Enter | KeyCode::Esc => browser.state.editing = false,
                KeyCode::Backspace => {
                    browser.state.filter.pop();
                }
                KeyCode::Char(c) if !c.is_control() && browser.state.filter.len() < 256 => {
                    browser.state.filter.push(c)
                }
                _ => {}
            }
            browser.state.selected = 0;
            continue;
        }
        let result: anyhow::Result<()> = (|| {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Up => browser.state.move_by(-1, browser.count()),
                KeyCode::Down => browser.state.move_by(1, browser.count()),
                KeyCode::Home => browser.state.selected = 0,
                KeyCode::End => browser.state.selected = browser.count().saturating_sub(1),
                KeyCode::PageDown if browser.state.pane == Pane::Main => {
                    if let Some(next) = browser.state.next {
                        browser.state.page_start = next;
                        browser.load()?;
                    }
                }
                KeyCode::PageUp if browser.state.pane == Pane::Main => {
                    // Rewalk only preorder boundaries; bounded memory, no stack of pages.
                    let target = browser.state.page_start;
                    let mut cursor = browser.dir.id.saturating_add(1);
                    let mut previous = cursor;
                    while cursor < target {
                        previous = cursor;
                        for _ in 0..PAGE {
                            if cursor >= target {
                                break;
                            }
                            cursor = browser.snapshot.dir(cursor)?.subtree_end.saturating_add(1);
                        }
                    }
                    browser.state.page_start = previous;
                    browser.load()?;
                }
                KeyCode::PageDown => browser.state.move_by(15, browser.count()),
                KeyCode::PageUp => browser.state.move_by(-15, browser.count()),
                KeyCode::Enter | KeyCode::Right => browser.enter()?,
                KeyCode::Backspace | KeyCode::Left => {
                    if browser.state.pane != Pane::Main {
                        browser.state.pane = Pane::Main;
                        browser.state.selected = 0;
                    } else if let Some(parent) = browser.dir.parent_id {
                        browser.navigate(browser.snapshot.dir(parent)?)?;
                    }
                }
                KeyCode::Char('s') => {
                    browser.state.sort = browser.state.sort.next();
                    browser.state.order(&mut browser.rows);
                }
                KeyCode::Char('i') => browser.pane(Pane::Info)?,
                KeyCode::Char('f') => browser.pane(Pane::Files)?,
                KeyCode::Char('l') => browser.pane(Pane::Links)?,
                KeyCode::Char('p') => browser.pane(Pane::Picker)?,
                KeyCode::Char('?') => browser.pane(Pane::Help)?,
                KeyCode::Char('/') => {
                    browser.state.pane = Pane::Main;
                    browser.state.editing = true;
                }
                KeyCode::Char('R') => {
                    browser.message =
                        "Live refresh blocked: top::live_rows is private; see W7-A handoff".into()
                }
                KeyCode::Esc => {
                    browser.state.pane = Pane::Main;
                    browser.state.filter.clear();
                    browser.state.selected = 0;
                }
                _ => {}
            }
            Ok(())
        })();
        if key.code == KeyCode::Char('q') {
            break;
        }
        if let Err(error) = result {
            browser.message = escape(error.to_string().as_bytes());
        }
    }
    drop(terminal);
    drop(guard);
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/w7a/browser.rs"]
mod tests;
