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

/// Run a foreground watcher until SIGINT/SIGTERM, displaying individual directional events.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    use std::{
        io::Write,
        sync::{
            atomic::{AtomicBool, Ordering},
            Arc,
        },
        time::Duration,
    };
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
        let mount = tidemark_fs::canon::canonicalize(&mount)?;
        let options = WatcherOptions {
            mount,
            events: if args.events.is_empty() {
                cfg.events.clone()
            } else {
                args.events.clone()
            },
            hot_depth: cfg.hot_depth as usize,
            exclude_prefixes: exclusions(&cfg.exclude_prefixes, &context.config.general.state_dir)?,
            lru_entries: cfg.lru_entries,
        };
        watchers.push(
            Watcher::start(options, attribution.clone())
                .map_err(|e| crate::CliError::Precondition(format!("watch cannot start: {e}")))?,
        );
    }
    for watcher in &watchers {
        watcher.enable_events();
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
        let mut limiter = RateLimit::default();
        let started = std::time::Instant::now();
        let mut dropped = vec![0u64; watchers.len()];
        while !stopped.load(Ordering::Relaxed) {
            let second = started.elapsed().as_secs().min(i64::MAX as u64) as i64;
            if let Some(omitted) = limiter.advance(second) {
                render_omitted(omitted, context.json, &mut output)?;
            }
            for (index, watcher) in watchers.iter().enumerate() {
                for event in watcher.drain_events(1000) {
                    if limiter.allow() {
                        render_event(&event, context.json, &mut output)?;
                    }
                }
                for row in watcher
                    .drain_completed_minutes(5)
                    .into_iter()
                    .filter(|row| row.scope == "dir")
                {
                    if context.json {
                        crate::render::json("tidemark.watch.v1", &row, &mut output)?;
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
                let stats = watcher.stats();
                let lost = stats.dropped_events.saturating_sub(dropped[index]);
                dropped[index] = stats.dropped_events;
                if lost != 0 {
                    render_omitted(lost, context.json, &mut output)?;
                }
                if stats.mode == tidemark_live::watch::WatchMode::Failed {
                    anyhow::bail!(
                        "watch worker failed after {} transport errors",
                        stats.read_errors
                    );
                }
            }
            output.flush()?;
            std::thread::sleep(Duration::from_millis(100));
        }
        if limiter.omitted != 0 {
            render_omitted(limiter.omitted, context.json, &mut output)?;
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
fn exclusions(
    paths: &[std::path::PathBuf],
    state: &std::path::Path,
) -> anyhow::Result<Vec<tidemark_core::path::RawPath>> {
    let mut result = paths
        .iter()
        .map(|p| tidemark_core::path::RawPath::from_path(p))
        .collect::<Result<Vec<_>, _>>()?;
    result.push(match tidemark_fs::canon::canonicalize(state) {
        Ok(path) => path,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            tidemark_core::path::RawPath::from_path(state)?
        }
        Err(error) => return Err(error.into()),
    });
    Ok(result)
}
#[derive(Default)]
struct RateLimit {
    second: Option<i64>,
    printed: usize,
    omitted: u64,
}
impl RateLimit {
    fn advance(&mut self, second: i64) -> Option<u64> {
        if self.second == Some(second) {
            return None;
        }
        self.second = Some(second);
        self.printed = 0;
        let omitted = std::mem::take(&mut self.omitted);
        (omitted != 0).then_some(omitted)
    }
    fn allow(&mut self) -> bool {
        if self.printed < 50 {
            self.printed += 1;
            true
        } else {
            self.omitted = self.omitted.saturating_add(1);
            false
        }
    }
}
fn render_omitted(count: u64, json: bool, output: &mut impl std::io::Write) -> anyhow::Result<()> {
    if json {
        crate::render::json(
            "tidemark.watch.v1",
            serde_json::json!({"omitted_events": count}),
            output,
        )?;
    } else {
        writeln!(output, "{count} events omitted")?;
    }
    Ok(())
}
fn render_event(
    event: &tidemark_live::watch::WatchEvent,
    json: bool,
    output: &mut impl std::io::Write,
) -> anyhow::Result<()> {
    if json {
        // Reuse the frozen HotEntry path adapter (the core helper is crate-private).
        let mut data = serde_json::to_value(tidemark_core::live::HotEntry {
            key: event.path.as_bytes().to_vec(),
            events: 0,
            files: 0,
            bytes_delta: 0,
            top_unit: None,
            lossy: false,
        })?;
        if let Some(fields) = data.as_object_mut() {
            fields.retain(|key, _| key == "path" || key == "path_b64");
            fields.insert("at".into(), event.at.into());
            fields.insert("delta".into(), event.delta.into());
            fields.insert(
                "attribution".into(),
                serde_json::to_value(&event.attribution)?,
            );
        }
        crate::render::json("tidemark.watch.v1", data, output)?;
    } else {
        writeln!(
            output,
            "{}  {}  {}  pid={} comm={} unit={}",
            event_time(event.at),
            signed_bytes(event.delta),
            event.path.display_lossy(),
            event.attribution.pid,
            tidemark_core::path::display_lossy_bytes(event.attribution.comm.as_bytes()),
            tidemark_core::path::display_lossy_bytes(
                event.attribution.unit.as_deref().unwrap_or("?").as_bytes()
            )
        )?;
    }
    Ok(())
}
fn event_time(at: i64) -> String {
    let seconds = at.rem_euclid(86_400);
    format!(
        "{:02}:{:02}:{:02}",
        seconds / 3600,
        seconds / 60 % 60,
        seconds % 60
    )
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
    fn limiter_and_complete_event_envelopes() {
        let mut limiter = super::RateLimit::default();
        assert_eq!(limiter.advance(10), None);
        for _ in 0..50 {
            assert!(limiter.allow());
        }
        for _ in 0..7 {
            assert!(!limiter.allow());
        }
        assert_eq!(limiter.advance(11), Some(7));
        assert!(limiter.allow());
        for bytes in [
            b"/ascii".to_vec(),
            "/日本".as_bytes().to_vec(),
            vec![b'/', 255],
        ] {
            let event = tidemark_live::watch::WatchEvent {
                at: 42,
                path: tidemark_core::path::RawPath::new(bytes.clone()).unwrap(),
                delta: -512,
                attribution: tidemark_core::live::Attribution {
                    pid: 123,
                    starttime: 1,
                    comm: "writer".into(),
                    unit: Some("writer.service".into()),
                    uid: None,
                },
            };
            let mut output = Vec::new();
            super::render_event(&event, true, &mut output).unwrap();
            assert_eq!(output.iter().filter(|b| **b == b'\n').count(), 1);
            let doc: serde_json::Value = serde_json::from_slice(&output).unwrap();
            assert_eq!(doc["schema"], "tidemark.watch.v1");
            assert_eq!(doc["data"]["at"], 42);
            assert_eq!(doc["data"]["delta"], -512);
            assert_eq!(doc["data"]["attribution"]["pid"], 123);
            if let Ok(path) = std::str::from_utf8(&bytes) {
                assert_eq!(doc["data"]["path"], path);
                assert!(doc["data"].get("path_b64").is_none());
            } else {
                assert_eq!(doc["data"]["path_b64"], "L/8=");
                assert!(doc["data"].get("path").is_none());
            }
            output.clear();
            super::render_event(&event, false, &mut output).unwrap();
            assert!(String::from_utf8(output)
                .unwrap()
                .contains("pid=123 comm=writer unit=writer.service"));
        }
    }
    #[test]
    fn mount_realpath_and_state_exclusion() {
        use std::os::unix::fs::symlink;
        let temp = ::test_support::private_tempdir().unwrap();
        let target = temp.path().join("target/sub");
        std::fs::create_dir_all(&target).unwrap();
        let alias = temp.path().join("alias");
        symlink(&target, &alias).unwrap();
        assert_eq!(
            tidemark_fs::canon::canonicalize(&alias).unwrap().as_path(),
            target
        );
        assert_eq!(
            tidemark_fs::canon::canonicalize(&alias.join(".."))
                .unwrap()
                .as_path(),
            temp.path().join("target")
        );
        assert_eq!(
            tidemark_fs::canon::canonicalize(std::path::Path::new("."))
                .unwrap()
                .as_path(),
            std::env::current_dir().unwrap()
        );
        let excluded = super::exclusions(&[], &alias).unwrap();
        assert!(
            tidemark_core::path::RawPath::from_path(&target.join("file"))
                .unwrap()
                .starts_with(&excluded[0])
        );
        assert!(!tidemark_core::path::RawPath::from_path(
            &temp.path().join("target/submarine/file")
        )
        .unwrap()
        .starts_with(&excluded[0]));
    }
    #[test]
    fn signed_summary() {
        assert_eq!(super::signed_bytes(-1024), "-1.00 KiB");
        assert_eq!(super::signed_bytes(0), "+0 B");
        assert_eq!(super::event_time(3661), "01:01:01");
    }
}
