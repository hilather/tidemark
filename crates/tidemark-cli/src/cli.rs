//! Owned by: W0-B2
//! Command surface and shared CLI support.

use crate::cmd;
/// Global command line.
#[derive(Debug, clap::Parser)]
#[command(
    name = "tidemark",
    version,
    long_version = if tidemark_store::state_dir::strict_ancestry() { concat!(env!("CARGO_PKG_VERSION"), "\ncommit: ", env!("TIDEMARK_BUILD_COMMIT"), " (", env!("TIDEMARK_BUILD_DATE"), ")\ntarget: ", env!("TIDEMARK_BUILD_TARGET"), ", profile ", env!("TIDEMARK_BUILD_PROFILE"), "\nancestry checks: strict") } else { concat!(env!("CARGO_PKG_VERSION"), "\ncommit: ", env!("TIDEMARK_BUILD_COMMIT"), " (", env!("TIDEMARK_BUILD_DATE"), ")\ntarget: ", env!("TIDEMARK_BUILD_TARGET"), ", profile ", env!("TIDEMARK_BUILD_PROFILE"), "\nancestry checks: development (relaxed)") },
    about = "Disk-usage intelligence",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Config file; use built-in defaults if missing.
    #[arg(
        short = 'c',
        long,
        global = true,
        value_name = "FILE",
        default_value = "/etc/tidemark/tidemark.toml"
    )]
    pub config: std::path::PathBuf,
    /// Override general.state_dir.
    #[arg(long, global = true, value_name = "DIR")]
    pub state_dir: Option<std::path::PathBuf>,
    /// Machine output with a command-specific schema.
    #[arg(long, global = true)]
    pub json: bool,
    /// Terminal colour policy.
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub color: Color,
    /// Increase log verbosity on stderr.
    #[arg(short = 'v', global = true, action = clap::ArgAction::Count, conflicts_with = "quiet")]
    pub verbose: u8,
    /// Decrease log verbosity on stderr.
    #[arg(short = 'q', global = true, action = clap::ArgAction::Count)]
    pub quiet: u8,
    /// Requested command.
    #[command(subcommand)]
    pub command: Command,
}
/// Terminal color policy.
#[derive(Debug, Clone, Copy, clap::ValueEnum)]
pub enum Color {
    /// Detect terminal.
    Auto,
    /// Enable color.
    Always,
    /// Disable color.
    Never,
}
/// All CLI commands.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Capability and environment report.
    Probe(cmd::probe::Args),
    /// Run a scan and write a snapshot.
    Scan(cmd::scan::Args),
    /// Directory drill-down.
    Top(cmd::top::Args),
    /// Largest files.
    Files(cmd::files::Args),
    /// Widest directories.
    Wide(cmd::wide::Args),
    /// Snapshot diff.
    Diff(cmd::diff::Args),
    /// Hard-link topology.
    Links(cmd::links::Args),
    /// Live symlink analysis reads link text and may update link access times according to the mount atime policy (never with noatime).
    Symlinks(cmd::symlinks::Args),
    /// Live situation.
    Now(cmd::now::Args),
    /// Hot directories/writers.
    Hot(cmd::hot::Args),
    /// LV/VG/thin-pool status and trend.
    Lvm(cmd::lvm::Args),
    /// Deleted-but-open files.
    Phantom(cmd::phantom::Args),
    /// Growing open files.
    Writers(cmd::writers::Args),
    /// Active alerts; use --all for history.
    Alerts(cmd::alerts::Args),
    /// Foreground event stream for debugging.
    Watch(cmd::watch::Args),
    /// Daemon health, versions, component states.
    Status(cmd::status::Args),
    /// Interactive browser.
    Tui(cmd::tui::Args),
    /// Snapshot catalog.
    Snapshots(cmd::snapshots::Args),
    /// ncdu JSON export.
    Export(cmd::export::Args),
    /// Pre-flight checks, service deployment and post-reboot verification.
    Deploy(cmd::deploy::Args),
    /// Shell completions.
    Completions(cmd::completions::Args),
}
impl Command {
    /// Dispatch to the command owner.
    pub fn run(&self, context: &crate::Context) -> anyhow::Result<()> {
        match self {
            Self::Probe(a) => cmd::probe::run(a, context),
            Self::Scan(a) => cmd::scan::run(a, context),
            Self::Top(a) => cmd::top::run(a, context),
            Self::Files(a) => cmd::files::run(a, context),
            Self::Wide(a) => cmd::wide::run(a, context),
            Self::Diff(a) => cmd::diff::run(a, context),
            Self::Links(a) => cmd::links::run(a, context),
            Self::Symlinks(a) => cmd::symlinks::run(a, context),
            Self::Now(a) => cmd::now::run(a, context),
            Self::Hot(a) => cmd::hot::run(a, context),
            Self::Lvm(a) => cmd::lvm::run(a, context),
            Self::Phantom(a) => cmd::phantom::run(a, context),
            Self::Writers(a) => cmd::writers::run(a, context),
            Self::Alerts(a) => cmd::alerts::run(a, context),
            Self::Watch(a) => cmd::watch::run(a, context),
            Self::Status(a) => cmd::status::run(a, context),
            Self::Tui(a) => cmd::tui::run(a, context),
            Self::Snapshots(a) => cmd::snapshots::run(a, context),
            Self::Export(a) => cmd::export::run(a, context),
            Self::Deploy(a) => cmd::deploy::run(a, context),
            Self::Completions(a) => cmd::completions::run(a, context),
        }
    }
}

/// Build the same command definition used by the CLI parser.
pub fn command() -> clap::Command {
    <Cli as clap::CommandFactory>::command()
}
