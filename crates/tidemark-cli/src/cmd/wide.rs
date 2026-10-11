//! Owned by: W3-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory path to query (canonicalised before lookup).
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Minimum number of directory entries.
    #[arg(long, value_name = "N", default_value_t = 10000)]
    pub min_entries: usize,
    /// Maximum number of results.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

/// Execute this command against the latest complete snapshot.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let min = u32::try_from(args.min_entries)
        .map_err(|_| crate::CliError::Configuration("min-entries exceeds u32".into()))?;
    let limit = args.limit.unwrap_or(25);
    if limit > 1_000_000 {
        return Err(crate::CliError::Configuration("limit exceeds 1,000,000".into()).into());
    }
    let (path, snapshot) = super::snapshots::select(context, args.path.as_deref(), None)?;
    let dir = snapshot
        .dir_by_path(&path)?
        .ok_or_else(|| crate::CliError::Precondition("path not present in snapshot".into()))?;
    let rows = snapshot.wide_dirs(dir.id, min, limit)?;
    let mut out = std::io::stdout().lock();
    if !context.json {
        crate::render::tree::snapshot_header(
            snapshot.meta(),
            tidemark_core::time::unix_now(),
            &mut out,
        )?;
    }
    crate::render::wide::directories(&rows, context.json, &mut out)
}
