//! Owned by: W2-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory path to query (canonicalised before lookup).
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Directory depth to display.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub depth: usize,
    /// Sort directories by the selected metric.
    #[arg(long, value_enum)]
    pub sort: Option<Sort>,
    /// Maximum number of results.
    #[arg(long, value_name = "N", default_value_t = 25)]
    pub limit: usize,
    /// Minimum file or directory size.
    #[arg(long, value_name = "SIZE")]
    pub min_size: Option<tidemark_core::units::ByteSize>,
    /// Snapshot ID to query.
    #[arg(long, value_name = "ID")]
    pub snapshot: Option<String>,
    /// Add NEWEST, INODES and APPARENT columns.
    #[arg(long)]
    pub wide: bool,
    /// Rescan PATH now, throttled, and show deltas versus the snapshot.
    #[arg(long)]
    pub live: bool,
}

/// Execute the command using direct state and snapshot APIs.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    use tidemark_core::snapshot::SortKey;
    let (path, snapshot) =
        super::snapshots::select(context, args.path.as_deref(), args.snapshot.as_deref())?;
    let dir = snapshot
        .dir_by_path(&path)?
        .ok_or_else(|| crate::CliError::Precondition("path not present in snapshot".into()))?;
    if args.limit > 1_000_000 {
        return Err(crate::CliError::Configuration("limit exceeds 1,000,000".into()).into());
    }
    let depth = u32::try_from(args.depth)
        .map_err(|_| crate::CliError::Configuration("depth exceeds u32".into()))?;
    let sort = args.sort.unwrap_or(Sort::Disk);
    let key = match sort {
        Sort::Disk | Sort::Growth => SortKey::Disk,
        Sort::Apparent => SortKey::Apparent,
        Sort::Reclaimable => SortKey::Reclaimable,
        Sort::Charged => SortKey::Charged,
        Sort::Files => SortKey::Files,
        Sort::Inodes => SortKey::Inodes,
        Sort::Newest => SortKey::Newest,
        Sort::Name => SortKey::Name,
    };
    let catalog = tidemark_store::catalog::Catalog::open(&context.config.general.state_dir, false)?;
    let old = older_snapshot(&catalog, &snapshot, tidemark_core::time::unix_now())?;
    if sort == Sort::Growth && old.is_none() {
        return Err(crate::CliError::Precondition(
            "growth sorting requires an older complete snapshot within 6h of now minus 24h".into(),
        )
        .into());
    }
    // Apply filters before the final limit, especially for cross-snapshot growth.
    let rows = snapshot.subtree_dirs(dir.id, depth, key, 1_000_001)?;
    let live = if args.live {
        Some(live_rows(&path, context)?)
    } else {
        None
    };
    let mut hits = Vec::with_capacity(rows.len());
    for row in rows
        .into_iter()
        .filter(|r| r.t_disk >= args.min_size.map(|s| s.0).unwrap_or(0))
    {
        let delta = if let Some(old) = &old {
            Some(
                i128::from(row.t_disk)
                    - i128::from(old.dir_by_path(&row.path)?.map(|r| r.t_disk).unwrap_or(0)),
            )
        } else {
            None
        };
        let live_disk = live.as_ref().and_then(|rows| rows.get(&row.path)).copied();
        hits.push(crate::render::tree::Directory {
            row,
            delta,
            live_disk,
        });
    }
    if sort == Sort::Growth {
        hits.sort_by(|a, b| {
            b.delta
                .cmp(&a.delta)
                .then_with(|| a.row.path.cmp(&b.row.path))
        });
    }
    hits.truncate(args.limit);
    let mut out = std::io::stdout().lock();
    if !context.json {
        crate::render::tree::snapshot_header(
            snapshot.meta(),
            tidemark_core::time::unix_now(),
            &mut out,
        )?;
    }
    crate::render::tree::directories(
        &hits,
        args.wide,
        crate::render::tree::ascii_locale(),
        context.json,
        &mut out,
    )
}
fn older_snapshot(
    catalog: &tidemark_store::catalog::Catalog,
    current: &tidemark_store::reader::Snapshot,
    now: i64,
) -> anyhow::Result<Option<tidemark_store::reader::Snapshot>> {
    let target = now.saturating_sub(86400);
    let entries = catalog.list(Some(&current.meta().root))?;
    let entry = entries
        .into_iter()
        .filter(|e| {
            e.status == "complete"
                && e.file.as_ref().is_some_and(|p| p.is_file())
                && e.scan_id != current.meta().scan_id.to_string()
                && e.started_at < current.meta().started_at
                && e.started_at.abs_diff(target) <= 21600
        })
        .min_by(|a, b| {
            a.started_at
                .abs_diff(target)
                .cmp(&b.started_at.abs_diff(target))
                .then_with(|| b.started_at.cmp(&a.started_at))
                .then_with(|| a.scan_id.cmp(&b.scan_id))
        });
    entry
        .and_then(|e| e.file)
        .map(|p| tidemark_store::reader::Snapshot::open(&p).map_err(Into::into))
        .transpose()
}
fn live_rows(
    path: &tidemark_core::path::RawPath,
    context: &crate::Context,
) -> anyhow::Result<std::collections::BTreeMap<tidemark_core::path::RawPath, u64>> {
    use tidemark_scan::agg::{AggConfig, Aggregator, VecSink};
    let _lock = tidemark_scan::orchestrator::ScanLock::acquire(&context.config.general.state_dir)
        .map_err(super::scan::scan_error)?;
    let mounts = tidemark_fs::mount::read_mountinfo()?;
    let mount = tidemark_fs::mount::mount_for_path(&mounts, path)
        .ok_or_else(|| crate::CliError::Precondition("live root mount unknown".into()))?;
    let leaves = tidemark_fs::blockdev::resolve_leaves(mount.dev).unwrap_or_default();
    let sample_leaves = leaves.clone();
    let scan_gate = tidemark_fs::throttle::gate_from_config(
        "gentle",
        &context.config.throttle,
        &leaves,
        Box::new(move || tidemark_fs::blockdev::read_disk_stats(&sample_leaves)),
    )?;
    let gate = scan_gate.gate();
    let root = context
        .config
        .roots
        .iter()
        .find(|r| r.path == path.as_path())
        .cloned()
        .unwrap_or_default();
    let now = tidemark_core::time::unix_now();
    let threads = context.config.scan.threads as usize;
    let capacity = tidemark_scan::orchestrator::channel_capacity(
        context.config.scan.memory_budget.0,
        threads,
        root.top_files,
        context.config.scan.getdents_buf.0,
    )?;
    let mut globs = globset::GlobSetBuilder::new();
    for g in &root.exclude {
        globs.add(globset::Glob::new(g)?);
    }
    let mut skips = tidemark_fs::mount::submounts_under(&mounts, path);
    let state = tidemark_fs::canon::canonicalize(&context.config.general.state_dir)?;
    if &state == path {
        return Err(crate::CliError::Precondition("cannot scan state directory".into()).into());
    }
    if state.starts_with(path) {
        skips.push(state);
    }
    let cfg = tidemark_fs::walk::WalkConfig {
        root: path.clone(),
        threads,
        stat_entries: true,
        exclude: globs.build()?,
        skip_mounts: skips,
        big_file_threshold: root.big_file_threshold.0,
        top_files: root.top_files,
        scan_start: now,
        getdents_buf: usize::try_from(context.config.scan.getdents_buf.0)?,
        io_idle: true,
        max_record_bytes: tidemark_scan::orchestrator::record_limits(
            context.config.scan.memory_budget.0,
            threads,
        )?
        .1,
    };
    let mut agg = Aggregator::new(AggConfig {
        root: path.clone(),
        scan_start: now,
        top_files: root.top_files,
        big_file_threshold: root.big_file_threshold.0,
        max_notable: context.config.scan.max_notable,
        link_instances: false,
        max_stored_errors: context.config.scan.max_stored_errors,
    });
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sig = signal_hook::flag::register(signal_hook::consts::SIGINT, cancel.clone())?;
    let walked = std::thread::scope(|scope| {
        let (tx, rx) = crossbeam_channel::bounded(capacity);
        let worker = scope.spawn(|| tidemark_fs::walk::walk(&cfg, gate.as_ref(), &cancel, tx));
        for record in rx {
            match record {
                tidemark_core::scan::WalkOutput::Dir(d) => agg.add_dir_record(d),
                tidemark_core::scan::WalkOutput::Gone(g) => agg.add_gone(&g),
                _ => {}
            }
        }
        worker
            .join()
            .map_err(|_| anyhow::anyhow!("live walker panicked"))?
            .map_err(anyhow::Error::from)
    });
    signal_hook::low_level::unregister(sig);
    walked?;
    scan_gate.stop();
    let mut sink = VecSink::default();
    agg.prepare()?.write(&mut sink)?;
    Ok(sink.dirs.into_iter().map(|r| (r.path, r.t_disk)).collect())
}

/// CLI directory ordering, including the cross-snapshot growth metric.
#[derive(Debug, Clone, Copy, PartialEq, Eq, clap::ValueEnum)]
pub enum Sort {
    /// Allocated disk bytes.
    Disk,
    /// Apparent bytes.
    Apparent,
    /// Reclaimable disk bytes.
    Reclaimable,
    /// Charged disk bytes.
    Charged,
    /// File count.
    Files,
    /// Inode count.
    Inodes,
    /// Newest modification time.
    Newest,
    /// Raw path name.
    Name,
    /// Signed disk-byte growth compared with the older complete snapshot.
    Growth,
}
