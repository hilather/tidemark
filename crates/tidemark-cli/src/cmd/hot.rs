//! Owned by: W6-A3
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Time window for activity.
    #[arg(long, value_parser = ["1m", "5m", "15m", "60m"])]
    pub window: Option<String>,
    /// Group activity by directory, unit or command.
    #[arg(long, value_parser = ["dir", "unit", "comm"])]
    pub by: Option<String>,
    /// Filter by mount path.
    #[arg(long, value_name = "M")]
    pub mount: Option<std::path::PathBuf>,
    /// Maximum number of results.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

/// Execute the command through the daemon API.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    use tidemark_core::api::{HotBy, HotParams, HotWindow};
    let params = HotParams {
        mount: super::now::mount(args.mount.as_deref())?,
        window: match args.window.as_deref().unwrap_or("5m") {
            "1m" => HotWindow::OneMinute,
            "15m" => HotWindow::FifteenMinutes,
            "60m" => HotWindow::SixtyMinutes,
            _ => HotWindow::FiveMinutes,
        },
        by: match args.by.as_deref().unwrap_or("dir") {
            "unit" => HotBy::Unit,
            "comm" => HotBy::Comm,
            _ => HotBy::Dir,
        },
        limit: args.limit.unwrap_or(20),
    };
    let mut client = super::now::connect(context)?;
    let rows: Vec<tidemark_core::live::HotEntry> =
        client.call("hot", &params).map_err(|error| {
            if let crate::client::ClientError::Remote(remote) = &error {
                if remote.code == tidemark_core::api::ApiErrorCode::Unavailable
                    && remote.message.contains(" is not watched (watch.mounts = ")
                {
                    if context.json {
                        if let Err(write_error) = crate::render::json(
                            "tidemark.hot.v1",
                            serde_json::json!({"error": remote}),
                            &mut std::io::stdout().lock(),
                        ) {
                            return write_error;
                        }
                    }
                    return anyhow::Error::from(crate::CliError::Precondition(
                        remote
                            .message
                            .trim_start_matches("unavailable: ")
                            .to_owned(),
                    ));
                }
            }
            super::now::call_error(error, "hot")
        })?;
    crate::render::hot::write_grouped(
        &rows,
        context.json,
        crate::render::table::terminal_width(),
        matches!(params.by, HotBy::Unit),
        &mut std::io::stdout().lock(),
    )
}
