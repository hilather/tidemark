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
            Some(a.mount.as_deref().unwrap_or(if a.direct {
                std::path::Path::new("/proc")
            } else {
                config.general.socket.as_path()
            })),
        ),
        Command::Writers(a) => (
            "writers",
            Some(a.mount.as_deref().unwrap_or(if a.direct {
                std::path::Path::new("/proc")
            } else {
                config.general.socket.as_path()
            })),
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
mod context_tests {
    use clap::Parser;
    #[test]
    fn direct_and_deployment_contexts_name_the_input_subject() {
        let config = tidemark_core::config::Config::default();
        for (args, expected) in [
            (vec!["tidemark", "writers", "--direct"], "writers: /proc"),
            (vec!["tidemark", "phantom", "--direct"], "phantom: /proc"),
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
