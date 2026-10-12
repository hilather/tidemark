//! Owned by: W2-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Root path to scan or select.
    #[arg(value_name = "ROOT")]
    pub root: Option<std::path::PathBuf>,
    /// Scan mode: auto, walk or bulk; bulk apparent sizes of files still being written may lag.
    #[arg(long, value_parser = ["auto", "walk", "bulk"])]
    pub mode: Option<String>,
    /// Scan throttle preset.
    #[arg(long, value_parser = ["gentle", "normal", "max"])]
    pub throttle: Option<String>,
    /// Number of scan worker threads.
    #[arg(long, value_name = "N")]
    pub threads: Option<usize>,
    /// Full-path globs to exclude from the scan.
    #[arg(long, value_name = "GLOB", num_args = 1.., action = clap::ArgAction::Append)]
    pub exclude: Vec<String>,
    /// Keep the partial snapshot if a walk-mode scan is interrupted (walk mode only).
    #[arg(long)]
    pub keep_partial: bool,
    /// Ask the daemon to cancel the running scan of ROOT.
    #[arg(long)]
    pub cancel: bool,
}

/// Execute the command using direct state and snapshot APIs.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    // W6-A3: daemon-first forwarding; direct implementation below is retained.
    if let Ok(mut client) = crate::client::Client::connect(
        &context.config.general.socket,
        std::time::Duration::from_secs(11),
    ) {
        use tidemark_core::api::*;
        if args.threads.is_some() || !args.exclude.is_empty() || args.keep_partial {
            return Err(crate::CliError::Configuration("daemon scans use configured threads/excludes and discard partial scans; stop the daemon to use direct overrides".into()).into());
        }
        let root = tidemark_fs::canon::canonicalize(
            args.root
                .as_deref()
                .unwrap_or_else(|| std::path::Path::new("/data")),
        )?;
        if args.cancel {
            let result: ScanCancelResult =
                super::now::call(&mut client, "scan.cancel", &ScanCancelParams { root })?;
            return crate::render::status::scan_cancel(
                &result,
                context.json,
                &mut std::io::stdout().lock(),
            );
        }
        let mode = args
            .mode
            .as_deref()
            .map(|s| serde_json::from_value(serde_json::Value::String(s.into())))
            .transpose()?;
        let throttle = args
            .throttle
            .as_deref()
            .map(|s| serde_json::from_value(serde_json::Value::String(s.into())))
            .transpose()?;
        let interrupt = super::now::Interrupt::new()?;
        let started: ScanStartResult = super::now::call(
            &mut client,
            "scan.start",
            &ScanStartParams {
                root: root.clone(),
                mode,
                throttle,
            },
        )?;
        loop {
            if interrupt.flag.load(std::sync::atomic::Ordering::Acquire) {
                let _: ScanCancelResult = super::now::call(
                    &mut client,
                    "scan.cancel",
                    &ScanCancelParams { root: root.clone() },
                )?;
                return Err(
                    crate::CliError::Partial("daemon scan cancellation requested".into()).into(),
                );
            }
            let statuses: ScanStatusResult = super::now::call(
                &mut client,
                "scan.status",
                &ScanStatusParams {
                    root: Some(root.clone()),
                },
            )?;
            if let Some(status) = statuses.first() {
                if status.running.is_none() {
                    if status
                        .last_complete
                        .as_ref()
                        .is_some_and(|s| s.scan_id == started.scan_id)
                    {
                        return crate::render::status::scan_complete(
                            status,
                            context.json,
                            &mut std::io::stdout().lock(),
                        );
                    }
                    return Err(crate::CliError::Partial(
                        "daemon scan failed or was cancelled".into(),
                    )
                    .into());
                }
            }
            std::thread::park_timeout(std::time::Duration::from_millis(100));
        }
    }
    use std::sync::{atomic::AtomicBool, Arc};
    use tidemark_scan::{
        mode::ModeRequest,
        orchestrator::{run_scan, ScanRequest},
    };
    if args.threads.is_some_and(|n| !(1..=64).contains(&n)) {
        return Err(crate::CliError::Configuration("threads must be 1..=64".into()).into());
    }
    for pattern in &args.exclude {
        globset::Glob::new(pattern).map_err(|e| crate::CliError::Configuration(e.to_string()))?;
    }
    if args.cancel {
        return Err(crate::CliError::Unavailable(
            "scan --cancel requires the daemon API (W6-A)".into(),
        )
        .into());
    }
    if std::os::unix::net::UnixStream::connect(&context.config.general.socket).is_ok() {
        return Err(crate::CliError::Precondition(
            "direct scan requires the daemon to be stopped; daemon forwarding belongs to W6-A"
                .into(),
        )
        .into());
    }
    {
        // Recover only in direct mode, under the scan lock, before any recorder
        // or catalog connections are retained.
        let _lock =
            tidemark_scan::orchestrator::ScanLock::acquire(&context.config.general.state_dir)
                .map_err(scan_error)?;
        if std::os::unix::net::UnixStream::connect(&context.config.general.socket).is_ok() {
            return Err(crate::CliError::Precondition(
                "direct scan requires the daemon to be stopped".into(),
            )
            .into());
        }
        drop(
            tidemark_scan::orchestrator::open_catalog(&context.config.general.state_dir, true)
                .map_err(scan_error)?,
        );
    }
    let path = args
        .root
        .as_deref()
        .unwrap_or_else(|| std::path::Path::new("/data"));
    let root = tidemark_fs::canon::canonicalize(path)?;
    let configured = context
        .config
        .roots
        .iter()
        .find(|r| r.path == root.as_path());
    let mode = match args.mode.as_deref() {
        Some("walk") => ModeRequest::Walk,
        Some("bulk") => ModeRequest::Bulk,
        Some("auto") => ModeRequest::Auto,
        None => match configured.map(|r| r.mode) {
            Some(tidemark_core::config::RootMode::Walk) => ModeRequest::Walk,
            Some(tidemark_core::config::RootMode::Bulk) => ModeRequest::Bulk,
            _ => ModeRequest::Auto,
        },
        _ => return Err(crate::CliError::Configuration("invalid scan mode".into()).into()),
    };
    let req = ScanRequest {
        root,
        mode,
        throttle: args.throttle.clone(),
        threads: args.threads,
        exclude_extra: args.exclude.clone(),
        keep_partial: args.keep_partial,
        annotations: Vec::new(),
    };
    let cancel = Arc::new(AtomicBool::new(false));
    let registration = signal_hook::flag::register(signal_hook::consts::SIGINT, cancel.clone())?;
    let term = match signal_hook::flag::register(signal_hook::consts::SIGTERM, cancel.clone()) {
        Ok(id) => id,
        Err(e) => {
            signal_hook::low_level::unregister(registration);
            return Err(e.into());
        }
    };
    let display = std::sync::Mutex::new(crate::render::scan_progress::Display::stderr());
    let recorder = DirectRecorder {
        state: context.config.general.state_dir.clone(),
    };
    let result = run_scan(&context.config, &req, &recorder, &cancel, &|p| {
        if let Ok(mut display) = display.lock() {
            let _ = display.update(p);
        }
    });
    signal_hook::low_level::unregister(registration);
    signal_hook::low_level::unregister(term);
    if let Ok(mut display) = display.lock() {
        display.finish()?;
    }
    let outcome = result.map_err(scan_error)?;
    crate::render::scan_progress::outcome(&outcome, context.json, &mut std::io::stdout().lock())?;
    if outcome.degraded || outcome.status == tidemark_core::scan::SnapshotStatus::Partial {
        return Err(crate::CliError::Partial(
            "scan completed with errors or partial results".into(),
        )
        .into());
    }
    Ok(())
}

