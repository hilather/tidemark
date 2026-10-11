//! Owned by: W5-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Filter by mount path.
    #[arg(long, value_name = "M")]
    pub mount: Option<std::path::PathBuf>,
    /// Positive duration over which to measure growth.
    #[arg(long, value_parser = parse_positive_duration)]
    pub window: Option<std::time::Duration>,
    /// Measure growth with two /proc passes without the daemon.
    #[arg(long)]
    pub direct: bool,
    /// Duration between the two direct /proc passes.
    #[arg(long, value_name = "DUR", default_value = "10s",
        requires = "direct", value_parser = humantime::parse_duration)]
    pub interval: std::time::Duration,
}

/// Execute this command (implemented by its later owner).
pub fn run(_args: &Args, _context: &crate::Context) -> anyhow::Result<()> {
    Err(anyhow::anyhow!("not implemented yet (W5-B)"))
}

fn parse_positive_duration(value: &str) -> Result<std::time::Duration, String> {
    let duration = humantime::parse_duration(value).map_err(|error| error.to_string())?;
    if duration.is_zero() {
        return Err("window must be a positive duration".into());
    }
    Ok(duration)
}
