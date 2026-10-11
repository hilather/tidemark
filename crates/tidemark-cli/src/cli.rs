//! Owned by: W0-B2
//! Command surface and shared CLI support.

use crate::cmd;
/// Global command line.
#[derive(Debug, clap::Parser)]
#[command(
    name = "tidemark",
    version,
    about = "Disk-usage intelligence",
    disable_help_subcommand = true
)]
pub struct Cli {
    /// Config.
    #[arg(
        short = 'c',
        long,
        global = true,
        value_name = "FILE",
        default_value = "/etc/tidemark/tidemark.toml"
    )]
    pub config: std::path::PathBuf,
    /// State dir.
    #[arg(long, global = true, value_name = "DIR")]
    pub state_dir: Option<std::path::PathBuf>,
    /// Json.
    #[arg(long, global = true)]
    pub json: bool,
    /// Color.
    #[arg(long, global = true, value_enum, default_value = "auto")]
    pub color: Color,
    /// Verbose.
    #[arg(short = 'v', global = true, action = clap::ArgAction::Count, conflicts_with = "quiet")]
    pub verbose: u8,
    /// Quiet.
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
    /// probe command.
    Probe(cmd::probe::Args),
    /// scan command.
    Scan(cmd::scan::Args),
    /// top command.
    Top(cmd::top::Args),
    /// files command.
    Files(cmd::files::Args),
    /// wide command.
    Wide(cmd::wide::Args),
    /// diff command.
    Diff(cmd::diff::Args),
    /// links command.
    Links(cmd::links::Args),
    /// symlinks command.
    Symlinks(cmd::symlinks::Args),
    /// now command.
    Now(cmd::now::Args),
    /// hot command.
    Hot(cmd::hot::Args),
    /// lvm command.
    Lvm(cmd::lvm::Args),
    /// phantom command.
    Phantom(cmd::phantom::Args),
    /// writers command.
    Writers(cmd::writers::Args),
    /// pg command.
    Pg(cmd::pg::Args),
    /// alerts command.
    Alerts(cmd::alerts::Args),
    /// watch command.
    Watch(cmd::watch::Args),
    /// status command.
    Status(cmd::status::Args),
    /// tui command.
    Tui(cmd::tui::Args),
    /// snapshots command.
    Snapshots(cmd::snapshots::Args),
    /// export command.
    Export(cmd::export::Args),
    /// deploy command.
    Deploy(cmd::deploy::Args),
    /// completions command.
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
            Self::Pg(a) => cmd::pg::run(a, context),
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
