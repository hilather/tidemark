//! Owned by: W5-A
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Mount.
    #[arg(long, value_name = "M")]
    pub mount: Option<std::path::PathBuf>,
    /// Events.
    #[arg(long, value_delimiter = ',', value_parser = ["close_write", "modify"])]
    pub events: Vec<String>,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W5-A)"))
}
