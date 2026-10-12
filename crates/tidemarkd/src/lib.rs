//! Owned by: W6-A1
//! tidemarkd crate skeleton.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

/// api module skeleton.
pub mod api;
/// firstboot module skeleton.
pub mod firstboot;
/// metrics module skeleton.
pub mod metrics;
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
        let result = tidemark_core::config::Config::load(&args.config)
            .map_err(|error| error.to_string())
            .and_then(|config| {
                run(
                    config,
                    RunOptions {
                        config_path: args.config.clone(),
                        now: 0,
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
    let issues = config.validate();
    for issue in &issues {
        let _ = writeln!(output, "{issue}");
    }
    if issues.is_empty() {
        0
    } else {
        2
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
    run_args(&args, &mut std::io::stderr().lock()).into()
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
/// Start daemon in normative order; preparation performs no storage or service I/O.
/// Shape proposed by W6-prep; the owning task may refine it inside its own file.
pub fn run(config: tidemark_core::config::Config, options: RunOptions) -> Result<(), DaemonError> {
    // 1. Config load/validation; supplied by invocation adapter.
    // 2. Create state dir and validate no-follow handles/permissions (W6-A1).
    // 3. Header preflight, migration; corrupt-only recovery after closing connections (W6-A1).
    // 4. Open healthy/recovered catalog and time-series connections (W6-A1).
    // 5. Namespace check; disable watcher loudly on failure (W6-A1).
    let state = state::LiveState::new(&config, options.now);
    let config = std::sync::Arc::new(config);
    let shutdown = Shutdown::default();
    // 6. Services, scheduler/firstboot and metrics.
    let (writer, _tswriter) =
        services::tswriter::start(config.clone(), state.clone(), shutdown.clone())?;
    let _sampler = services::sampler::start(
        config.clone(),
        state.clone(),
        writer.clone(),
        shutdown.clone(),
    )?;
    let _lvm = services::lvm::start(
        config.clone(),
        state.clone(),
        writer.clone(),
        shutdown.clone(),
    )?;
    let _watcher = services::watcher::start(
        config.clone(),
        state.clone(),
        writer.clone(),
        shutdown.clone(),
    )?;
    let _procscan = services::procscan::start(
        config.clone(),
        state.clone(),
        writer.clone(),
        shutdown.clone(),
    )?;
    let _alerts = services::alerts::start(
        config.clone(),
        state.clone(),
        writer.clone(),
        shutdown.clone(),
    )?;
    let (scans, _scheduler) =
        scheduler::start(config.clone(), state.clone(), writer, shutdown.clone())?;
    let _metrics = metrics::start(config.clone(), state.clone(), shutdown.clone())?;
    // 7. API: stale socket removal, bind and root:root 0600 (W6-A3).
    let context = api::Context {
        state,
        scans,
        reload: std::sync::Arc::new(|| Err(DaemonError::NotYetImplemented)),
    };
    let _api = api::start(config, context, shutdown)?;
    // 8. READY=1 only after successful startup; signals and ordered shutdown (W6-A1).
    Err(DaemonError::NotYetImplemented)
}
