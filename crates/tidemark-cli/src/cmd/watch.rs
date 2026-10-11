//! Owned by: W5-A
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Filter by mount path.
    #[arg(long, value_name = "M")]
    pub mount: Option<std::path::PathBuf>,
    /// Comma-separated events to watch.
    #[arg(long, value_delimiter = ',', value_parser = ["close_write", "modify"])]
    pub events: Vec<String>,
}

/// Run a foreground watcher until SIGINT/SIGTERM, displaying completed-minute leaders.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    use std::{
        io::Write,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::Duration,
    };
    use tidemark_core::path::RawPath;
    use tidemark_live::watch::{Watcher, WatcherOptions};
    let cfg = &context.config.watch;
    let mounts = args
        .mount
        .as_ref()
        .map_or_else(|| cfg.mounts.clone(), |p| vec![p.clone()]);
    let attribution = Arc::new(tidemark_fs::attribution::AttributionCache::new(
        Duration::from_secs(60),
        10_000,
    ));
    let mut watchers = Vec::new();
    for mount in mounts {
        let mount = RawPath::from_path(&mount)?;
        let options = WatcherOptions {
            mount,
            events: if args.events.is_empty() {
                cfg.events.clone()
            } else {
                args.events.clone()
            },
            hot_depth: cfg.hot_depth as usize,
            exclude_prefixes: cfg
                .exclude_prefixes
                .iter()
                .map(|p| RawPath::from_path(p))
                .collect::<Result<_, _>>()?,
            lru_entries: cfg.lru_entries,
        };
        watchers.push(
            Watcher::start(options, attribution.clone())
                .map_err(|e| crate::CliError::Precondition(format!("watch cannot start: {e}")))?,
        );
    }
    let stopped = Arc::new(AtomicBool::new(false));
    let interrupt = signal_hook::flag::register(signal_hook::consts::SIGINT, stopped.clone())?;
    let terminate = match signal_hook::flag::register(signal_hook::consts::SIGTERM, stopped.clone())
    {
        Ok(id) => id,
        Err(e) => {
            signal_hook::low_level::unregister(interrupt);
            return Err(e.into());
        }
    };
    let result = (|| -> anyhow::Result<()> {
        let stdout = std::io::stdout();
        let mut output = stdout.lock();
        while !stopped.load(Ordering::Relaxed) {
            for watcher in &watchers {
                for row in watcher
                    .drain_completed_minutes(5)
                    .into_iter()
                    .filter(|r| r.scope == "dir")
                {
                    if context.json {
                        writeln!(output, "{}", serde_json::to_string(&row)?)?;
                    } else {
                        writeln!(
                            output,
                            "{}  {}  {}  ({} events{})",
                            row.ts,
                            signed_bytes(row.bytes_delta),
                            tidemark_core::path::display_lossy_bytes(&row.key),
                            row.events,
                            if row.lossy { ", lossy" } else { "" }
                        )?;
                    }
                }
            }
            output.flush()?;
            std::thread::sleep(Duration::from_millis(100));
        }
        Ok(())
    })();
    signal_hook::low_level::unregister(interrupt);
    signal_hook::low_level::unregister(terminate);
    for watcher in watchers {
        watcher.stop();
    }
    result
}
fn signed_bytes(bytes: i64) -> String {
    format!(
        "{}{}",
        if bytes < 0 { "-" } else { "+" },
        tidemark_core::units::fmt_bytes(bytes.unsigned_abs())
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn signed_summary() {
        assert_eq!(super::signed_bytes(-1024), "-1.00 KiB");
        assert_eq!(super::signed_bytes(0), "+0 B");
    }
}
