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
    let report: tidemark_core::live::NowReport = mount_call(
        &mut client,
        "now",
        context,
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
pub(crate) fn mount_call<P: serde::Serialize, R: serde::de::DeserializeOwned>(
    client: &mut crate::client::Client,
    method: &str,
    context: &crate::Context,
    params: &P,
) -> anyhow::Result<R> {
    client.call(method, params).map_err(|error| {
        mount_call_error(error, method, context.json, &mut std::io::stdout().lock())
    })
}

fn mount_call_error(
    error: crate::client::ClientError,
    method: &str,
    json: bool,
    out: &mut impl std::io::Write,
) -> anyhow::Error {
    if let crate::client::ClientError::Remote(remote) = &error {
        if remote.code == tidemark_core::api::ApiErrorCode::Unavailable
            && remote.message.contains(" is not watched (watch.mounts = ")
        {
            if json {
                let schema = match method {
                    "hot" => "tidemark.hot.v1",
                    "now" => "tidemark.now.v1",
                    "phantom" => "tidemark.phantom.v1",
                    "writers" => "tidemark.writers.v1",
                    "lvm" => "tidemark.lvm.v1",
                    _ => return call_error(error, method),
                };
                if let Err(write_error) =
                    crate::render::json(schema, serde_json::json!({"error": remote}), out)
                {
                    return write_error;
                }
            }
            return crate::CliError::Precondition(
                remote
                    .message
                    .trim_start_matches("unavailable: ")
                    .to_owned(),
            )
            .into();
        }
    }
    call_error(error, method)
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

#[cfg(test)]
mod mount_filter_tests {
    #[test]
    fn phantom_unwatched_mapping_in_text_and_json() {
        let message = "/B is not watched (watch.mounts = [\"/A\"]); add it to [watch].mounts and run: tidemark deploy stage --from <file>";
        for method in ["phantom", "writers", "lvm", "now", "hot"] {
            for json in [false, true] {
                let remote = tidemark_core::api::ApiError {
                    code: tidemark_core::api::ApiErrorCode::Unavailable,
                    message: format!("unavailable: {message}"),
                };
                let mut output = Vec::new();
                let error = super::mount_call_error(
                    crate::client::ClientError::Remote(remote),
                    method,
                    json,
                    &mut output,
                );
                assert!(
                    matches!(error.downcast_ref::<crate::CliError>(), Some(crate::CliError::Precondition(detail)) if detail == message)
                );
                assert_eq!(crate::command_error(error).exit_code(), 5);
                if json {
                    let doc: serde_json::Value = serde_json::from_slice(&output).unwrap();
                    assert_eq!(doc["schema"], format!("tidemark.{method}.v1"));
                    assert_eq!(doc["data"]["error"]["code"], "unavailable");
                    assert_eq!(
                        doc["data"]["error"]["message"],
                        format!("unavailable: {message}")
                    );
                } else {
                    assert!(output.is_empty());
                }
            }
        }
    }
}
