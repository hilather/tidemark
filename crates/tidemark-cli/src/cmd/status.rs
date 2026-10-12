//! Owned by: W6-A3
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Print effective config including default alert rules.
    #[arg(long)]
    pub show_config: bool,
}

/// Execute the command through the daemon API.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let mut client = super::now::connect(context)?;
    let report: tidemark_core::api::StatusResult =
        super::now::call(&mut client, "status", &tidemark_core::api::StatusParams {})?;
    let scans: Vec<tidemark_core::live::ScanStatus> = super::now::call(
        &mut client,
        "scan.status",
        &tidemark_core::api::ScanStatusParams { root: None },
    )?;
    crate::render::status::write(
        &report,
        &scans,
        context,
        args.show_config,
        &mut std::io::stdout().lock(),
    )
}
