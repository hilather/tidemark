//! Owned by: W3-C
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
    /// ncdu JSON export.
    Ncdu {
        /// Directory path to query (canonicalised before lookup).
        #[arg(value_name = "PATH")]
        path: Option<std::path::PathBuf>,
        /// Write ncdu JSON to this file.
        #[arg(short = 'o', value_name = "FILE")]
        output: Option<std::path::PathBuf>,
        /// Snapshot ID to query.
        #[arg(long, value_name = "ID")]
        snapshot: Option<String>,
        /// Replace invalid UTF-8 in exported names.
        #[arg(long)]
        utf8_lossy: bool,
        /// Maximum directory depth to export.
        #[arg(long, value_name = "N")]
        max_depth: Option<usize>,
    },
}
/// Execute the operation.
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W3-C)"))
}
