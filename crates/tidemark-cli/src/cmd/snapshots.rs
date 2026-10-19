//! Owned by: W2-C
//! Command surface and shared CLI support.

use anyhow::Context as _;

/// Command arguments.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Operation.
    #[command(subcommand)]
    pub command: Command,
}
/// Available operations.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// List the snapshot catalog.
    List {
        /// Root path to scan or select.
        #[arg(long, value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
    },
    /// Show a snapshot from the catalog.
    Show {
        /// Snapshot ID to show.
        #[arg(value_name = "ID")]
        id: String,
        /// Root path to scan or select.
        #[arg(long, value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
    },
    /// Export a snapshot to a new SQLite database file (v1 schema, with `dirs.path`).
    Export {
        /// Write a v1-schema SQLite database (mode 0600; the output must not exist).
        #[arg(long, required = true)]
        sqlite: bool,
        /// Snapshot ID to export.
        #[arg(value_name = "ID")]
        id: String,
        /// New database file to create.
        #[arg(value_name = "OUT")]
        out: std::path::PathBuf,
        /// Root path to scan or select.
        #[arg(long, value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
    },
    /// Prune the snapshot catalog.
    Prune {
        /// Root path to scan or select.
        #[arg(long, value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
        /// Show proposed deletions and compressions without changing snapshots.
        #[arg(long)]
        dry_run: bool,
    },
    /// Rebuild the snapshot catalog for all roots.
    RebuildCatalog {
        /// Scoped rebuilding is unavailable; omit this option for a global rebuild.
        #[arg(long, value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
    },
}
/// Execute the operation.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    use tidemark_store::catalog::Catalog;
    let state = &context.config.general.state_dir;
    let (root_arg, mutation) = match &args.command {
        Command::List { root } | Command::Show { root, .. } | Command::Export { root, .. } => {
            (root, false)
        }
        Command::Prune { root, dry_run } => (root, !dry_run),
        Command::RebuildCatalog { root } => (root, true),
    };
    let root = root_arg
        .as_deref()
        .map(|path| {
            tidemark_fs::canon::canonicalize(path)
                .with_context(|| format!("resolve snapshot root: {}", super::display_path(path)))
        })
        .transpose()?;
    if matches!(args.command, Command::RebuildCatalog { .. }) && root_arg.is_some() {
        return Err(crate::CliError::Configuration(
            "rebuild-catalog is global; --root is unsupported; omit --root to rebuild all roots"
                .into(),
        )
        .into());
    }
    if matches!(args.command, Command::RebuildCatalog { .. }) {
        require_absent_daemon(context)?;
        drop(super::scan::offline_catalog(context)?);
    }
    let _writer_lease = if mutation {
        require_absent_daemon(context)?;
        Some(Catalog::open(state, true)?)
    } else {
        None
    };
    let lock = if mutation {
        require_absent_daemon(context)?;
        Some(
            tidemark_scan::orchestrator::ScanLock::acquire(state)
                .map_err(super::scan::scan_error)?,
        )
    } else {
        None
    };
    if mutation {
        require_absent_daemon(context)?;
    }
    let catalog = if matches!(args.command, Command::RebuildCatalog { .. }) {
        lock.as_ref()
            .ok_or_else(|| {
                crate::CliError::Precondition("catalog mutation requires scan ownership".into())
            })?
            .open_catalog(state)
            .with_context(|| format!("recover catalog: {}", super::display_path(state)))?
    } else {
        Catalog::open(state, mutation)
            .with_context(|| format!("open catalog: {}", super::display_path(state)))?
    };
    let mut out = std::io::stdout().lock();
    match &args.command {
        Command::List { .. } => {
            let rows = catalog.list(root.as_ref())?;
            if context.json {
                crate::render::json(
                    "tidemark.snapshots.v1",
                    rows.iter()
                        .map(|e| entry_json(refreshed(&catalog, e).as_ref().unwrap_or(e)))
                        .collect::<anyhow::Result<Vec<_>>>()?,
                    &mut out,
                )?;
            } else {
                use std::io::Write;
                let mut table = vec![vec![
                    "ID".into(),
                    "ROOT".into(),
                    "MODE".into(),
                    "STATUS".into(),
                    "DISK".into(),
                    "STORAGE".into(),
                    "FILE BYTES".into(),
                    "UNCOMPRESSED BYTES".into(),
                ]];
                for original in rows {
                    let fresh = refreshed(&catalog, &original);
                    let e = fresh.unwrap_or(original);
                    let (cold, file_bytes, uncompressed_bytes) = entry_sizes(&e);
                    table.push(vec![
                        e.scan_id,
                        e.root.display_lossy(),
                        crate::render::enum_label(e.mode),
                        format!(
                            "{}{}",
                            e.status.to_ascii_lowercase(),
                            if e.degraded { " (degraded)" } else { "" }
                        ),
                        e.t_disk
                            .map(crate::render::size::bytes)
                            .unwrap_or_else(|| "unknown".into()),
                        if cold { "cold".into() } else { "hot".into() },
                        file_bytes
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "-".into()),
                        uncompressed_bytes
                            .map(|n| n.to_string())
                            .unwrap_or_else(|| "-".into()),
                    ]);
                }
                write!(out, "{}", crate::render::table::aligned(&table))?;
            }
        }
        Command::Show { id, .. } => {
            let e = catalog
                .get(id)?
                .ok_or_else(|| crate::CliError::Precondition(format!("snapshot {id} not found")))?;
            if root.as_ref().is_some_and(|r| r != &e.root) {
                return Err(crate::CliError::Precondition(
                    "snapshot belongs to a different root".into(),
                )
                .into());
            }
            let data = if let Some(file) = &e.file {
                serde_json::to_value(
                    open_snapshot(context, file)
                        .with_context(|| format!("open snapshot: {}", super::display_path(file)))?
                        .meta(),
                )?
            } else {
                entry_json(&e)?
            };
            if context.json {
                crate::render::json("tidemark.snapshots.v1", data, &mut out)?;
            } else {
                use std::io::Write;
                writeln!(out, "{}", serde_json::to_string_pretty(&data)?)?;
            }
        }
        Command::Export {
            id, out: target, ..
        } => {
            // `-` is a filename today; refuse it as usage before any export work.
            if target.as_os_str() == "-" {
                return Err(crate::CliError::Configuration(
                    "output path '-' is not supported; pass a file path".into(),
                )
                .into());
            }
            let e = catalog
                .get(id)?
                .ok_or_else(|| crate::CliError::Precondition(format!("snapshot {id} not found")))?;
            if root.as_ref().is_some_and(|r| r != &e.root) {
                return Err(crate::CliError::Precondition(
                    "snapshot belongs to a different root".into(),
                )
                .into());
            }
            let file = e
                .file
                .as_ref()
                .ok_or_else(|| crate::CliError::Precondition("snapshot has no file".into()))?;
            // Resolve once: the published path is the one printed, with no second lookup.
            let target = &std::path::absolute(target)?;
            refuse_inside_state(target, state)?;
            // Fast refusal before open, inflate and copy. A path that appears
            // later is `StoreError::OutputExists` and still exits 5.
            if std::fs::symlink_metadata(target).is_ok() {
                return Err(crate::CliError::Precondition(format!(
                    "output path already exists: {}",
                    super::display_path(target)
                ))
                .into());
            }
            // SIGINT/SIGTERM/SIGHUP only set a flag; the export stops between batches and
            // unwinds, so its staging copy and the cold-inflate leaf are removed.
            let interrupt = ExportInterrupt::new()?;
            // Cold snapshots are inflated by the reader to a private leaf under
            // `.inflate` and removed when it drops.
            let snapshot = open_snapshot(context, file)
                .map_err(export_error)
                .with_context(|| format!("open snapshot: {}", super::display_path(file)))?;
            let stats = tidemark_store::export_sqlite::export_sqlite_cancellable(
                &snapshot,
                target,
                &|| interrupt.flag.load(std::sync::atomic::Ordering::Acquire),
            )
            .map_err(export_error)
            .with_context(|| format!("export snapshot: {}", super::display_path(target)))?;
            if context.json {
                crate::render::json(
                    "tidemark.snapshots.v1",
                    serde_json::json!({"exported":id,"out":tidemark_core::path::RawPath::from_path(target)?.to_json(),
                        "dirs":stats.dirs,"files":stats.files,"link_sets":stats.link_sets,
                        "link_instances":stats.link_instances,"skipped":stats.skipped,
                        "errors":stats.errors,"annotations":stats.annotations}),
                    &mut out,
                )?;
            } else {
                use std::io::Write;
                writeln!(
                    out,
                    "Exported {id} to {}: {} {}, {} {}, {} link {}",
                    super::display_path(target),
                    stats.dirs,
                    crate::render::unit(stats.dirs, "directory", "directories"),
                    stats.files,
                    crate::render::unit(stats.files, "file", "files"),
                    stats.link_sets,
                    crate::render::unit(stats.link_sets, "set", "sets"),
                )?;
            }
        }
        Command::Prune { dry_run, .. } => {
            let entries = catalog.list(root.as_ref())?;
            let mut ids = Vec::new();
            let now = tidemark_core::time::unix_now();
            // The retention planner already groups by root and protects its latest complete scan.
            for id in tidemark_store::retention::retention_plan(
                &entries,
                &context.config.retention.snapshots,
                now,
            ) {
                if !dry_run {
                    if let Some(file) = entries
                        .iter()
                        .find(|e| e.scan_id == id)
                        .and_then(|e| e.file.as_ref())
                    {
                        remove_snapshot(state, file)?;
                    }
                }
                ids.push(id);
            }
            if !dry_run {
                catalog.rebuild_from_files()?;
            }
            let policy = &context.config.retention.snapshots;
            let mut compressed = Vec::new();
            if policy.cold_after_recent {
                for id in tidemark_store::retention::cold_plan(&entries, policy, policy.recent, now)
                {
                    let Some(hot) = entries
                        .iter()
                        .find(|e| e.scan_id == id)
                        .and_then(|e| e.file.as_ref())
                    else {
                        continue;
                    };
                    if !dry_run {
                        let result = (|| -> anyhow::Result<()> {
                            let estimate = std::fs::metadata(hot)?.len() / 2;
                            if let tidemark_store::space_guard::SpaceVerdict::Refuse { reason } =
                                tidemark_store::space_guard::space_check(
                                    state,
                                    estimate,
                                    &context.config.retention,
                                )?
                            {
                                anyhow::bail!("{reason}");
                            }
                            let cold = tidemark_store::cold::compress_snapshot(
                                hot,
                                policy.cold_level,
                                &|| false,
                            )?;
                            catalog.point_to_cold(&id, &cold)?;
                            let directory = snapshot_directory(state, hot)?;
                            let name = hot
                                .file_name()
                                .ok_or_else(|| anyhow::anyhow!("missing snapshot filename"))?;
                            directory.unlink_leaf(name)?;
                            rustix::fs::fsync(directory.fd())?;
                            Ok(())
                        })();
                        if let Err(reason) = result {
                            tracing::warn!(scan_id=%id, %reason, "cold compression skipped");
                            continue;
                        }
                    }
                    compressed.push(id);
                }
            }
            if context.json {
                crate::render::json(
                    "tidemark.snapshots.v1",
                    serde_json::json!({"dry_run":dry_run,"pruned":ids,"compressed":compressed}),
                    &mut out,
                )?;
            } else {
                use std::io::Write;
                for id in ids {
                    writeln!(
                        out,
                        "{} {id}",
                        if *dry_run { "would prune" } else { "pruned" }
                    )?;
                }
                for id in compressed {
                    writeln!(
                        out,
                        "{} {id}",
                        if *dry_run {
                            "would compress"
                        } else {
                            "compressed"
                        }
                    )?;
                }
            }
        }
        Command::RebuildCatalog { .. } => {
            let count = catalog.rebuild_from_files()?;
            if context.json {
                crate::render::json(
                    "tidemark.snapshots.v1",
                    serde_json::json!({"rebuilt":count}),
                    &mut out,
                )?;
            } else {
                use std::io::Write;
                writeln!(
                    out,
                    "Rebuilt catalog: {count} {}",
                    crate::render::unit(count as u64, "snapshot", "snapshots")
                )?;
            }
        }
    }
    Ok(())
}
/// Exports never land inside the state directory, which only Tidemark writes.
fn refuse_inside_state(target: &std::path::Path, state: &std::path::Path) -> anyhow::Result<()> {
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| std::path::Path::new("."));
    let (Ok(parent), Ok(state)) = (std::fs::canonicalize(parent), std::fs::canonicalize(state))
    else {
        return Ok(());
    };
    if parent.starts_with(&state) {
        return Err(
            crate::CliError::Precondition("refusing export inside state_dir".into()).into(),
        );
    }
    Ok(())
}
/// Space exhaustion and an existing output path exit 5; other errors keep their classification.
fn export_error(error: tidemark_store::StoreError) -> anyhow::Error {
    use tidemark_store::StoreError;
    match error {
        StoreError::Space(reason) => {
            crate::CliError::Precondition(format!("no space for export: {reason}")).into()
        }
        StoreError::Io(io)
            if matches!(
                io.raw_os_error(),
                Some(n) if n == rustix::io::Errno::NOSPC.raw_os_error()
                    || n == rustix::io::Errno::DQUOT.raw_os_error()
            ) =>
        {
            crate::CliError::Precondition(format!("no space for export: {io}")).into()
        }
        StoreError::Io(io) if io.kind() == std::io::ErrorKind::Interrupted => {
            crate::CliError::Partial("export interrupted".into()).into()
        }
        // Output collisions only. Other `AlreadyExists` I/O stays a runtime error.
        StoreError::OutputExists(path) => {
            crate::CliError::Precondition(format!("output path already exists: {path}")).into()
        }
        other => other.into(),
    }
}
/// Termination signals during an export set a flag instead of killing the process.
struct ExportInterrupt {
    flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ids: Vec<signal_hook::SigId>,
}
impl ExportInterrupt {
    fn new() -> anyhow::Result<Self> {
        let mut guard = Self {
            flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ids: Vec::with_capacity(3),
        };
        for signal in [
            signal_hook::consts::SIGINT,
            signal_hook::consts::SIGTERM,
            signal_hook::consts::SIGHUP,
        ] {
            guard
                .ids
                .push(signal_hook::flag::register(signal, guard.flag.clone())?);
        }
        Ok(guard)
    }
}
impl Drop for ExportInterrupt {
    fn drop(&mut self) {
        for id in self.ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}
fn require_absent_daemon(context: &crate::Context) -> anyhow::Result<()> {
    if super::scan::daemon_connection(context)?.is_some() {
        return Err(crate::CliError::Precondition(
            "catalog mutation requires the daemon to be stopped".into(),
        )
        .into());
    }
    Ok(())
}
fn entry_json(e: &tidemark_store::catalog::ScanEntry) -> anyhow::Result<serde_json::Value> {
    let file = e
        .file
        .as_ref()
        .map(|p| tidemark_core::path::RawPath::from_path(p))
        .transpose()?;
    let (cold, file_bytes, uncompressed_bytes) = entry_sizes(e);
    Ok(
        serde_json::json!({"cold":cold,"file_bytes":file_bytes,"uncompressed_bytes":uncompressed_bytes,"scan_id":e.scan_id,"root":e.root.to_json(),"mode":e.mode,
        "started_at":e.started_at,"finished_at":e.finished_at,"status":e.status,"degraded":e.degraded,
        "file":file.map(|p|p.to_json()),"t_disk":e.t_disk,"t_inodes":e.t_inodes,"t_dirs":e.t_dirs,
        "errors":e.errors,"duration_ms":e.duration_ms}),
    )
}
pub(crate) fn remove_snapshot(
    state: &std::path::Path,
    file: &std::path::Path,
) -> anyhow::Result<()> {
    let directory = snapshot_directory(state, file)?;
    let name = file
        .file_name()
        .ok_or_else(|| anyhow::anyhow!("missing snapshot filename"))?;
    directory.unlink_leaf(name)?;
    let bytes = name.as_encoded_bytes();
    let stem = bytes
        .strip_suffix(b".db.zst")
        .or_else(|| bytes.strip_suffix(b".db"))
        .ok_or_else(|| anyhow::anyhow!("invalid snapshot filename"))?;
    use std::os::unix::ffi::OsStrExt;
    // Both published forms may exist after a crash; delete them all so a rebuild cannot resurrect the scan.
    for suffix in [
        b".db".as_slice(),
        b".db.zst".as_slice(),
        b".db.bad".as_slice(),
        b".db.zst.bad".as_slice(),
    ] {
        let mut sibling = stem.to_vec();
        sibling.extend_from_slice(suffix);
        directory.unlink_leaf(std::ffi::OsStr::from_bytes(&sibling))?;
    }
    rustix::fs::fsync(directory.fd())?;
    Ok(())
}
fn snapshot_directory(
    state: &std::path::Path,
    file: &std::path::Path,
) -> anyhow::Result<tidemark_store::state_dir::StateDir> {
    let relative = file.strip_prefix(state.join("snapshots"))?;
    let mut parts = relative.components();
    let Some(std::path::Component::Normal(root)) = parts.next() else {
        anyhow::bail!("unsafe snapshot root");
    };
    let Some(std::path::Component::Normal(_)) = parts.next() else {
        anyhow::bail!("unsafe snapshot leaf");
    };
    if parts.next().is_some()
        || !(file.as_os_str().as_encoded_bytes().ends_with(b".db")
            || file.as_os_str().as_encoded_bytes().ends_with(b".db.zst"))
    {
        anyhow::bail!("catalog snapshot path is outside the state snapshot directory");
    }
    Ok(tidemark_store::state_dir::StateDir::open(state, false)?
        .subdir(std::ffi::OsStr::new("snapshots"), false)?
        .subdir(root, false)?)
}
pub(crate) fn open_snapshot(
    context: &crate::Context,
    file: &std::path::Path,
) -> Result<tidemark_store::reader::Snapshot, tidemark_store::StoreError> {
    tidemark_store::reader::Snapshot::open_with_retention(file, &context.config.retention)
}
/// Re-read a catalog row once when its file vanished (a cold publication may have replaced it).
fn refreshed(
    catalog: &tidemark_store::catalog::Catalog,
    e: &tidemark_store::catalog::ScanEntry,
) -> Option<tidemark_store::catalog::ScanEntry> {
    let vanished = e
        .file
        .as_ref()
        .is_some_and(|f| std::fs::symlink_metadata(f).is_err());
    if vanished {
        return catalog.get(&e.scan_id).ok().flatten();
    }
    None
}
/// Sizes are optional metadata: failures yield unknown sizes plus a stderr diagnostic.
fn entry_sizes(e: &tidemark_store::catalog::ScanEntry) -> (bool, Option<u64>, Option<u64>) {
    let Some(file) = &e.file else {
        return (false, None, None);
    };
    let cold = file.as_os_str().as_encoded_bytes().ends_with(b".db.zst");
    let file_bytes = match std::fs::symlink_metadata(file) {
        Ok(m) => m.len(),
        Err(err) => {
            eprintln!(
                "warning: snapshot {}: cannot stat {}: {err}",
                e.scan_id,
                super::display_path(file)
            );
            return (cold, None, None);
        }
    };
    let uncompressed = if cold {
        match tidemark_store::cold::read_cold_header(file) {
            Ok(h) => Some(h.uncompressed_len),
            Err(err) => {
                eprintln!(
                    "warning: snapshot {}: unreadable cold header {}: {err}",
                    e.scan_id,
                    super::display_path(file)
                );
                None
            }
        }
    } else {
        Some(file_bytes)
    };
    (cold, Some(file_bytes), uncompressed)
}
// Bound diagnostic snapshot opens independently of the million-row catalog cap.
const REMEDY_CANDIDATE_LIMIT: usize = 20;

// Keep diagnostic advice aligned with implicit selection, including root specificity.
fn default_entry<'a>(
    entries: &'a [tidemark_store::catalog::ScanEntry],
    path: &tidemark_core::path::RawPath,
) -> Option<&'a tidemark_store::catalog::ScanEntry> {
    entries
        .iter()
        .filter(|e| e.status == "complete" && path.starts_with(&e.root))
        .max_by(|a, b| {
            a.root
                .as_bytes()
                .len()
                .cmp(&b.root.as_bytes().len())
                .then_with(|| a.started_at.cmp(&b.started_at))
                .then_with(|| a.scan_id.cmp(&b.scan_id))
        })
}

