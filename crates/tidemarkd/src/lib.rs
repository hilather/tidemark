//! Owned by: W6-A1
//! Daemon startup, signals, reload and bounded shutdown.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// api module skeleton.
pub mod api;
/// firstboot module skeleton.
pub mod firstboot;
/// metrics module skeleton.
pub mod metrics;
/// Configured mount presence and identity.
pub mod mounts;
/// scheduler module skeleton.
pub mod scheduler;
/// Daemon service modules.
pub mod services {
    /// alerts service skeleton.
    pub mod alerts;
    /// lvm service skeleton.
    pub mod lvm;
    /// procscan service skeleton.
    pub mod procscan;
    /// sampler service skeleton.
    pub mod sampler;
    /// tswriter service skeleton.
    pub mod tswriter;
    /// watcher service skeleton.
    pub mod watcher;
}
/// state module skeleton.
pub mod state;

use clap::Parser;
use std::io::Write;

/// Daemon invocation options.
#[derive(Debug, Parser)]
#[command(name = "tidemarkd", version, about = "Tidemark monitoring daemon")]
pub struct Args {
    /// Configuration file.
    #[arg(
        long,
        value_name = "FILE",
        default_value = "/etc/tidemark/tidemark.toml"
    )]
    pub config: std::path::PathBuf,
    /// Validate configuration and exit.
    #[arg(long)]
    pub check_config: bool,
}
/// Validate and report every issue; ordinary startup remains owned by W6-A.
pub fn run_args(args: &Args, output: &mut impl Write) -> u8 {
    if !args.check_config {
        let result = load_config(&args.config)
            .map(|(config, _)| config)
            .map_err(|error| error.to_string())
            .and_then(|config| {
                run(
                    config,
                    RunOptions {
                        config_path: args.config.clone(),
                        now: unix_now(),
                    },
                )
                .map_err(|error| error.to_string())
            });
        if let Err(error) = result {
            let _ = writeln!(output, "{error}");
            return 1;
        }
        return 0;
    }
    let config = match tidemark_core::config::Config::load(&args.config) {
        Ok(config) => config,
        Err(error) => {
            let _ = writeln!(output, "{error}");
            if let tidemark_core::error::CoreError::Io(error) = &error {
                if error.kind() == std::io::ErrorKind::PermissionDenied {
                    let _ = writeln!(
                        output,
                        "Tidemark's files are root-only; run this command with sudo."
                    );
                }
            }
            return 2;
        }
    };
    match validate_config(&config) {
        Ok(()) => 0,
        Err(error) => {
            let _ = writeln!(output, "{error}");
            2
        }
    }
}
/// Thin process entry point.
pub fn main_entry() -> std::process::ExitCode {
    let args = match Args::try_parse() {
        Ok(args) => args,
        Err(error) => {
            let code = error.exit_code() as u8;
            let _ = error.print();
            return code.into();
        }
    };
    if !args.check_config {
        use tracing_subscriber::prelude::*;
        let configured_level = load_config(&args.config)
            .map(|(c, _)| c.general.log_level)
            .unwrap_or_else(|_| "info".into());
        let filter = tracing_subscriber::EnvFilter::try_from_default_env()
            .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new(configured_level));
        if std::env::var_os("INVOCATION_ID").is_some() {
            match tracing_journald::layer() {
                Ok(layer) => {
                    let _ = tracing_subscriber::registry()
                        .with(filter)
                        .with(layer)
                        .try_init();
                }
                Err(_) => {
                    let _ = tracing_subscriber::registry()
                        .with(filter)
                        .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
                        .try_init();
                }
            }
        } else {
            let _ = tracing_subscriber::registry()
                .with(filter)
                .with(tracing_subscriber::fmt::layer().with_writer(std::io::stderr))
                .try_init();
        }
    }
    run_args(&args, &mut std::io::stderr()).into()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn example_and_unknown_key() {
        let doc = include_str!("../../../docs/plan/08-interfaces.md");
        let example = doc
            .split("```toml\n")
            .nth(1)
            .unwrap()
            .split("```")
            .next()
            .unwrap();
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("config.toml");
        std::fs::write(&path, example).unwrap();
        let args = Args {
            config: path.clone(),
            check_config: true,
        };
        let mut output = Vec::new();
        assert_eq!(
            run_args(&args, &mut output),
            0,
            "{}",
            String::from_utf8_lossy(&output)
        );
        std::fs::write(&path, "[general]\nunknown_key = true\n").unwrap();
        output.clear();
        assert_eq!(run_args(&args, &mut output), 2);
        assert!(String::from_utf8_lossy(&output).contains("unknown field `unknown_key`"));
    }
    #[test]
    fn reports_all_validation_issues() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("bad.toml");
        std::fs::write(
            &path,
            "[general]\nstate_dir = 'relative'\nsocket = 'relative'\n",
        )
        .unwrap();
        let mut output = Vec::new();
        assert_eq!(
            run_args(
                &Args {
                    config: path,
                    check_config: true
                },
                &mut output
            ),
            2
        );
        let text = String::from_utf8(output).unwrap();
        assert!(text.contains("general.state_dir"));
        assert!(text.contains("general.socket"));
    }
}

