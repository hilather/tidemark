//! Owned by: W4-B
//! Command surface and shared CLI support.

/// Stage command.
pub mod stage;
/// Unstage command.
pub mod unstage;
/// Verify command.
pub mod verify;
/// Deployment arguments.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Operation.
    #[command(subcommand)]
    pub command: Command,
}
/// Deployment operation.
#[derive(Debug, clap::Subcommand)]
pub enum Command {
    /// Pre-flight checks, install config, enable the service for next boot.
    Stage(stage::Args),
    /// Post-reboot pass/fail for the pipeline.
    Verify(verify::Args),
    /// Stop and disable the service; keep state.
    Unstage(unstage::Args),
}
/// Dispatch deployment.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    match &args.command {
        Command::Stage(a) => stage::run(a, context),
        Command::Verify(a) => verify::run(a, context),
        Command::Unstage(a) => unstage::run(a, context),
    }
}

mod render;
