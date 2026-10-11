//! Owned by: W2-C
//! Command surface and shared CLI support.

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
    /// Rebuild the snapshot catalog.
    RebuildCatalog {
        /// Root path to scan or select.
        #[arg(long, value_name = "ROOT")]
        root: Option<std::path::PathBuf>,
    },
}
/// Execute the operation.
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W2-C)"))
}
