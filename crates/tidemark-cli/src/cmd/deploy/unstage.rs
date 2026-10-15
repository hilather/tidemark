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
    crate::deploy::state_guard(&context.config.general.state_dir, Some(&context.config))
        .map_err(|e| crate::CliError::Precondition(e.to_string()))?;
    let absent = args.purge_state
        && matches!(std::fs::symlink_metadata(&context.config.general.state_dir), Err(error) if error.kind() == std::io::ErrorKind::NotFound);
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
    if absent {
        absent_message(
            &context.config.general.state_dir,
            &mut std::io::stderr().lock(),
        )?;
    }
    super::render::unstaged(context.json)
}

fn absent_message(path: &std::path::Path, out: &mut impl std::io::Write) -> std::io::Result<()> {
    writeln!(
        out,
        "state directory {} already absent; nothing to purge",
        super::super::display_path(path)
    )
}
#[cfg(test)]
mod tests {
    #[test]
    fn absent_purge_message_names_directory() {
        let mut output = Vec::new();
        super::absent_message(std::path::Path::new("/var/lib/tidemark"), &mut output).unwrap();
        assert_eq!(
            output,
            b"state directory /var/lib/tidemark already absent; nothing to purge\n"
        );
    }
}
