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
// Match the store query ceiling; never retain an unlimited live child cache.
const MAX_LIVE_CHILDREN: usize = 1_000_000;
#[derive(Default)]
struct Pages {
    cursor: Option<DirRow>,
    reverse: bool,
    growth: Option<Vec<(DirRow, Option<i128>)>>,
    offset: usize,
    total: usize,
    instances: Vec<(i64, tidemark_core::snapshot::LinkInstanceRow)>,
    inode: Option<u64>,
    live: Option<Vec<(DirRow, Option<i128>)>>,
    live_order: Option<state::Sort>,
    live_growth: std::collections::HashMap<u32, Option<i128>>,
}
struct LiveResult {
    root: DirRow,
    children: Vec<(DirRow, Option<i128>)>,
    sort: state::Sort,
    growth: std::collections::HashMap<u32, Option<i128>>,
}
struct Refresh {
    cancel: std::sync::Arc<std::sync::atomic::AtomicBool>,
    rx: crossbeam_channel::Receiver<anyhow::Result<LiveResult>>,
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
    snapshot_file: std::path::PathBuf,
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
        self.pages.total = if let Some(live) = &self.pages.live {
            live.iter()
                .filter(|(row, _)| byte_contains(&row.name, self.state.filter.as_bytes()))
                .count()
        } else {
            self.snapshot
                .children_count(self.dir.id, self.state.filter.as_bytes())? as usize
        };
        if self.state.sort == state::Sort::Growth
            && (self.snapshot.children_count(self.dir.id, b"")? > 10_000
                || self
                    .pages
                    .live
                    .as_ref()
                    .is_some_and(|rows| rows.len() > 10_000))
        {
            return Err(anyhow::anyhow!(
                "Growth sort requires at most 10,000 children"
            ));
        }
        if let Some(live) = &mut self.pages.live {
            if self.pages.live_order != Some(self.state.sort) {
                order_live(self.state.sort, live, &self.pages.live_growth);
                self.pages.live_order = Some(self.state.sort);
            }
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
                if self.pages.reverse && self.pages.cursor.is_none() {
                    // End uses the same aligned final slice as forward traversal.
                    self.pages.total.saturating_sub(self.pages.offset).min(PAGE)
                } else {
                    PAGE
                },
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
        self.pages.offset = if end {
            count.saturating_sub(1) / PAGE * PAGE
        } else {
            0
        };
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
        let previous_cursor = self.pages.cursor.clone();
        let previous_reverse = self.pages.reverse;
        let previous_offset = self.pages.offset;
        let previous_selection = self.state.selected;
        if self.pages.live.is_some() || self.state.sort == state::Sort::Growth {
            if reverse && self.pages.offset == 0 {
                return Ok(());
            }
            if reverse {
                self.pages.offset = self.pages.offset.saturating_sub(PAGE);
            } else if self.rows.len() == PAGE {
                self.pages.offset += PAGE;
            } else {
                return Ok(());
            }
        } else {
            self.pages.offset = if reverse {
                self.pages.offset.saturating_sub(PAGE)
            } else {
                self.pages.offset.saturating_add(self.rows.len())
            };
        }
        let cursor = if reverse {
            self.rows.first()
        } else {
            self.rows.last()
        }
        .map(|row| row.0.clone());
        if cursor.is_none() {
            return Ok(());
        }
        self.pages.cursor = cursor;
        self.pages.reverse = reverse;
        let previous = self.rows.clone();
        self.load()?;
        if self.rows.is_empty() {
            self.pages.offset = previous_offset;
            self.pages.cursor = previous_cursor;
            self.pages.reverse = previous_reverse;
            self.state.selected = previous_selection;
            self.rows = previous;
        }
        Ok(())
    }
    fn cycle_sort(&mut self) -> anyhow::Result<()> {
        let mut next = self.state.sort.next();
        if next == state::Sort::Growth && self.state.growth_refused {
            next = state::Sort::Name;
        }
        if next == state::Sort::Growth
            && (self.snapshot.children_count(self.dir.id, b"")? > 10_000
                || self
                    .pages
                    .live
                    .as_ref()
                    .is_some_and(|rows| rows.len() > 10_000))
        {
            self.state.growth_refused = true;
            self.message =
                "Growth sort requires at most 10,000 children; s advances to name".into();
            return Ok(());
        }
        self.state.sort = next;
        self.state.growth_refused = false;
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
        let snapshot_file = self.snapshot_file.clone();
        let sort = self.state.sort;
        let previous_file = self.old.as_ref().and_then(|old| {
            self.snapshots
                .iter()
                .find(|(id, _, _)| *id == old.meta().scan_id.to_string())
                .map(|(_, file, _)| file.clone())
        });
        let (tx, rx) = crossbeam_channel::bounded(1);
        let worker = std::thread::spawn(move || {
            let result = (|| -> anyhow::Result<LiveResult> {
                let rows = crate::live_rows::live_rows(&path, &config, &worker_cancel)?;
                let root = rows
                    .iter()
                    .find(|r| r.path == path)
                    .cloned()
                    .ok_or_else(|| anyhow::anyhow!("live root missing"))?;
                let snapshot = Snapshot::open(&snapshot_file)?;
                let mut children = Vec::new();
                for row in rows.into_iter().filter(|r| r.parent_id == Some(root.id)) {
                    if worker_cancel.load(std::sync::atomic::Ordering::Acquire) {
                        return Err(tidemark_fs::walk::WalkError::Cancelled.into());
                    }
                    if children.len() == MAX_LIVE_CHILDREN {
                        return Err(anyhow::anyhow!(
                            "Live refresh exceeds the 1,000,000-child display cap"
                        ));
                    }
                    let baseline = snapshot
                        .dir_by_path(&row.path)?
                        .map(|r| r.t_disk)
                        .unwrap_or(0);
                    let delta = i128::from(row.t_disk) - i128::from(baseline);
                    children.push((row, Some(delta)));
                }
                let mut growth = std::collections::HashMap::new();
                if children.len() <= 10_000 {
                    let previous = previous_file
                        .as_ref()
                        .map(|file| Snapshot::open(file))
                        .transpose()?;
                    for (row, _) in &children {
                        if worker_cancel.load(std::sync::atomic::Ordering::Acquire) {
                            return Err(tidemark_fs::walk::WalkError::Cancelled.into());
                        }
                        let delta = if let Some(previous) = &previous {
                            Some(
                                i128::from(row.t_disk)
                                    - i128::from(
                                        previous
                                            .dir_by_path(&row.path)?
                                            .map(|r| r.t_disk)
                                            .unwrap_or(0),
                                    ),
                            )
                        } else {
                            None
                        };
                        growth.insert(row.id, delta);
                    }
                }
                order_live(sort, &mut children, &growth);
                Ok(LiveResult {
                    root,
                    children,
                    sort,
                    growth,
                })
            })();
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
        let result = match refresh.rx.try_recv() {
            Ok(result) => result,
            Err(crossbeam_channel::TryRecvError::Empty) => return Ok(()),
            Err(crossbeam_channel::TryRecvError::Disconnected) => {
                self.refresh = None;
                self.message = "Live refresh worker stopped without a result".into();
                return Ok(());
            }
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
        self.dir = DirRow {
            id: self.dir.id,
            parent_id: self.dir.parent_id,
            ..rows.root
        };
        let growth_unavailable =
            rows.children.len() > 10_000 && self.state.sort == state::Sort::Growth;
        if growth_unavailable {
            self.state.sort = state::Sort::Name;
        }
        self.pages.live = Some(rows.children);
        self.pages.live_order = Some(rows.sort);
        self.pages.live_growth = rows.growth;
        self.pages.offset = 0;
        self.pages.cursor = None;
        self.pages.reverse = false;
        self.load()?;
        self.message = if growth_unavailable {
            "live values; delta against snapshot; growth exceeds 10,000 children, showing name"
                .into()
        } else {
            "live values; delta against snapshot".into()
        };
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
    fn own_position(&self) -> usize {
        // Own growth is not retained; put unknown growth after known children.
        if self.state.sort == state::Sort::Growth {
            return if byte_contains(b".", self.state.filter.as_bytes())
                && self.pages.offset.saturating_add(self.rows.len()) >= self.pages.total
            {
                self.rows.len()
            } else {
                usize::MAX
            };
        }
        let mut own = self.dir.clone();
        own.name = b".".to_vec();
        own.t_disk = own.own_charged;
        own.t_files = own.own_files;
        own.t_reclaimable = 0;
        own.newest_mtime = i64::MIN;
        let own = (own, None);
        let before = |row: &(DirRow, Option<i128>)| self.state.compare(row, &own).is_lt();
        let position = self.rows.iter().take_while(|row| before(row)).count();
        let matches = byte_contains(b".", self.state.filter.as_bytes());
        let at_beginning = self.pages.offset == 0;
        let at_end = self.pages.offset.saturating_add(self.rows.len()) >= self.pages.total;
        let in_page = if let Some(live) = &self.pages.live {
            // Live rows use offset slices, so keyset cursors cannot delimit them.
            let rank = live
                .iter()
                .filter(|(row, _)| byte_contains(&row.name, self.state.filter.as_bytes()))
                .take_while(|row| before(row))
                .count();
            rank >= self.pages.offset
                && (rank < self.pages.offset.saturating_add(self.rows.len()) || at_end)
        } else if let Some(cursor) = &self.pages.cursor {
            if self.pages.reverse {
                !before(&(cursor.clone(), None)) && (position > 0 || at_beginning)
            } else {
                before(&(cursor.clone(), None)) && (position < self.rows.len() || at_end)
            }
        } else {
            (position > 0 || at_beginning) && (position < self.rows.len() || at_end)
        };
        if matches && in_page {
            position
        } else {
            usize::MAX
        }
    }
    fn child_at(&self, selected: usize) -> Option<&(DirRow, Option<i128>)> {
        let own = self.own_position();
        if selected == own {
            None
        } else {
            self.rows
                .get(selected - usize::from(selected > own && own != usize::MAX))
        }
    }
    fn count(&self) -> usize {
        match self.state.pane {
            Pane::Main => self.visible().len() + usize::from(self.own_position() != usize::MAX),
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
        self.message.clear();
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
        let own = self.state.pane == Pane::Main && self.state.selected == self.own_position();
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
                    self.text.push_str(&format!("Link totals: {} {sets}, {} {instances}, {} {bytes}; incomplete {}\n",
                        report.totals.sets,report.totals.instances,report.totals.disk,report.totals.incomplete_sets, sets=crate::render::unit(report.totals.sets,"set","sets"), instances=crate::render::unit(report.totals.instances,"instance","instances"), bytes=crate::render::unit(report.totals.disk,"byte","bytes")));
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
                self.text=hits.iter().map(|h|format!("{} {} H {} {instances} {}\n",h.row.ino,
                    crate::render::size::bytes(h.row.disk),h.instance_count,h.lca_path.display_lossy(),instances=crate::render::unit(h.instance_count,"instance","instances"))).collect();
            }
            Pane::Help => self.text="Arrows move / descend / ascend; Enter descends\nPgDn next page; PgUp previous page; Home/End whole directory\ns sort: size/reclaim/files/newest/growth/name\ni info; l links (Enter instances); f files; p snapshots\n/ filter children; Escape clear/close pane; ? help; q quit\nRead-only browser: no delete, move, open or shell actions\nPages contain at most 1,000 direct children.\nR live refresh; Esc cancels; growth limited to 10,000 children.".into(),
            _ => {}
        }
        Ok(())
    }
    fn close_pane(&mut self) {
        self.state.pane = Pane::Main;
        self.state.selected = 0;
    }
    fn enter(&mut self) -> anyhow::Result<()> {
        match self.state.pane {
            Pane::Main => {
                if self.state.selected == self.own_position() {
                    let snapshot_dir = self.snapshot.dir(self.dir.id)?;
                    let hits = crate::cmd::files::retained_files(
                        &self.snapshot,
                        self.dir.id,
                        &Default::default(),
                        1_000_000,
                        true,
                    )?;
                    let count = hits.len() as u64;
                    self.text = format!(
                        "Snapshot {} at {}\ndoes not include changes since this snapshot\nPATH  SIZE  APPARENT  MTIME UTC\n",
                        self.snapshot.meta().scan_id,
                        crate::render::size::timestamp(self.snapshot.meta().finished_at)
                    );
                    self.text.push_str(
                        &hits
                            .iter()
                            .map(|hit| {
                                format!(
                                    "{}  {}  {}  {}{}\n",
                                    hit.path
                                        .as_ref()
                                        .map(|p| p.display_lossy())
                                        .unwrap_or_else(|| "[unresolved]".into()),
                                    crate::render::size::bytes(hit.row.disk),
                                    crate::render::size::bytes(hit.row.apparent),
                                    crate::render::size::timestamp(hit.row.mtime),
                                    if hit.row.nlink > 1 {
                                        " [hard links]"
                                    } else {
                                        ""
                                    }
                                )
                            })
                            .collect::<String>(),
                    );
                    // Representatives, entry counts and LCA charges are distinct quantities.
                    self.text.push_str(&format!(
                        "{count} retained representatives listed; {} file entries here\ncharged to this directory: {}\n(includes directory blocks and hard-linked files whose links share this directory as common ancestor)",
                        snapshot_dir.own_files,
                        crate::render::size::bytes(snapshot_dir.own_charged)
                    ));
                    self.state.pane = Pane::Files;
                    self.state.selected = 0;
                    return Ok(());
                }
                if let Some((row, _)) = self.child_at(self.state.selected) {
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
                        self.snapshot_file = file.clone();
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
fn order_live(
    sort: state::Sort,
    rows: &mut [(DirRow, Option<i128>)],
    growth: &std::collections::HashMap<u32, Option<i128>>,
) {
    if sort == state::Sort::Growth {
        rows.sort_by(|a, b| {
            growth
                .get(&b.0.id)
                .cmp(&growth.get(&a.0.id))
                .then_with(|| a.0.name.cmp(&b.0.name))
        });
    } else {
        let mut state = State::new(0);
        state.sort = sort;
        state.order(rows);
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
        .ok_or_else(|| crate::cmd::snapshots::missing_path(context, &path, &snapshot))?;
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
    let snapshot_file = entries
        .iter()
        .find(|e| e.scan_id == snapshot.meta().scan_id.to_string())
        .and_then(|e| e.file.clone())
        .ok_or_else(|| anyhow::anyhow!("snapshot missing from catalog"))?;
    let snapshots = entries
        .into_iter()
        .filter_map(|e| e.file.map(|file| (e.scan_id, file, e.started_at)))
        .collect();
    let start = dir.id.saturating_add(1);
    let mut browser = Browser {
        snapshot,
        snapshot_file,
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
        if key.code == KeyCode::Esc {
            if let Some(refresh) = &browser.refresh {
                refresh
                    .cancel
                    .store(true, std::sync::atomic::Ordering::Release);
            }
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
                KeyCode::Up if browser.state.pane == Pane::Main && browser.state.selected == 0 => {
                    let first = browser.rows.first().map(|r| r.0.id);
                    browser.page(true)?;
                    if browser.rows.first().map(|r| r.0.id) != first {
                        browser.state.selected = browser.count().saturating_sub(1);
                    }
                }
                KeyCode::Down
                    if browser.state.pane == Pane::Main
                        && browser.state.selected + 1 == browser.count() =>
                {
                    browser.page(false)?
                }
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
                        browser.close_pane();
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
                    browser.state.filter.clear();
                    browser.boundary(false)?;
                    browser.close_pane();
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
