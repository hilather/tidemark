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
    /// Prune the snapshot catalog.
    Prune {
        /// Root path to scan or select.
        #[arg(long, value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
        /// Show snapshots that would be pruned without deleting them.
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
        Command::List { root } | Command::Show { root, .. } => (root, false),
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
                        .map(entry_json)
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
                ]];
                for e in rows {
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
                    tidemark_store::reader::Snapshot::open(file)
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
        Command::Prune { dry_run, .. } => {
            let entries = catalog.list(root.as_ref())?;
            let mut ids = Vec::new();
            // The retention planner already groups by root and protects its latest complete scan.
            for id in tidemark_store::retention::retention_plan(
                &entries,
                &context.config.retention.snapshots,
                tidemark_core::time::unix_now(),
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
            if context.json {
                crate::render::json(
                    "tidemark.snapshots.v1",
                    serde_json::json!({"dry_run":dry_run,"pruned":ids}),
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
    Ok(
        serde_json::json!({"scan_id":e.scan_id,"root":e.root.to_json(),"mode":e.mode,
        "started_at":e.started_at,"finished_at":e.finished_at,"status":e.status,"degraded":e.degraded,
        "file":file.map(|p|p.to_json()),"t_disk":e.t_disk,"t_inodes":e.t_inodes,"t_dirs":e.t_dirs,
        "errors":e.errors,"duration_ms":e.duration_ms}),
    )
}
pub(crate) fn remove_snapshot(
    state: &std::path::Path,
    file: &std::path::Path,
) -> anyhow::Result<()> {
    let parent = file
        .parent()
        .ok_or_else(|| crate::CliError::Precondition("snapshot has no parent".into()))?;
    let allowed = std::fs::canonicalize(state.join("snapshots")).with_context(|| {
        format!(
            "resolve snapshot directory: {}",
            super::display_path(&state.join("snapshots"))
        )
    })?;
    let parent = std::fs::canonicalize(parent)
        .with_context(|| format!("resolve parent: {}", super::display_path(parent)))?;
    if !parent.starts_with(&allowed) || file.extension() != Some(std::ffi::OsStr::new("db")) {
        return Err(crate::CliError::Precondition(
            "catalog snapshot path is outside the state snapshot directory".into(),
        )
        .into());
    }
    match std::fs::remove_file(file) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(anyhow::Error::new(e)
            .context(format!("remove snapshot: {}", super::display_path(file)))),
    }
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
        entries
            .into_iter()
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
        tidemark_store::reader::Snapshot::open(&file)
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
