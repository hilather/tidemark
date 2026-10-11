//! Owned by: W4-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Config file to install; global -c names the installed path.
    #[arg(long, value_name = "FILE")]
    pub from: Option<std::path::PathBuf>,
    /// Start the service immediately after staging.
    #[arg(long)]
    pub start_now: bool,
    /// Skip named warning-level pre-flight checks.
    #[arg(long, value_name = "NAME", num_args = 1.., action = clap::ArgAction::Append)]
    pub skip_check: Vec<String>,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W4-B)"))
}
