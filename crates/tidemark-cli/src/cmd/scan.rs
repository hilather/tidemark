//! Owned by: W2-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Root.
    #[arg(value_name = "ROOT")]
    pub root: Option<std::path::PathBuf>,
    /// Mode.
    #[arg(long, value_parser = ["auto", "walk", "bulk"])]
    pub mode: Option<String>,
    /// Throttle.
    #[arg(long, value_parser = ["gentle", "normal", "max"])]
    pub throttle: Option<String>,
    /// Threads.
    #[arg(long, value_name = "N")]
    pub threads: Option<usize>,
    /// Exclude.
    #[arg(long, value_name = "GLOB", num_args = 1.., action = clap::ArgAction::Append)]
    pub exclude: Vec<String>,
    /// Keep partial.
    #[arg(long)]
    pub keep_partial: bool,
    /// Cancel.
    #[arg(long)]
    pub cancel: bool,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W2-C)"))
}
