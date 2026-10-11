//! Owned by: W5-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Mount.
    #[arg(long, value_name = "M")]
    pub mount: Option<std::path::PathBuf>,
    /// Window.
    #[arg(long, value_parser = ["1m", "5m", "15m", "60m"])]
    pub window: Option<String>,
    /// Direct.
    #[arg(long)]
    pub direct: bool,
    /// Interval.
    #[arg(long, value_name = "DUR", default_value = "10s",
        requires = "direct", value_parser = humantime::parse_duration)]
    pub interval: std::time::Duration,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W5-B)"))
}
