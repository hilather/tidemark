//! Owned by: W5-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Include Postgres relations.
    #[arg(long)]
    pub relations: bool,
    /// Select the Postgres database.
    #[arg(long, value_name = "NAME")]
    pub db: Option<String>,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W5-C)"))
}
