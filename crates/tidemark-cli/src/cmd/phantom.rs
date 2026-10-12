//! Owned by: W5-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Filter by mount path.
    #[arg(long, value_name = "M")]
    pub mount: Option<std::path::PathBuf>,
    /// Scan /proc now without the daemon.
    #[arg(long)]
    pub direct: bool,
}

/// Run a direct metadata pass or report daemon unavailability until Wave 6.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    // W6-A3: daemon-first forwarding; explicit direct mode skips the socket.
    if !args.direct {
        match crate::client::Client::connect(
            &context.config.general.socket,
            std::time::Duration::from_secs(11),
        ) {
            Ok(mut client) => {
                let rows: tidemark_core::api::PhantomResult = super::now::call(
                    &mut client,
                    "phantom",
                    &tidemark_core::api::PhantomParams {
                        mount: super::now::mount(args.mount.as_deref())?,
                    },
                )?;
                return crate::render::now::phantom(
                    &rows,
                    context.json,
                    &mut std::io::stdout().lock(),
                );
            }
            Err(_) => {
                crate::render::now::fallback("phantom")?;
                let direct = Args {
                    mount: args.mount.clone(),
                    direct: true,
                };
                return run(&direct, context);
            }
        }
    }
    use std::io::Write;
    if !args.direct {
        return Err(crate::CliError::Unavailable("daemon unavailable; use --direct".into()).into());
    }
    let mut scanner = scanner(context, args.mount.as_deref(), false)?;
    scanner.scan_once(timestamp()?)?;
    let rows = scanner.phantom(None);
    let mut out = std::io::stdout().lock();
    if context.json {
        return crate::render::json("tidemark.phantom.v1", rows, &mut out);
    }
    crate::render::now::phantom(&rows, false, &mut out)?;
    writeln!(
        out,
        "Root provides full /proc coverage; other users see the readable subset."
    )?;
    writeln!(
        out,
        "Consider restarting or HUP-ing the holder to release deleted files."
    )?;
    Ok(())
}

pub(super) fn timestamp() -> anyhow::Result<i64> {
    Ok(i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    )?)
}

pub(super) fn scanner(
    context: &crate::Context,
    mount: Option<&std::path::Path>,
    writers: bool,
) -> anyhow::Result<tidemark_live::procscan::ProcScanner> {
    use tidemark_core::path::RawPath;
    let table = tidemark_fs::mount::read_mountinfo()?;
    let paths: Vec<_> = mount.map_or_else(
        || context.config.watch.mounts.clone(),
        |p| vec![p.to_path_buf()],
    );
    let mut mounts = Vec::new();
    for p in paths {
        let raw = canonical_mount(&p)?;
        if let Some(m) = visible_mount(&table, &raw) {
            if !mounts.iter().any(|(_, d)| *d == m.dev) {
                mounts.push((raw, m.dev));
            }
        } else if mount.is_some() {
            return Err(crate::CliError::Configuration(format!(
                "{} is not a mounted filesystem",
                raw.display_lossy()
            ))
            .into());
        }
    }
    let exclude_prefixes = context
        .config
        .procscan
        .exclude_prefixes
        .iter()
        .map(|p| RawPath::from_path(p))
        .collect::<Result<Vec<_>, _>>()?;
    Ok(tidemark_live::procscan::ProcScanner::new(
        tidemark_live::procscan::ProcScannerOptions {
            mounts,
            open_writers: writers,
            exclude_prefixes,
        },
        std::sync::Arc::new(tidemark_fs::attribution::AttributionCache::new(
            std::time::Duration::from_secs(60),
            10_000,
        )),
    ))
}

#[cfg(test)]
#[path = "../../tests/w5b/commands.rs"]
mod tests;

fn canonical_mount(path: &std::path::Path) -> anyhow::Result<tidemark_core::path::RawPath> {
    Ok(tidemark_core::path::RawPath::from_path(
        &std::fs::canonicalize(path)?,
    )?)
}
fn visible_mount<'a>(
    table: &'a [tidemark_fs::mount::MountInfo],
    raw: &tidemark_core::path::RawPath,
) -> Option<&'a tidemark_fs::mount::MountInfo> {
    tidemark_fs::mount::mount_for_path(table, raw).filter(|m| m.mount_point == *raw)
}
