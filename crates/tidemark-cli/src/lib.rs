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
    #[error("{0:#}")]
    Runtime(#[source] anyhow::Error),
    /// Usage or configuration failure.
    #[error("{0}")]
    Configuration(String),
    /// Configuration validation report, with separately escaped diagnostic lines.
    #[error("{}", .0.join("\n"))]
    ConfigurationLines(Vec<String>),
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
            Self::Configuration(_) | Self::ConfigurationLines(_) => 2,
            Self::Partial(_) => 3,
            Self::Unavailable(_) => 4,
            Self::Precondition(_) => 5,
            Self::Unhealthy(_) => 6,
        }
    }
}
/// Every returned [`tidemark_store::StoreError::Space`] is a precondition.
///
/// Callers leave the store error in the chain. Plain `top` does not return a
/// free-space floor refusal of the Δ24h snapshot; that soft skip stays exit 0.
/// `MaxStateSize`, `Overflow`, `InvalidPolicy`, an unclassified refusal, and
/// `Floor` on every path that cannot continue all exit 5. I/O stays a store
/// error and keeps its current code (normally runtime exit 1; `snapshots export`
/// still maps a confirmed `ENOSPC`/`EDQUOT` I/O error itself).
fn is_space_refusal(error: &anyhow::Error) -> bool {
    error.chain().any(|source| {
        matches!(
            source.downcast_ref::<tidemark_store::StoreError>(),
            Some(tidemark_store::StoreError::Space { .. })
        )
    })
}
fn command_error(error: anyhow::Error) -> CliError {
    let mut code = error.downcast_ref::<CliError>().map(CliError::exit_code);
    // The command subject (`snapshots: <state_dir>`) names the catalog, which
    // does not help when the failure is the output path itself.
    let unprefixed = error.downcast_ref::<CliError>().and_then(|cli| match cli {
        CliError::Precondition(message) if message.starts_with("output path already exists:") => {
            Some(message.clone())
        }
        CliError::Configuration(message)
            if message.starts_with("output path '-' is not supported") =>
        {
            Some(message.clone())
        }
        _ => None,
    });
    let message = match unprefixed {
        Some(message) => message,
        None => format!("{error:#}"),
    };
    // One mapping for every command that propagates `StoreError::Space`.
    // An explicit `CliError` (export's out-of-space I/O, output collisions)
    // already chose a code and is left alone.
    if code.is_none() && is_space_refusal(&error) {
        code = Some(5);
    }
    match code {
        Some(2) => CliError::Configuration(message),
        Some(3) => CliError::Partial(message),
        Some(4) => CliError::Unavailable(message),
        Some(5) => CliError::Precondition(message),
        Some(6) => CliError::Unhealthy(message),
        _ => CliError::Runtime(error),
    }
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
            Err(CliError::Configuration(format!(
                "load configuration: {}: {e}\n{ACCESS_HINT}",
                cmd::display_path(path)
            )))
        }
        Err(e) => Err(CliError::Configuration(format!(
            "load configuration: {}: {e}",
            cmd::display_path(path)
        ))),
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
        .map_err(|error| command_error(error.context("completions: stdout")));
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
        return Err(CliError::ConfigurationLines(
            issues
                .iter()
                .map(|issue| {
                    format!(
                        "validate configuration: {}: {issue}",
                        cmd::display_path(&cli.config)
                    )
                })
                .collect::<Vec<_>>(),
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
    let operation = cmd::error_context(&cli.command, &config, &cli.config);
    cli.command
        .run(&Context {
            config,
            config_path: cli.config.clone(),
            json: cli.json,
            color,
        })
        .map_err(|error| command_error(error.context(operation)))
}
// Diagnostic messages are single human cells; only the separately appended
// access hint owns a line break. Embedded newlines in paths are escaped.
fn human_error(error: &CliError) -> String {
    if let CliError::ConfigurationLines(lines) = error {
        return lines
            .iter()
            .map(|line| render::table::diagnostic(line))
            .collect::<Vec<_>>()
            .join("\n");
    }
    let message = match error {
        CliError::Runtime(cause) => format!("{cause:#}"),
        _ => error.to_string(),
    };
    let suffix = format!("\n{ACCESS_HINT}");
    if let Some(message) = message.strip_suffix(&suffix) {
        format!("{}\n{ACCESS_HINT}", render::table::diagnostic(message))
    } else {
        render::table::diagnostic(&message)
    }
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
            let _ = writeln!(std::io::stderr().lock(), "{}", human_error(&error));
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
    fn human_diagnostics_escape_path_controls_and_keep_owned_line_breaks() {
        let error = CliError::Runtime(anyhow::anyhow!(
            "deploy: /tmp/admin\u{1b}[31m\nforged: not eligible"
        ));
        assert_eq!(
            human_error(&error),
            "deploy: /tmp/admin\\x1B[31m\\x0Aforged: not eligible"
        );
        let error =
            CliError::Configuration(format!("load config /bad\npath: denied\n{ACCESS_HINT}"));
        assert_eq!(
            human_error(&error),
            format!("load config /bad\\x0Apath: denied\n{ACCESS_HINT}")
        );
        let error = CliError::ConfigurationLines(vec![
            "first: /bad\npath".into(),
            "second: invalid".into(),
        ]);
        assert_eq!(human_error(&error), "first: /bad\\x0Apath\nsecond: invalid");
        assert_eq!(render::table::diagnostic("/already\\xFF"), "/already\\xFF");
    }

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

    #[test]
    fn space_refusals_exit_5_and_io_keeps_runtime() {
        use tidemark_store::space_guard::SpaceRefusal;
        use tidemark_store::StoreError;
        let kinds = [
            Some(SpaceRefusal::Floor {
                bytes: 2 << 30,
                minimum_applied: true,
            }),
            Some(SpaceRefusal::MaxStateSize),
            Some(SpaceRefusal::Overflow),
            Some(SpaceRefusal::InvalidPolicy),
            None,
        ];
        let reasons = [
            "state filesystem free-space floor would be crossed",
            "maximum state size would be exceeded",
            "free-space floor overflow",
            "invalid minimum free percentage",
            "destination disk full",
        ];
        // `Overflow` is not produced by a real filesystem-sized `space_check`
        // (the u64 arithmetic cannot overflow). Every command leaves
        // `StoreError::Space` in the error chain, and this mapping does not
        // branch on the kind or on which command added context.
        for context in [
            "diff: /data",
            "top: /data",
            "snapshots: /var/lib/tidemark",
            "files: /data",
            "wide: /data",
            "links: /data",
            "export: /var/lib/tidemark",
            "tui: /data",
        ] {
            for (kind, reason) in kinds.iter().copied().zip(reasons) {
                let error = anyhow::Error::from(StoreError::space(kind, reason)).context(format!(
                    "open snapshot: {}",
                    cmd::display_path(std::path::Path::new("/var/lib/tidemark/snap.db.zst"))
                ));
                let error = command_error(error.context(context));
                assert_eq!(error.exit_code(), 5, "{context} {reason}");
                let text = error.to_string();
                assert!(
                    text.contains(reason) && text.contains("state space exhausted"),
                    "{context}: {text}"
                );
                assert!(!text.contains("delta_skipped"), "{text}");
            }
        }
        let io = anyhow::Error::from(StoreError::Io(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "missing snapshot",
        )))
        .context("open snapshot: /var/lib/tidemark/snap.db.zst");
        let io = command_error(io.context("snapshots show: /var/lib/tidemark"));
        assert_eq!(io.exit_code(), 1);
        assert!(io.to_string().contains("missing snapshot"), "{io}");
        let sql = anyhow::Error::from(StoreError::Invalid("not a snapshot".into()));
        assert_eq!(command_error(sql).exit_code(), 1);
    }

    #[test]
    fn output_path_errors_omit_the_command_subject() {
        let error = anyhow::Error::new(CliError::Precondition(
            "output path already exists: /tmp/out".into(),
        ))
        .context("export snapshot: /tmp/out")
        .context("snapshots: /var/lib/tidemark");
        let error = command_error(error);
        assert_eq!(error.exit_code(), 5);
        assert_eq!(error.to_string(), "output path already exists: /tmp/out");
    }

    #[test]
    fn dash_output_errors_omit_the_command_subject() {
        let error = anyhow::Error::new(CliError::Configuration(
            "output path '-' is not supported; pass a file path".into(),
        ))
        .context("snapshots: /var/lib/tidemark");
        let error = command_error(error);
        assert_eq!(error.exit_code(), 2);
        assert_eq!(
            error.to_string(),
            "output path '-' is not supported; pass a file path"
        );
    }
}

/// Synchronous daemon API client.
pub mod client;
/// Staged deployment.
pub mod deploy;

mod live_rows;
mod tui;
