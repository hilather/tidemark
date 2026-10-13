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
#[derive(Default)]
struct Pages {
    cursor: Option<DirRow>,
    reverse: bool,
    growth: Option<Vec<(DirRow, Option<i128>)>>,
    offset: usize,
    instances: Vec<(i64, tidemark_core::snapshot::LinkInstanceRow)>,
    inode: Option<u64>,
    live: Option<Vec<(DirRow, Option<i128>)>>,
}
struct Refresh {
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    rx: crossbeam_channel::Receiver<anyhow::Result<Vec<DirRow>>>,
    worker: Option<std::thread::JoinHandle<()>>,
}
impl Drop for Refresh {
    fn drop(&mut self) {
        self.cancel
            .store(true, std::sync::atomic::Ordering::Release);
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}
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
    pages: Pages,
    refresh: Option<Refresh>,
}
impl Browser {
    fn load(&mut self) -> anyhow::Result<()> {
        if self.state.sort == state::Sort::Growth
            && self.snapshot.children_count(self.dir.id, b"")? > 10_000
        {
            return Err(anyhow::anyhow!(
                "Growth sort requires at most 10,000 children"
            ));
        }
        if let Some(live) = &mut self.pages.live {
            self.state.order(live);
            self.rows = live
                .iter()
                .filter(|(r, _)| byte_contains(&r.name, self.state.filter.as_bytes()))
                .skip(self.pages.offset)
                .take(PAGE)
                .cloned()
                .collect();
        } else if self.state.sort == state::Sort::Growth {
            if self.pages.growth.is_none() {
                let mut rows = self
                    .snapshot
                    .children_page(
                        self.dir.id,
                        tidemark_core::snapshot::SortKey::Name,
                        false,
                        None,
                        false,
                        10_000,
                        b"",
                    )?
                    .into_iter()
                    .map(|r| (r, None))
                    .collect::<Vec<_>>();
                if let Some(old) = &self.old {
                    for (row, delta) in &mut rows {
                        *delta = Some(
                            i128::from(row.t_disk)
                                - i128::from(
                                    old.dir_by_path(&row.path)?.map(|r| r.t_disk).unwrap_or(0),
                                ),
                        );
                    }
                }
                self.state.order(&mut rows);
                self.pages.growth = Some(rows);
            }
            self.rows = self
                .pages
                .growth
                .as_ref()
                .into_iter()
                .flatten()
                .filter(|(r, _)| byte_contains(&r.name, self.state.filter.as_bytes()))
                .skip(self.pages.offset)
                .take(PAGE)
                .cloned()
                .collect();
        } else {
            let sort = match self.state.sort {
                state::Sort::Size => tidemark_core::snapshot::SortKey::Disk,
                state::Sort::Reclaim => tidemark_core::snapshot::SortKey::Reclaimable,
                state::Sort::Files => tidemark_core::snapshot::SortKey::Files,
                state::Sort::Newest => tidemark_core::snapshot::SortKey::Newest,
                _ => tidemark_core::snapshot::SortKey::Name,
            };
            let mut rows = self.snapshot.children_page(
                self.dir.id,
                sort,
                self.state.sort != state::Sort::Name,
                self.pages.cursor.as_ref(),
                self.pages.reverse,
                PAGE,
                self.state.filter.as_bytes(),
            )?;
            if self.pages.reverse {
                rows.reverse();
            }
            self.rows = rows.into_iter().map(|r| (r, None)).collect();
        }
        self.state.selected = 0;
        Ok(())
    }
    fn boundary(&mut self, end: bool) -> anyhow::Result<()> {
        self.pages.cursor = None;
        self.pages.reverse = end;
        let count = if let Some(live) = &self.pages.live {
            live.iter()
                .filter(|(r, _)| byte_contains(&r.name, self.state.filter.as_bytes()))
                .count()
        } else {
            self.snapshot
                .children_count(self.dir.id, self.state.filter.as_bytes())? as usize
        };
        self.pages.offset = if end { count.saturating_sub(PAGE) } else { 0 };
        self.load()?;
        if end {
            self.state.selected = self.count().saturating_sub(1);
        }
        Ok(())
    }
    fn page(&mut self, reverse: bool) -> anyhow::Result<()> {
        if self.state.pane == Pane::Instances {
            return self.instance_page(reverse, false);
        }
        if self.pages.live.is_some() || self.state.sort == state::Sort::Growth {
            if reverse {
                self.pages.offset = self.pages.offset.saturating_sub(PAGE);
            } else if self.rows.len() == PAGE {
                self.pages.offset += PAGE;
            } else {
                return Ok(());
            }
        } else {
            let cursor = if reverse {
                self.rows.first()
            } else {
                self.rows.last()
            }
            .map(|r| r.0.clone());
            if cursor.is_none() {
                return Ok(());
            }
            self.pages.cursor = cursor;
            self.pages.reverse = reverse;
        }
        let previous = self.rows.clone();
        self.load()?;
        if self.rows.is_empty() {
            self.rows = previous;
        }
        Ok(())
    }
    fn cycle_sort(&mut self) -> anyhow::Result<()> {
        let next = self.state.sort.next();
        if next == state::Sort::Growth && self.snapshot.children_count(self.dir.id, b"")? > 10_000 {
            self.message = "Growth sort requires at most 10,000 children".into();
            return Ok(());
        }
        self.state.sort = next;
        self.boundary(false)
    }
    fn instance_page(&mut self, reverse: bool, boundary: bool) -> anyhow::Result<()> {
        let Some(ino) = self.pages.inode else {
            return Ok(());
        };
        let cursor = if boundary {
            None
        } else if reverse {
            self.pages.instances.first()
        } else {
            self.pages.instances.last()
        };
        let mut rows = self
            .snapshot
            .link_instances_page(ino, cursor, reverse, PAGE)?;
        if rows.is_empty() && !boundary {
            return Ok(());
        }
        if reverse {
            rows.reverse();
        }
        let mut lines = Vec::with_capacity(rows.len());
        for (_, row) in &rows {
            let dir = self.snapshot.dir(row.dir_id)?;
            lines.push(
                row.name
                    .as_ref()
                    .map(|name| dir.path.join(name).display_lossy())
                    .unwrap_or_else(|| format!("{}/[unresolved name]", dir.path.display_lossy())),
            );
        }
        self.pages.instances = rows;
        self.text = lines.join("\n");
        self.state.selected = 0;
        Ok(())
    }
    fn start_refresh(&mut self, config: &tidemark_core::config::Config) {
        if self.refresh.is_some() {
            self.message = "Live refresh already running".into();
            return;
        }
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let worker_cancel = cancel.clone();
        let path = self.dir.path.clone();
        let config = config.clone();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let worker = std::thread::spawn(move || {
            let result = crate::live_rows::live_rows(&path, &config, &worker_cancel);
            let _ = tx.send(result);
        });
        self.refresh = Some(Refresh {
            cancel,
            rx,
            worker: Some(worker),
        });
        self.message = "Live refresh running; Esc cancels".into();
    }
    fn poll_refresh(&mut self) -> anyhow::Result<()> {
        let Some(refresh) = &self.refresh else {
            return Ok(());
        };
        let Ok(result) = refresh.rx.try_recv() else {
            return Ok(());
        };
        let cancelled = refresh.cancel.load(std::sync::atomic::Ordering::Acquire);
        self.refresh = None;
        if cancelled {
            self.message = "Live refresh cancelled".into();
            return Ok(());
        }
        let rows = match result {
            Ok(rows) => rows,
            Err(error) => {
                self.message = escape(error.to_string().as_bytes());
                return Ok(());
            }
        };
        let root = rows.iter().find(|r| r.path == self.dir.path).map(|r| r.id);
        let mut children = Vec::new();
        for row in rows {
            if Some(row.id) == root {
                self.dir = DirRow {
                    id: self.dir.id,
                    parent_id: self.dir.parent_id,
                    ..row
                };
            } else if row.parent_id == root {
                let baseline = self
                    .snapshot
                    .dir_by_path(&row.path)?
                    .map(|r| r.t_disk)
                    .unwrap_or(0);
                children.push((row, Some(i128::from(baseline))));
            }
        }
        for (row, delta) in &mut children {
            *delta = delta.map(|baseline| i128::from(row.t_disk) - baseline);
        }
        self.pages.live = Some(children);
        self.pages.offset = 0;
        self.load()?;
        self.message = "live values; delta against snapshot".into();
        Ok(())
    }
    fn deltas(&mut self, height: usize, all: bool) -> anyhow::Result<()> {
        let Some(old) = &self.old else {
            return Ok(());
        };
        let start = if all {
            0
        } else {
            self.state.selected.saturating_sub(height)
        };
        let limit = if all { PAGE } else { height.saturating_mul(2) };
        for (row, delta) in self.rows.iter_mut().skip(start).take(limit) {
            if delta.is_none() {
                *delta = Some(
                    i128::from(row.t_disk)
                        - i128::from(old.dir_by_path(&row.path)?.map(|r| r.t_disk).unwrap_or(0)),
                );
            }
        }
        Ok(())
    }
    fn visible(&self) -> Vec<&(DirRow, Option<i128>)> {
        self.rows.iter().collect()
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
        if let Some(refresh) = &self.refresh {
            refresh
                .cancel
                .store(true, std::sync::atomic::Ordering::Release);
        }
        self.pages = Pages::default();
        self.state.filter.clear();
        if self.state.sort == state::Sort::Growth
            && self.snapshot.children_count(dir.id, b"")? > 10_000
        {
            self.state.sort = state::Sort::Name;
        }
        self.state.reset(dir.id.saturating_add(1));
        self.dir = dir;
        self.load()
    }
    fn pane(&mut self, pane: Pane) -> anyhow::Result<()> {
        let own = self.state.pane == Pane::Main && self.state.selected == self.rows.len();
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
                if own { self.text.push_str("[own files]: the directory’s own files as charged\n"); }
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
            Pane::Help => self.text="Arrows move / descend / ascend; Enter descends\nPgDn next page; PgUp previous page; Home/End whole directory\ns sort: size/reclaim/files/newest/growth/name\ni info; l links (Enter instances); f files; p snapshots\n/ filter children; Escape clear/close pane; ? help; q quit\nRead-only browser: no delete, move, open or shell actions\nPages contain at most 1,000 direct children.\nR live refresh; Esc cancels; growth limited to 10,000 children.".into(),
            _ => {}
        }
        Ok(())
    }
    fn enter(&mut self) -> anyhow::Result<()> {
        match self.state.pane {
            Pane::Main => {
                if let Some((row, _)) = self.visible().get(self.state.selected).copied() {
                    let row = self.snapshot.dir_by_path(&row.path)?;
                    if let Some(row) = row {
                        self.navigate(row)?;
                    } else {
                        self.message = "Live directory absent from snapshot".into();
                    }
                }
            }
            Pane::Links => {
                if let Some(ino) = self.links.get(self.state.selected) {
                    self.pages.inode = Some(*ino);
                    self.state.pane = Pane::Instances;
                    self.instance_page(false, true)?;
                }
            }
            Pane::Instances => {
                if let Some((_, row)) = self.pages.instances.get(self.state.selected) {
                    self.navigate(self.snapshot.dir(row.dir_id)?)?;
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
fn byte_contains(name: &[u8], filter: &[u8]) -> bool {
    filter.is_empty() || name.windows(filter.len()).any(|w| w == filter)
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
        message: "Sort and filter apply to whole directory".into(),
        now: tidemark_core::time::unix_now(),
        pages: Pages::default(),
        refresh: None,
    };
    browser.load()?;
    let guard = terminal::Guard::enter()?;
    let mut terminal =
        ratatui::Terminal::new(ratatui::backend::CrosstermBackend::new(std::io::stdout()))?;
    loop {
        browser.poll_refresh()?;
        if browser.state.pane == Pane::Main {
            browser.deltas(
                usize::from(terminal.size()?.height),
                browser.state.sort == state::Sort::Growth,
            )?;
        }
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
            browser.boundary(false)?;
            continue;
        }
        let result: anyhow::Result<()> = (|| {
            match key.code {
                KeyCode::Char('q') => return Ok(()),
                KeyCode::Up => browser.state.move_by(-1, browser.count()),
                KeyCode::Down => browser.state.move_by(1, browser.count()),
                KeyCode::Home | KeyCode::End if browser.state.pane == Pane::Main => {
                    browser.boundary(key.code == KeyCode::End)?
                }
                KeyCode::Home | KeyCode::End if browser.state.pane == Pane::Instances => {
                    browser.instance_page(key.code == KeyCode::End, true)?;
                    if key.code == KeyCode::End {
                        browser.state.selected = browser.count().saturating_sub(1);
                    }
                }
                KeyCode::Home => browser.state.selected = 0,
                KeyCode::End => browser.state.selected = browser.count().saturating_sub(1),
                KeyCode::PageDown | KeyCode::PageUp
                    if matches!(browser.state.pane, Pane::Main | Pane::Instances) =>
                {
                    browser.page(key.code == KeyCode::PageUp)?
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
                KeyCode::Char('s') => browser.cycle_sort()?,
                KeyCode::Char('i') => browser.pane(Pane::Info)?,
                KeyCode::Char('f') => browser.pane(Pane::Files)?,
                KeyCode::Char('l') => browser.pane(Pane::Links)?,
                KeyCode::Char('p') => browser.pane(Pane::Picker)?,
                KeyCode::Char('?') => browser.pane(Pane::Help)?,
                KeyCode::Char('/') => {
                    browser.state.pane = Pane::Main;
                    browser.state.editing = true;
                }
                KeyCode::Char('R') => browser.start_refresh(&context.config),
                KeyCode::Esc => {
                    if let Some(refresh) = &browser.refresh {
                        refresh
                            .cancel
                            .store(true, std::sync::atomic::Ordering::Release);
                    }
                    browser.state.pane = Pane::Main;
                    browser.state.filter.clear();
                    browser.boundary(false)?;
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
