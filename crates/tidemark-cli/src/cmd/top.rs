//! Owned by: W2-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Path.
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Depth.
    #[arg(long, value_name = "N", default_value_t = 1)]
    pub depth: usize,
    /// Sort.
    #[arg(long, value_parser = ["disk", "apparent", "reclaimable", "charged", "files", "inodes", "newest", "name", "growth"])]
    pub sort: Option<String>,
    /// Limit.
    #[arg(long, value_name = "N", default_value_t = 25)]
    pub limit: usize,
    /// Min size.
    #[arg(long, value_name = "SIZE")]
    pub min_size: Option<tidemark_core::units::ByteSize>,
    /// Snapshot.
    #[arg(long, value_name = "ID")]
    pub snapshot: Option<String>,
    /// Wide.
    #[arg(long)]
    pub wide: bool,
    /// Live.
    #[arg(long)]
    pub live: bool,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W2-C)"))
}