/// Resolve a canonical live path; select an explicit snapshot or the latest containing complete one.
pub(crate) fn select(
    context: &crate::Context,
    path: Option<&std::path::Path>,
    id: Option<&str>,
) -> anyhow::Result<(
    tidemark_core::path::RawPath,
    tidemark_store::reader::Snapshot,
)> {
    let subject = path.unwrap_or_else(|| std::path::Path::new("/data"));
    let path = tidemark_fs::canon::canonicalize(subject)
        .with_context(|| format!("resolve query path: {}", super::display_path(subject)))?;
    let catalog = tidemark_store::catalog::Catalog::open(&context.config.general.state_dir, false)
        .with_context(|| {
            format!(
                "open catalog: {}",
                super::display_path(&context.config.general.state_dir)
            )
        })?;
    let entry = if let Some(id) = id {
        catalog.get(id)?
    } else {
        let entries = catalog.list(None)?;
        let selected = default_entry(&entries, &path).map(|entry| entry.scan_id.clone());
        entries
            .into_iter()
            .find(|entry| Some(&entry.scan_id) == selected.as_ref())
    }
    .ok_or_else(|| {
        crate::CliError::Precondition("no matching snapshot; run tidemark scan first".into())
    })?;
    if !path.starts_with(&entry.root) {
        return Err(crate::CliError::Precondition(
            "query path is outside the selected snapshot root".into(),
        )
        .into());
    }
    let file = entry
        .file
        .ok_or_else(|| crate::CliError::Precondition("snapshot has no file".into()))?;
    Ok((
        path,
        open_snapshot(context, &file)
            .with_context(|| format!("open snapshot: {}", super::display_path(&file)))?,
    ))
}

