//! Owned by: W6-A3
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Include alert history from the current daemon run only.
    #[arg(long)]
    pub all: bool,
}

/// Execute the command through the daemon API.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let mut client = super::now::connect(context)?;
    if args.all {
        let rows: Vec<tidemark_core::live::AlertEvent> = super::now::call(
            &mut client,
            "alerts.history",
            &tidemark_core::api::AlertsHistoryParams { limit: None },
        )?;
        crate::render::alerts::history(&rows, context.json, &mut std::io::stdout().lock())
    } else {
        let rows: Vec<tidemark_core::live::AlertSummary> = super::now::call(
            &mut client,
            "alerts.active",
            &tidemark_core::api::AlertsActiveParams { limit: None },
        )?;
        crate::render::alerts::active(&rows, context.json, &mut std::io::stdout().lock())
    }
}
