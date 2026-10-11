//! Owned by: W1-A
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Root paths to probe for capabilities.
    #[arg(long, value_name = "PATH", num_args = 1..)]
    pub root: Vec<std::path::PathBuf>,
}

/// Execute the read-only probe.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let filesystem = filesystem_report(args, &context.config);
    crate::render::probe::write(
        &crate::render::probe::Report { filesystem },
        context.json,
        &mut std::io::stdout().lock(),
    )
}

fn filesystem_report(
    args: &Args,
    config: &tidemark_core::config::Config,
) -> tidemark_fs::probe::ProbeReport {
    // Keep original paths until the filesystem collector performs realpath.
    let mut config = config.clone();
    config.roots.extend(
        args.root
            .iter()
            .map(|path| tidemark_core::config::RootConfig {
                path: path.clone(),
                ..Default::default()
            }),
    );
    tidemark_fs::probe::probe(&config, &[])
}
#[cfg(test)]
#[path = "../../tests/w1a/paths.rs"]
mod tests;
