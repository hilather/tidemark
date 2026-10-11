//! Owned by: W4-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Also delete state_dir.
    #[arg(long)]
    pub purge_state: bool,
    /// Confirm unstaging without prompting.
    #[arg(long)]
    pub yes: bool,
}

/// Stop and disable; state deletion requires explicit confirmation and all purge guards.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    if args.purge_state && !args.yes {
        return Err(crate::CliError::Precondition("--purge-state requires --yes".into()).into());
    }
    match crate::deploy::unstage(
        &context.config.general.state_dir,
        args.purge_state,
        &crate::deploy::RealSystemctl,
    ) {
        Ok(()) => {}
        Err(crate::deploy::DeployError::Guard(message)) => {
            return Err(crate::CliError::Precondition(message).into())
        }
        Err(error) => return Err(error.into()),
    }
    super::render::unstaged(context.json)
}
