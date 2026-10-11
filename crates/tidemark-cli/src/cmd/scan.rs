//! Owned by: W2-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Root path to scan or select.
    #[arg(value_name = "ROOT")]
    pub root: Option<std::path::PathBuf>,
    /// Scan mode: auto, walk or bulk.
    #[arg(long, value_parser = ["auto", "walk", "bulk"])]
    pub mode: Option<String>,
    /// Scan throttle preset.
    #[arg(long, value_parser = ["gentle", "normal", "max"])]
    pub throttle: Option<String>,
    /// Number of scan worker threads.
    #[arg(long, value_name = "N")]
    pub threads: Option<usize>,
    /// Full-path globs to exclude from the scan.
    #[arg(long, value_name = "GLOB", num_args = 1.., action = clap::ArgAction::Append)]
    pub exclude: Vec<String>,
    /// Keep the partial snapshot if the scan is interrupted.
    #[arg(long)]
    pub keep_partial: bool,
    /// Ask the daemon to cancel the running scan of ROOT.
    #[arg(long)]
    pub cancel: bool,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W2-C)"))
}
