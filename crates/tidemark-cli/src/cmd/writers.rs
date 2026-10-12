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

/// Measure direct growth with two passes, or report daemon unavailability.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    // W6-A3: daemon-first forwarding; explicit direct mode skips the socket.
    if !args.direct {
        match crate::client::Client::connect(
            &context.config.general.socket,
            std::time::Duration::from_secs(11),
        ) {
            Ok(mut client) => {
                let rows: tidemark_core::api::WritersResult = super::now::call(
                    &mut client,
                    "writers",
                    &tidemark_core::api::WritersParams {
                        mount: super::now::mount(args.mount.as_deref())?,
                        window: args.window.unwrap_or(std::time::Duration::from_secs(60)),
                    },
                )?;
                return crate::render::now::writers(
                    &rows,
                    context.json,
                    &mut std::io::stdout().lock(),
                );
            }
            Err(_) => {
                crate::render::now::fallback("writers")?;
                let direct = Args {
                    mount: args.mount.clone(),
                    window: args.window,
                    direct: true,
                    interval: args.interval,
                };
                return run(&direct, context);
            }
        }
    }
    use std::io::Write;
    if !args.direct {
        return Err(crate::CliError::Unavailable("daemon unavailable; use --direct".into()).into());
    }
    if args.interval.is_zero() {
        return Err(crate::CliError::Configuration("interval must be positive".into()).into());
    }
    let mut scanner = super::phantom::scanner(context, args.mount.as_deref(), true)?;
    let first = super::phantom::timestamp()?;
    scanner.scan_once(first)?;
    std::thread::sleep(args.interval);
    scanner.scan_once(super::phantom::timestamp()?)?;
    let rows = scanner.writers(None, args.window.unwrap_or(args.interval), 20);
    let mut out = std::io::stdout().lock();
    if context.json {
        return crate::render::json("tidemark.writers.v1", rows, &mut out);
    }
    for r in rows {
        writeln!(
            out,
            "{} {} {} {} {} +{} bytes ({} bytes allocated)",
            r.mount,
            r.pid,
            tidemark_core::path::display_lossy_bytes(r.comm.as_bytes()),
            tidemark_core::path::display_lossy_bytes(r.unit.as_deref().unwrap_or("?").as_bytes()),
            tidemark_core::path::display_lossy_bytes(&r.path),
            r.growth,
            r.disk
        )?;
    }
    writeln!(
        out,
        "Root provides full /proc coverage; other users see the readable subset."
    )?;
    Ok(())
}

fn parse_positive_duration(value: &str) -> Result<std::time::Duration, String> {
    let duration = humantime::parse_duration(value).map_err(|error| error.to_string())?;
    if duration.is_zero() {
        return Err("window must be a positive duration".into());
    }
    Ok(duration)
}
