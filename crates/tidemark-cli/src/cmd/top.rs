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

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W2-C)"))
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
