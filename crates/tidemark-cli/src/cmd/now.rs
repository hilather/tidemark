//! Owned by: W6-A3
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Filter by mount path.
    #[arg(long, value_name = "M")]
    pub mount: Option<std::path::PathBuf>,
}

/// Execute the command through the daemon API.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let mount = mount(
        args.mount.as_deref(),
        "now",
        context.json,
        &mut std::io::stdout().lock(),
    )?;
    let mut client = connect(context)?;
    let report: tidemark_core::live::NowReport = mount_call(
        &mut client,
        "now",
        context,
        &tidemark_core::api::NowParams { mount },
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
                let Some(schema) = mount_schema(method) else {
                    return call_error(error, method);
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
fn mount_schema(method: &str) -> Option<&'static str> {
    match method {
        "hot" => Some("tidemark.hot.v1"),
        "now" => Some("tidemark.now.v1"),
        "phantom" => Some("tidemark.phantom.v1"),
        "writers" => Some("tidemark.writers.v1"),
        "lvm" => Some("tidemark.lvm.daemon.v1"),
        _ => None,
    }
}

pub(crate) fn mount(
    path: Option<&std::path::Path>,
    method: &str,
    json: bool,
    out: &mut impl std::io::Write,
) -> anyhow::Result<Option<String>> {
    path.map(|p| {
        let raw = tidemark_fs::canon::canonicalize(p).map_err(|error| {
            let message = format!("resolve mount: {}: {error}", super::display_path(p));
            if error.kind() != std::io::ErrorKind::NotFound {
                return anyhow::Error::new(error)
                    .context(format!("resolve mount: {}", super::display_path(p)));
            }
            if json {
                if let Some(schema) = mount_schema(method) {
                    let error = tidemark_core::api::ApiError {
                        code: tidemark_core::api::ApiErrorCode::NotFound,
                        message: message.clone(),
                    };
                    if let Err(error) =
                        crate::render::json(schema, serde_json::json!({"error": error}), out)
                    {
                        return error;
                    }
                }
            }
            crate::CliError::Precondition(message).into()
        })?;
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
    fn missing_mount_is_a_precondition() {
        let temp = tempfile::tempdir().unwrap();
        let missing = temp.path().join("missing");
        let os_error = tidemark_fs::canon::canonicalize(&missing).unwrap_err();
        let message = format!(
            "resolve mount: {}: {os_error}",
            super::super::display_path(&missing)
        );
        for method in ["phantom", "writers", "lvm", "now", "hot"] {
            for json in [false, true] {
                let mut output = Vec::new();
                let error = super::mount(Some(&missing), method, json, &mut output).unwrap_err();
                assert!(
                    matches!(error.downcast_ref::<crate::CliError>(), Some(crate::CliError::Precondition(detail)) if detail == &message)
                );
                assert_eq!(crate::command_error(error).exit_code(), 5);
                if json {
                    let doc: serde_json::Value = serde_json::from_slice(&output).unwrap();
                    let schema = if method == "lvm" {
                        "tidemark.lvm.daemon.v1".to_owned()
                    } else {
                        format!("tidemark.{method}.v1")
                    };
                    assert_eq!(
                        doc,
                        serde_json::json!({"schema": schema, "data": {"error": {"code": "not_found", "message": message}}})
                    );
                } else {
                    assert!(output.is_empty());
                }
            }
        }
    }

    #[test]
    fn mount_resolution_keeps_existing_optional_and_utf8_behavior() {
        let temp = tempfile::tempdir().unwrap();
        let mut out = Vec::new();
        assert_eq!(super::mount(None, "now", true, &mut out).unwrap(), None);
        assert_eq!(
            super::mount(Some(temp.path()), "now", true, &mut out).unwrap(),
            Some(
                std::fs::canonicalize(temp.path())
                    .unwrap()
                    .to_str()
                    .unwrap()
                    .to_owned()
            )
        );
        use std::os::unix::ffi::OsStringExt;
        let non_utf8 = temp
            .path()
            .join(std::ffi::OsString::from_vec(vec![b'x', 255]));
        std::fs::create_dir(&non_utf8).unwrap();
        let error = super::mount(Some(&non_utf8), "now", true, &mut out).unwrap_err();
        assert_eq!(crate::command_error(error).exit_code(), 2);
        assert!(out.is_empty());
    }

    #[test]
    fn disabled_lvm_remote_error_keeps_unavailable_exit() {
        let error = super::mount_call_error(
            crate::client::ClientError::Remote(tidemark_core::api::ApiError {
                code: tidemark_core::api::ApiErrorCode::Unavailable,
                message: "unavailable: lvm report not configured: disabled by config".into(),
            }),
            "lvm",
            false,
            &mut Vec::new(),
        );
        assert_eq!(
            format!("{error:#}"),
            "daemon request lvm: unavailable: lvm report not configured: disabled by config"
        );
        assert_eq!(crate::command_error(error).exit_code(), 4);
    }

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
                    assert_eq!(
                        doc["schema"],
                        if method == "lvm" {
                            "tidemark.lvm.daemon.v1".to_owned()
                        } else {
                            format!("tidemark.{method}.v1")
                        }
                    );
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
