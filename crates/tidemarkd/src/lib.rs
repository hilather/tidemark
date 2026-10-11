//! Owned by: W6-A
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
pub fn run(args: &Args, output: &mut impl Write) -> u8 {
    if !args.check_config {
        let _ = writeln!(output, "not implemented (W6-A)");
        return 1;
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
    run(&args, &mut std::io::stderr().lock()).into()
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
            run(&args, &mut output),
            0,
            "{}",
            String::from_utf8_lossy(&output)
        );
        std::fs::write(&path, "[general]\nunknown_key = true\n").unwrap();
        output.clear();
        assert_eq!(run(&args, &mut output), 2);
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
            run(
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
