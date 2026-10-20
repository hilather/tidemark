//! Owned by: W0-B2
//! Command surface and shared CLI support.

/// alerts command.
pub mod alerts;
/// completions command.
pub mod completions;
/// deploy command.
pub mod deploy;
/// diff command.
pub mod diff;
/// export command.
pub mod export;
/// files command.
pub mod files;
/// hot command.
pub mod hot;
/// links command.
pub mod links;
/// lvm command.
pub mod lvm;
/// now command.
pub mod now;
/// phantom command.
pub mod phantom;
/// probe command.
pub mod probe;
/// scan command.
pub mod scan;
/// snapshots command.
pub mod snapshots;
/// status command.
pub mod status;
/// symlinks command.
pub mod symlinks;
/// top command.
pub mod top;
/// tui command.
pub mod tui;
/// watch command.
pub mod watch;
/// wide command.
pub mod wide;
/// writers command.
pub mod writers;

// Paths are escaped before entering contextual diagnostics, preserving byte names.
pub(crate) fn display_path(path: &std::path::Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    tidemark_core::path::display_lossy_bytes(path.as_os_str().as_bytes())
}

pub(crate) fn error_context(
    command: &crate::cli::Command,
    config: &tidemark_core::config::Config,
    config_path: &std::path::Path,
) -> String {
    use crate::cli::Command;
    let (name, path) = match command {
        Command::Top(a) => ("top", a.path.as_deref()),
        Command::Files(a) => ("files", a.path.as_deref()),
        Command::Wide(a) => ("wide", a.path.as_deref()),
        Command::Links(a) => ("links", a.path.as_deref()),
        Command::Symlinks(a) => ("symlinks", a.path.as_deref()),
        Command::Diff(a) => ("diff", a.path.as_deref()),
        Command::Tui(a) => ("tui", a.path.as_deref()),
        Command::Scan(a) => ("scan", a.root.as_deref()),
        Command::Watch(a) => ("watch", a.mount.as_deref()),
        Command::Probe(_) => ("probe", Some(config.general.state_dir.as_path())),
        Command::Snapshots(_) => ("snapshots", Some(config.general.state_dir.as_path())),
        Command::Export(_) => ("export", Some(config.general.state_dir.as_path())),
        Command::Deploy(a) => match &a.command {
            deploy::Command::Stage(a) => (
                "deploy stage",
                Some(a.from.as_deref().unwrap_or(config_path)),
            ),
            deploy::Command::Verify(_) => {
                ("deploy verify", Some(config.general.state_dir.as_path()))
            }
            deploy::Command::Unstage(_) => {
                ("deploy unstage", Some(config.general.state_dir.as_path()))
            }
        },
        Command::Now(_) => ("now", Some(config.general.socket.as_path())),
        Command::Hot(_) => ("hot", Some(config.general.socket.as_path())),
        Command::Lvm(_) => ("lvm", Some(config.general.socket.as_path())),
        Command::Phantom(a) => (
            "phantom",
            Some(if a.direct {
                a.mount
                    .as_deref()
                    .unwrap_or_else(|| std::path::Path::new("/proc"))
            } else {
                config.general.socket.as_path()
            }),
        ),
        Command::Writers(a) => (
            "writers",
            Some(if a.direct {
                a.mount
                    .as_deref()
                    .unwrap_or_else(|| std::path::Path::new("/proc"))
            } else {
                config.general.socket.as_path()
            }),
        ),
        Command::Alerts(_) => ("alerts", Some(config.general.socket.as_path())),
        Command::Status(_) => ("status", Some(config.general.socket.as_path())),
        Command::Completions(_) => ("completions", Some(config.general.state_dir.as_path())),
    };
    format!(
        "{name}: {}",
        display_path(path.unwrap_or_else(|| std::path::Path::new("/data")))
    )
}

#[cfg(test)]
mod space_refusal_tests;

#[cfg(test)]
mod context_tests {
    use clap::Parser;
    #[test]
    fn fallback_errors_name_the_direct_subject() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        let mut config = tidemark_core::config::Config::default();
        config.general.socket = temp.path().join("absent.sock");
        config.watch.mounts = vec![missing.clone()];
        let context = crate::Context {
            config,
            config_path: temp.path().join("config"),
            json: false,
            color: false,
        };
        for mount in [None, Some(missing.clone())] {
            let subject =
                super::display_path(mount.as_deref().unwrap_or(std::path::Path::new("/proc")));
            let phantom = super::phantom::run(
                &super::phantom::Args {
                    mount: mount.clone(),
                    direct: false,
                },
                &context,
            )
            .unwrap_err();
            let writers = super::writers::run(
                &super::writers::Args {
                    mount,
                    direct: false,
                    window: None,
                    interval: std::time::Duration::ZERO,
                },
                &context,
            )
            .unwrap_err();
            for (method, error) in [("phantom", phantom), ("writers", writers)] {
                assert!(
                    format!("{error:#}")
                        .starts_with(&format!("direct fallback: {method}: {subject}: ")),
                    "{method}: {error:#}"
                );
            }
        }
    }

    #[test]
    fn daemon_mount_contexts_are_consistent() {
        let config = tidemark_core::config::Config::default();
        for method in ["phantom", "writers", "lvm", "now", "hot"] {
            for mount in [None, Some("/B")] {
                let mut args = vec!["tidemark", method];
                if let Some(mount) = mount {
                    args.extend(["--mount", mount]);
                }
                let cli = crate::cli::Cli::try_parse_from(args).unwrap();
                assert_eq!(
                    super::error_context(&cli.command, &config, std::path::Path::new("/config")),
                    format!("{method}: {}", super::display_path(&config.general.socket))
                );
            }
        }
    }

    #[test]
    fn direct_and_deployment_contexts_name_the_input_subject() {
        let config = tidemark_core::config::Config::default();
        for (args, expected) in [
            (vec!["tidemark", "writers", "--direct"], "writers: /proc"),
            (vec!["tidemark", "phantom", "--direct"], "phantom: /proc"),
            (
                vec!["tidemark", "phantom", "--direct", "--mount", "/B"],
                "phantom: /B",
            ),
            (
                vec!["tidemark", "writers", "--direct", "--mount", "/B"],
                "writers: /B",
            ),
            (
                vec!["tidemark", "deploy", "stage", "--from", "/tmp/source.toml"],
                "deploy stage: /tmp/source.toml",
            ),
            (
                vec!["tidemark", "deploy", "stage"],
                "deploy stage: /tmp/installed.toml",
            ),
        ] {
            let cli = crate::cli::Cli::try_parse_from(args).unwrap();
            assert_eq!(
                super::error_context(
                    &cli.command,
                    &config,
                    std::path::Path::new("/tmp/installed.toml")
                ),
                expected
            );
        }
    }
}
