//! Owned by: W4-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Config file to install; global -c names the installed path.
    #[arg(long, value_name = "FILE")]
    pub from: Option<std::path::PathBuf>,
    /// Start the service immediately after staging.
    #[arg(long)]
    pub start_now: bool,
    /// Skip named warning-level pre-flight checks.
    #[arg(long, value_name = "NAME", num_args = 1.., action = clap::ArgAction::Append)]
    pub skip_check: Vec<String>,
}

/// Stage the supplied configuration after read-only pre-flight checks.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let installed = std::path::PathBuf::from("/etc/tidemark/tidemark.toml");
    let source = args.from.as_deref().unwrap_or(&installed);
    let env = crate::deploy::RealEnv {
        installed_config: installed.clone(),
        now: tidemark_core::time::unix_now(),
    };
    let options = crate::deploy::StageOptions {
        start_now: args.start_now,
        skip_check: args.skip_check.clone(),
    };
    match crate::deploy::stage(source, &options, &crate::deploy::RealSystemctl, &env) {
        Ok(record) => super::render::staged(&record, context.json),
        Err(crate::deploy::DeployError::Preflight(checks)) => {
            super::render::preflight_failure(&checks, context.json)?;
            Err(crate::CliError::Precondition(
                "deployment pre-flight failed; nothing changed".into(),
            )
            .into())
        }
        Err(crate::deploy::DeployError::Guard(message)) => {
            Err(crate::CliError::Precondition(message).into())
        }
        Err(error) => Err(error.into()),
    }
}