pub(crate) fn scan_error(e: tidemark_scan::orchestrator::ScanError) -> crate::CliError {
    use tidemark_scan::orchestrator::ScanError;
    match e {
        ScanError::Configuration(message) => crate::CliError::Configuration(message),
        ScanError::Busy
        | ScanError::NoSpace
        | ScanError::Precondition(_)
        | ScanError::Resource(_) => crate::CliError::Precondition(e.to_string()),
        ScanError::Cancelled
        | ScanError::Walk(tidemark_fs::walk::WalkError::Cancelled)
        | ScanError::Bulk(tidemark_scan::bulk::BulkError::Cancelled) => {
            crate::CliError::Partial(e.to_string())
        }
        _ => crate::CliError::Runtime(e.into()),
    }
}
/// Direct catalog recorder used only while the daemon is absent.
pub struct DirectRecorder {
    /// Tidemark's own state directory.
    pub state: std::path::PathBuf,
}
impl tidemark_scan::orchestrator::ScanRecorder for DirectRecorder {
    fn scan_started(
        &self,
        e: &tidemark_store::catalog::ScanEntry,
    ) -> Result<(), tidemark_scan::orchestrator::ScanError> {
        Ok(tidemark_store::catalog::Catalog::open(&self.state, true)?.record_start(e)?)
    }
    fn scan_finished(
        &self,
        e: &tidemark_store::catalog::ScanEntry,
    ) -> Result<(), tidemark_scan::orchestrator::ScanError> {
        Ok(tidemark_store::catalog::Catalog::open(&self.state, true)?.record_finish(e)?)
    }
    fn dir_series(
        &self,
        root: &tidemark_core::path::RawPath,
        id: &str,
        ts: i64,
        rows: &[tidemark_core::snapshot::DirRow],
    ) -> Result<(), tidemark_scan::orchestrator::ScanError> {
        tidemark_scan::orchestrator::record_dir_series(&self.state, root, id, ts, rows)
    }
}
