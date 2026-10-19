//! Owned by: W2-C
//! Command surface and shared CLI support.

use anyhow::Context as _;

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
        .ok_or_else(|| super::snapshots::missing_path(context, &path, &snapshot))?;
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
    let catalog = tidemark_store::catalog::Catalog::open(&context.config.general.state_dir, false)
        .with_context(|| {
            format!(
                "open catalog: {}",
                super::display_path(&context.config.general.state_dir)
            )
        })?;
    // Plain top uses the ~24h snapshot only for Δ24h. A free-space refusal
    // leaves that column blank, the way the TUI does. `--snapshot` and
    // `--sort growth` still require the open to succeed.
    let soft_delta = args.snapshot.is_none() && args.sort != Some(Sort::Growth);
    let (old, blank_delta) = match older_snapshot(
        &catalog,
        &snapshot,
        tidemark_core::time::unix_now(),
        &context.config.retention,
    ) {
        Ok(old) => (old, false),
        Err(OlderFailure {
            scan_id: Some(scan_id),
            ..
        }) if soft_delta => {
            warn_delta_skipped(&scan_id, context.config.retention.min_free_pct);
            (None, true)
        }
        Err(failure) => return Err(failure.error),
    };
    if sort == Sort::Growth && old.is_none() {
        return Err(crate::CliError::Precondition(
            "growth sorting requires an older complete snapshot within 6h of now minus 24h".into(),
        )
        .into());
    }
    // Apply filters before the final limit, especially for cross-snapshot growth.
    let rows = snapshot.subtree_dirs(dir.id, depth, key, 1_000_001)?;
    let live = if args.live {
        let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
        let sig = signal_hook::flag::register(signal_hook::consts::SIGINT, cancel.clone())?;
        let result = crate::live_rows::live_rows(&path, &context.config, &cancel);
        signal_hook::low_level::unregister(sig);
        Some(
            result?
                .into_iter()
                .map(|r| (r.path, r.t_disk))
                .collect::<std::collections::BTreeMap<_, _>>(),
        )
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
            delta_skipped: blank_delta.then_some("free_space_floor"),
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
        blank_delta,
        &mut out,
    )
}
/// One stderr line when plain `top` leaves Δ24h blank after a floor refusal.
fn warn_delta_skipped(scan_id: &str, min_free_pct: f64) {
    eprintln!(
        "warning: Δ24h skipped: inflating {scan_id} would breach min_free_pct={min_free_pct}%"
    );
}
struct OlderFailure {
    scan_id: Option<String>,
    error: anyhow::Error,
}
fn older_snapshot(
    catalog: &tidemark_store::catalog::Catalog,
    current: &tidemark_store::reader::Snapshot,
    now: i64,
    policy: &tidemark_core::config::RetentionConfig,
) -> Result<Option<tidemark_store::reader::Snapshot>, OlderFailure> {
    let target = now.saturating_sub(86400);
    let entries = match catalog.list(Some(&current.meta().root)) {
        Ok(entries) => entries,
        Err(error) => {
            return Err(OlderFailure {
                scan_id: None,
                error: error.into(),
            })
        }
    };
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
    let Some(entry) = entry else {
        return Ok(None);
    };
    let Some(path) = entry.file else {
        return Ok(None);
    };
    match tidemark_store::reader::Snapshot::open_with_retention(&path, policy) {
        Ok(snapshot) => Ok(Some(snapshot)),
        Err(error @ tidemark_store::StoreError::Space(_)) => Err(OlderFailure {
            scan_id: Some(entry.scan_id),
            error: anyhow::Error::from(error).context(format!(
                "open older snapshot: {}",
                super::display_path(&path)
            )),
        }),
        Err(error) => Err(OlderFailure {
            scan_id: None,
            error: anyhow::Error::from(error).context(format!(
                "open older snapshot: {}",
                super::display_path(&path)
            )),
        }),
    }
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