/// Explain an absent path using only recorded snapshot evidence and optional daemon scheduling.
pub(crate) fn missing_path(
    context: &crate::Context,
    path: &tidemark_core::path::RawPath,
    snapshot: &tidemark_store::reader::Snapshot,
) -> crate::CliError {
    let subject = path.display_lossy();
    let mut detail = format!(
        "the selected {} snapshot of {} ({}) does not contain {subject}",
        crate::render::enum_label(snapshot.meta().status),
        snapshot.meta().root.display_lossy(),
        snapshot.meta().scan_id
    );
    if let Ok(root) = snapshot.root() {
        if let Ok(skipped) = snapshot.skipped_under(root.id) {
            for skip in skipped {
                if let Ok(parent) = snapshot.dir(skip.dir_id) {
                    if path.starts_with(&parent.path.join(&skip.name)) {
                        detail.push_str(match skip.reason {
                            tidemark_core::scan::SkipReason::Mount => {
                                " (a mount point; snapshots do not cross mounts)"
                            }
                            tidemark_core::scan::SkipReason::OtherFs => {
                                " (a different filesystem; snapshots do not cross filesystems)"
                            }
                            tidemark_core::scan::SkipReason::Excluded => {
                                " (excluded from this scan)"
                            }
                        });
                        break;
                    }
                }
            }
        }
    }
    // Search recorded complete snapshots, never infer presence from the live path.
    if let Ok(catalog) =
        tidemark_store::catalog::Catalog::open(&context.config.general.state_dir, false)
    {
        if let Ok(mut entries) = catalog.list(None) {
            let default_id = default_entry(&entries, path).map(|entry| entry.scan_id.clone());
            entries.retain(|entry| {
                entry.status == "complete"
                    && entry.started_at > snapshot.meta().started_at
                    && path.starts_with(&entry.root)
            });
            entries.sort_by(|a, b| {
                b.started_at
                    .cmp(&a.started_at)
                    .then_with(|| a.scan_id.cmp(&b.scan_id))
            });
            for entry in entries.into_iter().take(REMEDY_CANDIDATE_LIMIT) {
                let Some(file) = entry.file else { continue };
                let Ok(newer) = open_snapshot(context, &file) else {
                    continue;
                };
                if newer.dir_by_path(path).is_ok_and(|row| row.is_some()) {
                    detail.push_str(&format!(
                        "; newer complete snapshot {} contains this path; use --snapshot {}",
                        entry.scan_id, entry.scan_id
                    ));
                    if default_id.as_ref() == Some(&entry.scan_id) {
                        detail.push_str(" or drop --snapshot");
                    }
                    return crate::CliError::Precondition(detail);
                }
            }
        }
    }
    detail.push_str(&format!("; run: tidemark scan {subject}"));
    if context.config.roots.iter().any(|root| {
        tidemark_core::path::RawPath::from_path(&root.path).is_ok_and(|root| root == *path)
    }) {
        if let Ok(mut client) = crate::client::Client::connect(
            &context.config.general.socket,
            std::time::Duration::from_secs(1),
        ) {
            if let Ok(scans) = client.call::<_, Vec<tidemark_core::live::ScanStatus>>(
                "scan.status",
                &tidemark_core::api::ScanStatusParams { root: None },
            ) {
                if let Some(next) = scans
                    .iter()
                    .find(|scan| scan.root == subject)
                    .and_then(|scan| scan.next_due)
                {
                    detail.push_str(&format!(
                        "; next scheduled scan: {}",
                        crate::render::size::timestamp(next)
                    ));
                }
            }
        }
    }
    crate::CliError::Precondition(detail)
}

#[cfg(test)]
mod export_error_tests {
    use super::export_error;
    use tidemark_store::StoreError;

    #[test]
    fn output_exists_is_precondition_and_io_already_exists_is_not() {
        let err = export_error(StoreError::OutputExists("/tmp/out".into()));
        let cli = err.downcast_ref::<crate::CliError>();
        assert!(
            matches!(cli, Some(crate::CliError::Precondition(message)) if message == "output path already exists: /tmp/out")
        );
        assert_eq!(cli.map(crate::CliError::exit_code), Some(5));

        let err = export_error(StoreError::Io(std::io::Error::new(
            std::io::ErrorKind::AlreadyExists,
            "could not create a unique staging directory",
        )));
        assert!(err.downcast_ref::<crate::CliError>().is_none(), "{err}");
    }
}
