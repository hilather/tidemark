//! Owned by: W4-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Purge state.
    #[arg(long)]
    pub purge_state: bool,
    /// Yes.
    #[arg(long)]
    pub yes: bool,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W4-B)"))
}
