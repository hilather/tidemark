//! Owned by: W7-A
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory path to query (canonicalised before lookup).
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Snapshot ID to query.
    #[arg(long, value_name = "ID")]
    pub snapshot: Option<String>,
}

/// Execute the read-only interactive snapshot browser.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    crate::tui::run(args, context)
}