/// Daemon library failures; owners may extend this enum within W6-A1's file.
#[derive(Debug, thiserror::Error)]
pub enum DaemonError {
    /// Preparation stub has no runtime implementation.
    #[error("NotYetImplemented (Wave 6)")]
    NotYetImplemented,
    /// Filesystem or operating-system operation failed.
    #[error("{context}: {source}")]
    Io {
        /// Operation being attempted.
        context: String,
        /// Underlying failure.
        #[source]
        source: std::io::Error,
    },
    /// Configuration could not be loaded or validated.
    #[error("configuration: {0}")]
    Config(String),
    /// Persistent storage failed.
    #[error("storage: {0}")]
    Store(String),
    /// Scan failed.
    #[error("scan: {0}")]
    Scan(String),
    /// Component is disabled or has no measurement.
    #[error("unavailable: {0}")]
    Unavailable(String),
    /// Request or argument is invalid.
    #[error("invalid: {0}")]
    Invalid(String),
    /// Daemon is shutting down.
    #[error("daemon shutting down")]
    Shutdown,
    /// Internal invariant or worker failed.
    #[error("internal: {0}")]
    Internal(String),
}
/// Shared shutdown flag; lifecycle owner broadcasts by setting it true.
pub type Shutdown = std::sync::Arc<std::sync::atomic::AtomicBool>;
/// Service thread joined during bounded shutdown. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub type ServiceHandle = std::thread::JoinHandle<Result<(), DaemonError>>;
/// Startup options with injected clock. Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub struct RunOptions {
    /// Source file for reload.
    pub config_path: std::path::PathBuf,
    /// Unix startup timestamp.
    pub now: i64,
}
/// Start the daemon with OS signals and systemd notification.
pub fn run(config: tidemark_core::config::Config, options: RunOptions) -> Result<(), DaemonError> {
    let state = state::LiveState::new(&config, options.now);
    let shutdown = Shutdown::default();
    let reload = Shutdown::default();
    let ids = [
        signal_hook::flag::register(signal_hook::consts::SIGTERM, shutdown.clone()),
        signal_hook::flag::register(signal_hook::consts::SIGINT, shutdown.clone()),
        signal_hook::flag::register(signal_hook::consts::SIGHUP, reload.clone()),
    ];
    let mut registered = Vec::new();
    for id in ids {
        match id {
            Ok(id) => registered.push(id),
            Err(source) => {
                for id in registered {
                    signal_hook::low_level::unregister(id);
                }
                return Err(DaemonError::Io {
                    context: "register daemon signals".into(),
                    source,
                });
            }
        }
    }
    let result = run_controlled(config, options, state, shutdown, reload);
    for id in registered {
        signal_hook::low_level::unregister(id);
    }
    result
}
/// In-process lifecycle entry point. Caller controls shutdown/reload flags and observes state.
/// No process-global signal handlers are installed. All flags use Acquire/Release ordering.
// Default remains compatible when the scheduler owner replaces its unit stub.
#[allow(clippy::default_constructed_unit_structs)]
pub fn run_controlled(
    mut config: tidemark_core::config::Config,
    options: RunOptions,
    state: state::LiveState,
    shutdown: Shutdown,
    reload_flag: Shutdown,
) -> Result<(), DaemonError> {
    use std::{
        sync::{atomic::Ordering, Arc},
        time::{Duration, Instant},
    };
    validate_config(&config)?;
    let (loaded, hash) = load_config(&options.config_path)?;
    if loaded != config {
        return Err(DaemonError::Config(
            "supplied configuration differs from loaded bytes".into(),
        ));
    }
    // Validate all unsafe paths before creating any artifact; retain the directory handle.
    let fd = prepare_state(&config)?;
    protect_config(&mut config)?;
    state.replace_config(config.clone())?;
    state.bind_storage(fd);
    state.publish(
        state::Publication::Deployment {
            config_sha256: hash,
            started_at: options.now,
        },
        component("daemon", "starting", "initializing"),
    )?;
    // Both storage opens perform header preflight before SQLite. Recovery is corruption-only.
    let storage = state.storage_path();
    let catalog = match tidemark_store::catalog::Catalog::open(&storage, true) {
        Ok(c) => c,
        Err(error) if corrupt(&error) => {
            let c = tidemark_store::catalog::recover_catalog(&storage)
                .map_err(|e| DaemonError::Store(e.to_string()))?;
            for path in c.recovery_files() {
                tracing::warn!(path=%path.display(),"preserved corrupt catalog artifact");
            }
            c
        }
        Err(error) => return Err(DaemonError::Store(error.to_string())),
    };
    // Read catalog history before treating any configured root as measured never-scanned.
    for root in &config.roots {
        let raw = tidemark_core::path::RawPath::from_path(&root.path)
            .map_err(|e| DaemonError::Config(e.to_string()))?;
        let entry = catalog
            .latest(&raw)
            .map_err(|e| DaemonError::Store(e.to_string()))?;
        let last_complete = entry.and_then(|e| {
            Some(tidemark_core::live::LastScan {
                scan_id: e.scan_id,
                finished_at: e.finished_at?,
                mode: format!("{:?}", e.mode).to_lowercase(),
                duration_ms: e.duration_ms.unwrap_or(0),
            })
        });
        state.publish(
            state::Publication::Scan(tidemark_core::live::ScanStatus {
                root: root.path.to_string_lossy().into_owned(),
                running: None,
                last_complete,
                next_due: None,
            }),
            component("catalog", "ok", "scan history read"),
        )?;
    }
    drop(catalog);
    let ts = tidemark_store::timeseries::TimeSeries::open(&storage.join("timeseries.db"), true)
        .map_err(|e| DaemonError::Store(e.to_string()))?;
    drop(ts);
    if config.watch.enabled {
        match tidemark_live::watch::namespace_ok() {
            Ok(true) => {}
            result => {
                let reason =
                    format!("host mount namespace unavailable: {result:?}; watcher disabled");
                tracing::error!(%reason);
                config.watch.enabled = false;
                state.publish(
                    state::Publication::Degraded(reason.clone()),
                    component("watcher", "disabled", reason),
                )?;
                state.replace_config(config.clone())?;
            }
        }
    }
    match tidemark_store::space_guard::space_check(&storage, 0, &config.retention) {
        Ok(verdict) => state.publish(
            state::Publication::StateSpaceRefused(matches!(
                verdict,
                tidemark_store::space_guard::SpaceVerdict::Refuse { .. }
            )),
            component("state_space", "ok", "space guard measured"),
        )?,
        Err(error) => {
            state.publish_component(component("state_space", "error", error.to_string()))?
        }
    }
    let config = Arc::new(config);
    let writer_shutdown = Shutdown::default();
    let (writer, writer_handle) =
        services::tswriter::start(config.clone(), state.clone(), writer_shutdown.clone())?;
    // Any startup error stops already-started services before returning.
    let mut handles = Vec::new();
    let started = (|| -> Result<_, DaemonError> {
        add_service(
            "sampler",
            services::sampler::start(
                config.clone(),
                state.clone(),
                writer.clone(),
                shutdown.clone(),
            ),
            &state,
            &mut handles,
        )?;
        add_service(
            "lvm",
            services::lvm::start(
                config.clone(),
                state.clone(),
                writer.clone(),
                shutdown.clone(),
            ),
            &state,
            &mut handles,
        )?;
        add_service(
            "watcher",
            services::watcher::start(
                config.clone(),
                state.clone(),
                writer.clone(),
                shutdown.clone(),
            ),
            &state,
            &mut handles,
        )?;
        add_service(
            "procscan",
            services::procscan::start(
                config.clone(),
                state.clone(),
                writer.clone(),
                shutdown.clone(),
            ),
            &state,
            &mut handles,
        )?;
        add_service(
            "alerts",
            services::alerts::start(
                config.clone(),
                state.clone(),
                writer.clone(),
                shutdown.clone(),
            ),
            &state,
            &mut handles,
        )?;
        let scans = match scheduler::start(
            config.clone(),
            state.clone(),
            writer.clone(),
            shutdown.clone(),
        ) {
            Ok((scans, h)) => {
                handles.push(("scheduler", h));
                scans
            }
            Err(DaemonError::NotYetImplemented) => {
                state.publish_component(component(
                    "scheduler",
                    "disabled",
                    "implementation pending",
                ))?;
                scheduler::ScanControl::default()
            }
            Err(e) => return Err(e),
        };
        add_service(
            "metrics",
            metrics::start(config.clone(), state.clone(), shutdown.clone()),
            &state,
            &mut handles,
        )?;
        let reload = reload_callback(options.config_path.clone(), state.clone());
        let context = api::Context {
            state: state.clone(),
            scans: scans.clone(),
            reload: reload.clone(),
        };
        add_service(
            "api",
            api::start(config.clone(), context, shutdown.clone()),
            &state,
            &mut handles,
        )?;
        Ok((scans, reload))
    })();
    let mut shutdown_deadline = None;
    let outcome = match started {
        Ok((scans, reload)) => {
            state.publish_component(component("daemon", "ok", "ready"))?;
            let _ = sd_notify::notify(
                false,
                &[
                    sd_notify::NotifyState::Ready,
                    sd_notify::NotifyState::Status("Tidemark ready"),
                ],
            );
            tracing::info!("daemon ready");
            while !shutdown.load(Ordering::Acquire) {
                if reload_flag.swap(false, Ordering::AcqRel) {
                    match reload() {
                        Ok(r) => {
                            tracing::info!(reloaded=r.reloaded,warnings=?r.warnings,"config reload")
                        }
                        Err(e) => tracing::error!(error=%e,"config reload rejected"),
                    }
                }
                for (name, h) in &handles {
                    if h.is_finished() {
                        let _ = state.publish_component(component(
                            name,
                            "error",
                            "service thread exited",
                        ));
                    }
                }
                std::thread::park_timeout(Duration::from_millis(50));
            }
            let deadline = Instant::now() + Duration::from_secs(9);
            shutdown_deadline = Some(deadline);
            scans.begin_shutdown(deadline);
            // API observes shutdown first; cancellation precedes producer joins and writer drain.
            for scan in state
                .view()
                .scans()
                .unwrap_or_default()
                .iter()
                .filter(|s| s.running.is_some())
            {
                if let Ok(root) = tidemark_core::path::RawPath::new(scan.root.as_bytes().to_vec()) {
                    let _ = scans.cancel(&tidemark_core::api::ScanCancelParams { root });
                }
            }
            Ok(())
        }
        Err(e) => Err(e),
    };
    let _ = sd_notify::notify(
        false,
        &[
            sd_notify::NotifyState::Stopping,
            sd_notify::NotifyState::Status("Tidemark stopping"),
        ],
    );
    tracing::info!("daemon stopping");
    shutdown.store(true, Ordering::Release);
    let deadline = shutdown_deadline.unwrap_or_else(|| Instant::now() + Duration::from_secs(9));
    // API first, then scheduler/watchers and the other producers.
    handles.sort_by_key(|(name, _)| match *name {
        "api" => 0,
        "scheduler" => 1,
        "watcher" => 2,
        _ => 3,
    });
    let mut cleanup_error = None;
    for (name, h) in handles {
        if let Err(e) = join_until(name, h, deadline) {
            cleanup_error = Some(e);
        }
    }
    state.clear_watchers();
    let (ack, done) = crossbeam_channel::bounded(1);
    let remaining = deadline.saturating_duration_since(Instant::now());
    if writer
        .send_timeout(services::tswriter::Message::Flush(ack), remaining)
        .is_ok()
    {
        let _ = done.recv_timeout(deadline.saturating_duration_since(Instant::now()));
    }
    drop(writer);
    writer_shutdown.store(true, Ordering::Release);
    if let Err(e) = join_until("tswriter", writer_handle, deadline) {
        cleanup_error = Some(e);
    }
    state.publish_component(component("daemon", "stopped", "shutdown completed"))?;
    outcome.and(cleanup_error.map_or(Ok(()), Err))
}
fn add_service(
    name: &'static str,
    result: Result<ServiceHandle, DaemonError>,
    state: &state::LiveState,
    handles: &mut Vec<(&'static str, ServiceHandle)>,
) -> Result<(), DaemonError> {
    match result {
        Ok(h) => {
            handles.push((name, h));
            Ok(())
        }
        Err(DaemonError::NotYetImplemented) => {
            tracing::warn!(component = name, "service implementation pending");
            state.publish_component(component(name, "disabled", "implementation pending"))
        }
        Err(e) => Err(e),
    }
}
fn join_until(
    name: &str,
    handle: ServiceHandle,
    deadline: std::time::Instant,
) -> Result<(), DaemonError> {
    let mut expired = false;
    while !handle.is_finished() {
        if std::time::Instant::now() >= deadline {
            expired = true;
            break;
        }
        std::thread::park_timeout(std::time::Duration::from_millis(10));
    }
    // Never detach a producer: resources and the writer remain alive until it exits.
    let result = handle
        .join()
        .map_err(|_| DaemonError::Internal(format!("{name} thread panicked")))?;
    if expired {
        return Err(DaemonError::Internal(format!(
            "{name} did not stop before shutdown deadline"
        )));
    }
    result
}
fn corrupt(e: &tidemark_store::StoreError) -> bool {
    // Match the exposed SQLite error code without adding a direct rusqlite dependency.
    matches!(e,tidemark_store::StoreError::Sql(e) if e.sqlite_error_code().is_some_and(|code|matches!(format!("{code:?}").as_str(),"DatabaseCorrupt"|"NotADatabase")))
}
fn validate_config(c: &tidemark_core::config::Config) -> Result<(), DaemonError> {
    let issues = c.validate();
    if !issues.is_empty() {
        return Err(DaemonError::Config(
            issues
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("; "),
        ));
    }
    state::validate_bounds(c)?;
    for (index, root) in c.roots.iter().enumerate() {
        scheduler::parse_schedule(&root.schedule)
            .map_err(|e| DaemonError::Config(format!("roots[{index}].schedule: {e}")))?;
        if let Some(window) = &root.window {
            scheduler::parse_window(window)
                .map_err(|e| DaemonError::Config(format!("roots[{index}].window: {e}")))?;
        }
    }
    tidemark_live::alerts::AlertEngine::new(&c.alerts.rule, c.alerts.renotify)
        .map_err(|e| DaemonError::Config(format!("alerts: {e}")))?;
    Ok(())
}
/// Add state_dir to all collector and scan exclusions before publishing config.
pub fn protect_config(c: &mut tidemark_core::config::Config) -> Result<(), DaemonError> {
    let path = c.general.state_dir.clone();
    for excludes in [
        &mut c.watch.exclude_prefixes,
        &mut c.procscan.exclude_prefixes,
    ] {
        if !excludes.contains(&path) {
            excludes.push(path.clone());
        }
    }
    let text = path
        .to_str()
        .ok_or_else(|| DaemonError::Config("state_dir from TOML must be UTF-8".into()))?;
    // Glob metacharacters are escaped so the directory is always excluded literally.
    let mut literal = String::new();
    for ch in text.chars() {
        if matches!(ch, '*' | '?' | '[' | ']' | '{' | '}' | '\\') {
            literal.push('\\');
        }
        literal.push(ch);
    }
    for root in &mut c.roots {
        if !root.exclude.contains(&literal) {
            root.exclude.push(literal.clone());
        }
    }
    Ok(())
}
/// API/SIGHUP reload callback. Invalid config returns reloaded=false with all issues.
/// Services observe LiveState::config generation; state_dir/socket require restart.
pub fn reload_callback(path: std::path::PathBuf, state: state::LiveState) -> api::Reload {
    std::sync::Arc::new(move || {
        let result = (|| {
            let (mut config, hash) = load_config(&path)?;
            validate_config(&config)?;
            validate_state_paths(&config)?;
            protect_config(&mut config)?;
            scheduler::preflight_reload(&state, &config)?;
            state.replace_config(config)?;
            let started = state
                .view()
                .firstboot_inputs()
                .map_or(0, |i| i.daemon_started_at);
            state.publish(
                state::Publication::Deployment {
                    config_sha256: hash,
                    started_at: started,
                },
                component("config", "ok", "reloaded"),
            )?;
            Ok::<_, DaemonError>(())
        })();
        match result {
            Ok(()) => {
                let _ = sd_notify::notify(
                    false,
                    &[sd_notify::NotifyState::Status("Configuration reloaded")],
                );
                Ok(tidemark_core::api::ConfigReloadResult {
                    reloaded: true,
                    warnings: Vec::new(),
                })
            }
            Err(e) => {
                tracing::error!(error=%e,"keeping previous config");
                let _ = sd_notify::notify(
                    false,
                    &[sd_notify::NotifyState::Status(
                        "Configuration invalid; retaining previous configuration",
                    )],
                );
                state.publish_component(component("config", "error", e.to_string()))?;
                Ok(tidemark_core::api::ConfigReloadResult {
                    reloaded: false,
                    warnings: vec![e.to_string()],
                })
            }
        }
    })
}
#[path = "../../tidemark-cli/src/deploy/files.rs"]
// firstboot::files is private; both inclusions share the CLI-owned policy source.
#[allow(dead_code, clippy::duplicate_mod)]
mod state_files;
fn validate_state_paths(
    c: &tidemark_core::config::Config,
) -> Result<(Option<std::os::fd::OwnedFd>, Option<std::os::fd::OwnedFd>), DaemonError> {
    state_files::validate_state(
        &c.general.state_dir,
        c.roots
            .iter()
            .map(|r| &r.path)
            .chain(c.sampler.mounts.iter())
            .chain(c.watch.mounts.iter()),
    )
    .map_err(|source| DaemonError::Io {
        context: "unsafe state_dir refused".into(),
        source,
    })?;
    if c.general.state_dir.join("deploy").exists() {
        state_files::validate_state(
            &c.general.state_dir.join("deploy"),
            std::iter::empty::<&std::path::Path>(),
        )
        .map_err(|source| DaemonError::Io {
            context: "unsafe deploy directory".into(),
            source,
        })?;
    }
    for name in [
        ".tidemark-state.json",
        "deploy/staged.json",
        "deploy/firstboot.json",
    ] {
        state_files::inspect(&c.general.state_dir.join(name)).map_err(|source| {
            DaemonError::Io {
                context: format!("inspect {name}"),
                source,
            }
        })?;
    }
    for name in ["snapshots", "tmp"] {
        let path = c.general.state_dir.join(name);
        if std::fs::symlink_metadata(&path).is_ok() {
            state_files::validate_state(&path, std::iter::empty::<&std::path::Path>()).map_err(
                |source| DaemonError::Io {
                    context: format!("unsafe {name} directory"),
                    source,
                },
            )?;
        }
    }
    // Check eligibility without creating anything, and retain the checked handles.
    let (state, deploy) =
        state_files::state_handles(&c.general.state_dir, false).map_err(|source| {
            DaemonError::Io {
                context: "open eligible state directories".into(),
                source,
            }
        })?;
    state_files::validate_marker_roots_at(&c.general.state_dir, state.as_ref()).map_err(
        |source| DaemonError::Io {
            context: "validate state marker".into(),
            source,
        },
    )?;
    Ok((state, deploy))
}
fn prepare_state(c: &tidemark_core::config::Config) -> Result<std::os::fd::OwnedFd, DaemonError> {
    use rustix::fs::{self, AtFlags, FileType};
    let (state, deploy) = validate_state_paths(c)?;
    let (fd, _deploy) = state_files::complete_handles(&c.general.state_dir, state, deploy)
        .map_err(|source| DaemonError::Io {
            context: "complete private state directories".into(),
            source,
        })?;
    for name in [
        "timeseries.db",
        "timeseries.db-wal",
        "timeseries.db-shm",
        "timeseries.db.bak-v0",
    ] {
        match fs::statat(&fd, name, AtFlags::SYMLINK_NOFOLLOW) {
            Ok(s) if FileType::from_raw_mode(s.st_mode) != FileType::RegularFile => {
                return Err(DaemonError::Config(format!(
                    "nonregular state artifact: {name}"
                )))
            }
            Ok(_) => {}
            Err(rustix::io::Errno::NOENT) => {}
            Err(e) => {
                return Err(DaemonError::Io {
                    context: format!("inspect {name}"),
                    source: e.into(),
                })
            }
        }
    }
    Ok(fd)
}
fn load_config(
    path: &std::path::Path,
) -> Result<(tidemark_core::config::Config, String), DaemonError> {
    use rustix::fs::{self, FileType, Mode, OFlags};
    use sha2::{Digest, Sha256};
    use std::{
        io::{Read, Seek},
        os::fd::AsRawFd,
    };
    let absolute = if path.is_absolute() {
        path.to_path_buf()
    } else {
        std::env::current_dir()
            .map_err(|source| DaemonError::Io {
                context: "config cwd".into(),
                source,
            })?
            .join(path)
    };
    let parent = state_files::directory(
        absolute
            .parent()
            .ok_or_else(|| DaemonError::Config("config parent missing".into()))?,
        false,
    )
    .map_err(|source| DaemonError::Io {
        context: "open config parent".into(),
        source,
    })?;
    let fd = fs::openat(
        &parent,
        absolute
            .file_name()
            .ok_or_else(|| DaemonError::Config("config name missing".into()))?,
        OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC | OFlags::NONBLOCK,
        Mode::empty(),
    )
    .map_err(|e| DaemonError::Io {
        context: "open config without following links".into(),
        source: e.into(),
    })?;
    if FileType::from_raw_mode(
        fs::fstat(&fd)
            .map_err(|e| DaemonError::Config(e.to_string()))?
            .st_mode,
    ) != FileType::RegularFile
    {
        return Err(DaemonError::Config("config must be a regular file".into()));
    }
    let mut file = std::fs::File::from(fd);
    let mut bytes = Vec::new();
    Read::by_ref(&mut file)
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|e| DaemonError::Config(e.to_string()))?;
    if bytes.len() > 1024 * 1024 {
        return Err(DaemonError::Config("config exceeds 1 MiB".into()));
    }
    let config = tidemark_core::config::Config::load(std::path::Path::new(&format!(
        "/proc/self/fd/{}",
        file.as_raw_fd()
    )))
    .map_err(|e| DaemonError::Config(e.to_string()))?;
    file.rewind()
        .map_err(|e| DaemonError::Config(e.to_string()))?;
    let mut after = Vec::new();
    file.take(1024 * 1024 + 1)
        .read_to_end(&mut after)
        .map_err(|e| DaemonError::Config(e.to_string()))?;
    if bytes != after {
        return Err(DaemonError::Config(
            "config changed during load; retry".into(),
        ));
    }
    Ok((config, format!("{:x}", Sha256::digest(&bytes))))
}

/// Injectable Unix clock for service scheduling and compaction tests.
pub type Clock = std::sync::Arc<dyn Fn() -> i64 + Send + Sync>;
/// Wall clock adapter used only at process/service boundaries.
pub fn unix_now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| i64::try_from(d.as_secs()).unwrap_or(i64::MAX))
        .unwrap_or(0)
}
/// Construct component health using the shared frozen DTO.
pub fn component(
    name: &str,
    state: &str,
    detail: impl Into<String>,
) -> tidemark_core::api::ComponentState {
    tidemark_core::api::ComponentState {
        name: name.into(),
        state: state.into(),
        detail: detail.into(),
    }
}
/// Wait interruptibly for a service interval; reload generation changes wake services.
pub fn wait_service(
    state: &state::LiveState,
    generation: u64,
    shutdown: &Shutdown,
    interval: std::time::Duration,
) {
    let start = std::time::Instant::now();
    while !shutdown.load(std::sync::atomic::Ordering::Acquire) && state.config().1 == generation {
        let remaining = interval.saturating_sub(start.elapsed());
        if remaining.is_zero() {
            break;
        }
        std::thread::park_timeout(remaining.min(std::time::Duration::from_millis(50)));
    }
}
