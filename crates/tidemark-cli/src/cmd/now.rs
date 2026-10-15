//! Owned by: W6-A3
//! Command surface and shared CLI support.

use anyhow::Context as _;

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Filter by mount path.
    #[arg(long, value_name = "M")]
    pub mount: Option<std::path::PathBuf>,
}

/// Execute the command through the daemon API.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let mut client = connect(context)?;
    let report: tidemark_core::live::NowReport = call(
        &mut client,
        "now",
        &tidemark_core::api::NowParams {
            mount: mount(args.mount.as_deref())?,
        },
    )?;
    crate::render::now::write(&report, context, &mut std::io::stdout().lock())
}

pub(crate) fn connect(context: &crate::Context) -> anyhow::Result<crate::client::Client> {
    crate::client::Client::connect(
        &context.config.general.socket,
        std::time::Duration::from_secs(11),
    )
    .map_err(|e| {
        let hint = if matches!(&e, crate::client::ClientError::Io(io) if io.kind() == std::io::ErrorKind::PermissionDenied) { format!("\n{}", crate::ACCESS_HINT) } else { String::new() };
        crate::CliError::Unavailable(format!("daemon unavailable — is tidemarkd running? (systemctl status tidemarkd): connect socket {}: {e}{hint}", super::display_path(&context.config.general.socket))).into()
    })
}
pub(crate) fn call<P: serde::Serialize, R: serde::de::DeserializeOwned>(
    client: &mut crate::client::Client,
    method: &str,
    params: &P,
) -> anyhow::Result<R> {
    client
        .call(method, params)
        .map_err(|error| call_error(error, method))
}
pub(crate) fn call_error(e: crate::client::ClientError, method: &str) -> anyhow::Error {
    let error: anyhow::Error = {
        use crate::client::ClientError;
        use tidemark_core::api::ApiErrorCode;
        match e {
            ClientError::Unavailable => crate::CliError::Unavailable(
                "daemon unavailable — is tidemarkd running? (systemctl status tidemarkd)".into(),
            )
            .into(),
            ClientError::Remote(error) => match error.code {
                ApiErrorCode::BadRequest => crate::CliError::Configuration(error.message).into(),
                ApiErrorCode::Forbidden => crate::CliError::Precondition(format!(
                    "{}\n{}",
                    error.message,
                    crate::ACCESS_HINT
                ))
                .into(),
                ApiErrorCode::Unavailable => crate::CliError::Unavailable(error.message).into(),
                ApiErrorCode::Busy => crate::CliError::Precondition(error.message).into(),
                _ => anyhow::anyhow!("daemon error: {}", error.message),
            },
            e => anyhow::Error::new(e),
        }
    };
    error.context(format!("daemon request {method}"))
}
pub(crate) fn mount(path: Option<&std::path::Path>) -> anyhow::Result<Option<String>> {
    path.map(|p| {
        let raw = tidemark_fs::canon::canonicalize(p)
            .with_context(|| format!("resolve mount: {}", super::display_path(p)))?;
        raw.as_path().to_str().map(str::to_owned).ok_or_else(|| {
            crate::CliError::Configuration("API mount labels require UTF-8".into()).into()
        })
    })
    .transpose()
}

pub(crate) struct Interrupt {
    pub flag: std::sync::Arc<std::sync::atomic::AtomicBool>,
    ids: Vec<signal_hook::SigId>,
}
impl Interrupt {
    pub fn new() -> anyhow::Result<Self> {
        let mut guard = Self {
            flag: std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false)),
            ids: Vec::with_capacity(2),
        };
        for signal in [signal_hook::consts::SIGINT, signal_hook::consts::SIGTERM] {
            guard
                .ids
                .push(signal_hook::flag::register(signal, guard.flag.clone())?);
        }
        Ok(guard)
    }
}
impl Drop for Interrupt {
    fn drop(&mut self) {
        for id in self.ids.drain(..) {
            signal_hook::low_level::unregister(id);
        }
    }
}
