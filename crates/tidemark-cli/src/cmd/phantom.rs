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
    for row in rows {
        writeln!(
            out,
            "{}: {} bytes in {} deleted open files",
            row.mount, row.total_bytes, row.files
        )?;
        for h in row.holders {
            writeln!(
                out,
                "  {} {} {} {} {} bytes",
                h.pid,
                tidemark_core::path::display_lossy_bytes(h.comm.as_bytes()),
                tidemark_core::path::display_lossy_bytes(
                    h.unit.as_deref().unwrap_or("?").as_bytes()
                ),
                tidemark_core::path::display_lossy_bytes(&h.path),
                h.bytes
            )?;
        }
    }
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
        let raw = RawPath::from_path(&p)?;
        if let Some(m) = table.iter().find(|m| m.mount_point == raw) {
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
