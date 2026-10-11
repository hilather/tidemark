//! Owned by: W5-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Relations.
    #[arg(long)]
    pub relations: bool,
    /// Db.
    #[arg(long, value_name = "NAME")]
    pub db: Option<String>,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W5-C)"))
}
