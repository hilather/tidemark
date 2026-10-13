//! Owned by: W0-B2
//! Configuration, output policy and command dispatch.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

use clap::Parser;
use std::io::{IsTerminal, Write};
use tidemark_core::{config::Config, error::CoreError};

/// Complete clap command tree.
pub mod cli;
/// Command implementations and later-owner stubs.
pub mod cmd;
/// Shared human and machine output.
pub mod render;

/// Effective settings provided to command implementations.
#[derive(Debug)]
pub struct Context {
    /// Loaded and validated configuration.
    pub config: Config,
    /// Installed configuration destination from global -c.
    pub config_path: std::path::PathBuf,
    /// Emit schema-wrapped machine output.
    pub json: bool,
    /// Whether terminal color is enabled.
    pub color: bool,
}

/// CLI failures and their stable process exit codes.
#[derive(Debug, thiserror::Error)]
pub enum CliError {
    /// Runtime failure.
    #[error("{0}")]
    Runtime(#[source] anyhow::Error),
    /// Usage or configuration failure.
    #[error("{0}")]
    Configuration(String),
    /// Completed with errors.
    #[error("{0}")]
    Partial(String),
    /// Daemon unavailable.
    #[error("{0}")]
    Unavailable(String),
    /// Required precondition failed.
    #[error("{0}")]
    Precondition(String),
    /// Deployment is not healthy yet.
    #[error("{0}")]
    Unhealthy(String),
}
impl CliError {
    /// Stable exit status from interfaces §1.3.
    pub fn exit_code(&self) -> u8 {
        match self {
            Self::Runtime(_) => 1,
            Self::Configuration(_) => 2,
            Self::Partial(_) => 3,
            Self::Unavailable(_) => 4,
            Self::Precondition(_) => 5,
            Self::Unhealthy(_) => 6,
        }
    }
}
fn command_error(error: anyhow::Error) -> CliError {
    error
        .downcast::<CliError>()
        .unwrap_or_else(CliError::Runtime)
}

/// Root-only access hint for permission errors, without an effective-uid gate.
pub const ACCESS_HINT: &str = "Tidemark's files are root-only; run this command with sudo.";

/// Load a configuration, using built-in defaults only for a missing file.
pub fn load_config(path: &std::path::Path) -> Result<Config, CliError> {
    match Config::load(path) {
        Ok(config) => Ok(config),
        Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut config = Config::default();
            config.merge_default_rules();
            Ok(config)
        }
        Err(CoreError::Io(e)) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            Err(CliError::Configuration(format!("{e}\n{ACCESS_HINT}")))
        }
        Err(e) => Err(CliError::Configuration(e.to_string())),
    }
}
/// Execute a parsed invocation.
pub fn run(cli: cli::Cli) -> Result<(), CliError> {
    // Completions are independent of installed configuration and privileges.
    if let cli::Command::Completions(args) = &cli.command {
        return cmd::completions::run(
            args,
            &Context {
                config: Config::default(),
                config_path: cli.config.clone(),
                json: cli.json,
                color: false,
            },
        )
        .map_err(command_error);
    }
    let staging = matches!(&cli.command, cli::Command::Deploy(args) if matches!(&args.command, cmd::deploy::Command::Stage(_)));
    let mut config = if staging {
        Config::default()
    } else {
        load_config(&cli.config)?
    };
    if let Some(path) = cli.state_dir {
        config.general.state_dir = path;
    }
    let issues = config.validate();
    if !issues.is_empty() {
        return Err(CliError::Configuration(
            issues
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join("\n"),
        ));
    }
    let level = if cli.quiet > 0 {
        "error"
    } else if cli.verbose > 1 {
        "trace"
    } else if cli.verbose > 0 {
        "debug"
    } else {
        &config.general.log_level
    };
    tracing_subscriber::fmt()
        .with_env_filter(level)
        .with_writer(std::io::stderr)
        .with_ansi(false)
        .try_init()
        .ok();
    let color = !cli.json
        && match cli.color {
            cli::Color::Auto => std::io::stdout().is_terminal(),
            cli::Color::Always => true,
            cli::Color::Never => false,
        };
    cli.command
        .run(&Context {
            config,
            config_path: cli.config.clone(),
            json: cli.json,
            color,
        })
        .map_err(command_error)
}
/// Process entry point with clap's help and usage statuses intact.
pub fn main_entry() -> std::process::ExitCode {
    let cli = match cli::Cli::try_parse() {
        Ok(cli) => cli,
        Err(error) => {
            let code = error.exit_code() as u8;
            let _ = error.print();
            return code.into();
        }
    };
    match run(cli) {
        Ok(()) => 0.into(),
        Err(error) => {
            let _ = writeln!(std::io::stderr().lock(), "{error}");
            if let CliError::Runtime(cause) = &error {
                if cause.chain().any(|source| {
                    source
                        .downcast_ref::<std::io::Error>()
                        .is_some_and(|io| io.kind() == std::io::ErrorKind::PermissionDenied)
                }) {
                    let _ = writeln!(std::io::stderr().lock(), "{ACCESS_HINT}");
                }
            }
            error.exit_code().into()
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn configuration_loading_and_overrides() {
        let dir = ::test_support::private_tempdir().unwrap();
        let path = dir.path().join("missing.toml");
        let defaults = load_config(&path).unwrap();
        assert_eq!(
            defaults.general.state_dir,
            std::path::PathBuf::from("/var/lib/tidemark")
        );
        std::fs::write(&path, "[general]\nunknown = true\n").unwrap();
        let error = load_config(&path).unwrap_err();
        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().contains("unknown field `unknown`"));
        std::fs::write(&path, "").unwrap();
        let cli = cli::Cli::try_parse_from([
            "tidemark",
            "--config",
            path.to_str().unwrap(),
            "--state-dir",
            "relative",
            "status",
        ])
        .unwrap();
        let error = run(cli).unwrap_err();
        assert_eq!(error.exit_code(), 2);
        assert!(error.to_string().contains("general.state_dir"));
    }
    #[test]
    fn command_errors_preserve_exit_codes_through_context() {
        for contextual in [false, true] {
            let errors = [
                CliError::Configuration("config".into()),
                CliError::Partial("partial".into()),
                CliError::Unavailable("daemon".into()),
                CliError::Precondition("precondition".into()),
                CliError::Unhealthy("verify".into()),
            ];
            for (error, code) in errors.into_iter().zip(2..=6) {
                let error = anyhow::Error::new(error);
                let error = if contextual {
                    error.context("command context")
                } else {
                    error
                };
                assert_eq!(command_error(error).exit_code(), code);
            }
            let error = anyhow::anyhow!("runtime");
            let error = if contextual {
                error.context("command context")
            } else {
                error
            };
            assert_eq!(command_error(error).exit_code(), 1);
        }
    }
}

/// Synchronous daemon API client.
pub mod client;
/// Staged deployment.
pub mod deploy;

mod live_rows;
mod tui;
