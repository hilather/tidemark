//! Owned by: W4-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Also delete state_dir.
    #[arg(long)]
    pub purge_state: bool,
    /// Confirm unstaging without prompting.
    #[arg(long)]
    pub yes: bool,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W4-B)"))
}
