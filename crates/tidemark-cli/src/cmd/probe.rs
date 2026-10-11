//! Owned by: W1-A
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Root paths to probe for capabilities.
    #[arg(long, value_name = "PATH", num_args = 1..)]
    pub root: Vec<std::path::PathBuf>,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W1-A)"))
}
