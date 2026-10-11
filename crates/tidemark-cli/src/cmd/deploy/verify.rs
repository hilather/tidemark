//! Owned by: W4-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Poll until healthy or this duration expires.
    #[arg(long, value_name = "DUR", value_parser = humantime::parse_duration)]
    pub wait: Option<std::time::Duration>,
}

/// Verify the service, staged report and API status, returning exit 6 until healthy.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let api = crate::deploy::SocketPing {
        socket: context.config.general.socket.clone(),
        timeout: std::time::Duration::from_secs(10),
    };
    let result = crate::deploy::verify(
        &context.config.general.state_dir,
        args.wait.unwrap_or_default(),
        &crate::deploy::RealSystemctl,
        &api,
    )?;
    super::render::verified(&result, context.json)?;
    if result.healthy {
        Ok(())
    } else {
        Err(crate::CliError::Unhealthy("deployment not healthy".into()).into())
    }
}
