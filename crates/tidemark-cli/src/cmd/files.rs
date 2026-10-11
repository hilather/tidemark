//! Owned by: W2-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory path to query (canonicalised before lookup).
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Maximum number of results.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
    /// Minimum file or directory size.
    #[arg(long, value_name = "SIZE")]
    pub min_size: Option<tidemark_core::units::ByteSize>,
    /// Select files older than this duration.
    #[arg(long, value_name = "DUR", value_parser = humantime::parse_duration)]
    pub older_than: Option<std::time::Duration>,
    /// Select files newer than this duration.
    #[arg(long, value_name = "DUR", value_parser = humantime::parse_duration)]
    pub newer_than: Option<std::time::Duration>,
    /// Snapshot ID to query.
    #[arg(long, value_name = "ID")]
    pub snapshot: Option<String>,
}

/// Execute the command using direct state and snapshot APIs.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let (path, snapshot) =
        super::snapshots::select(context, args.path.as_deref(), args.snapshot.as_deref())?;
    let dir = snapshot
        .dir_by_path(&path)?
        .ok_or_else(|| crate::CliError::Precondition("path not present in snapshot".into()))?;
    let limit = args.limit.unwrap_or(25);
    if limit > 1_000_000 {
        return Err(crate::CliError::Configuration("limit exceeds 1,000,000".into()).into());
    }
    let rows = snapshot.subtree_files(
        dir.id,
        &tidemark_core::snapshot::FileFilter {
            min_size: args.min_size.map(|s| s.0),
            older_than: args.older_than,
            newer_than: args.newer_than,
        },
        limit,
    )?;
    let mut out = std::io::stdout().lock();
    if !context.json {
        crate::render::tree::snapshot_header(
            snapshot.meta(),
            tidemark_core::time::unix_now(),
            &mut out,
        )?;
    }
    crate::render::tree::files(&rows, context.json, &mut out)
}
