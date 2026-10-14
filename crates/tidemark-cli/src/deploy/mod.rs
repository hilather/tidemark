//! Owned by: W4-B
//! Staged deployment checks, durable records, verification and guarded rollback.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    io::{self},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tidemark_core::config::Config;

const UNIT: &str = "tidemarkd";
#[cfg(test)]
const MAX_RECORD_BYTES: u64 = 1024 * 1024;
/// State ownership marker, created only after successful staging.
const MARKER: &str = ".tidemark-state.json";
/// Outcome of one pre-flight check.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum CheckStatus {
    /// Prerequisite satisfied.
    Pass,
    /// Optional functionality is unavailable.
    Warn,
    /// Blocking prerequisite failed.
    Fail,
}
/// A stable check name and its explanation.
#[derive(Debug, Clone, Serialize)]
pub struct CheckResult {
    /// Canonical check name.
    pub name: &'static str,
    /// Severity.
    pub status: CheckStatus,
    /// Human-readable details.
    pub detail: String,
}
impl<'de> Deserialize<'de> for CheckResult {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        struct OwnedCheck {
            name: String,
            status: CheckStatus,
            detail: String,
        }
        let value = OwnedCheck::deserialize(deserializer)?;
        let name = [
            "installation",
            "config",
            "roots",
            "mounts",
            "state_space",
            "state_filesystem",
            "fanotify",
            "bulkstat",
            "lvm",
            "previous",
            "service",
            "config_hash",
            "core_components",
            "api",
        ]
        .into_iter()
        .find(|candidate| *candidate == value.name)
        .ok_or_else(|| serde::de::Error::custom("unknown deployment check"))?;
        Ok(Self {
            name,
            status: value.status,
            detail: value.detail,
        })
    }
}
/// Evidence saved when a configuration is staged.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedRecord {
    /// Installed binary version.
    pub version: String,
    /// Injected Unix time.
    pub staged_at: i64,
    /// SHA-256 of the exact installed bytes.
    pub config_sha256: String,
    /// All pre-flight results.
    pub checks: Vec<CheckResult>,
    /// Whether an immediate restart was requested.
    pub start_now: bool,
}
/// Minimal service-manager boundary; tests always supply a fake.
pub trait Systemctl {
    /// Whether the unit is enabled.
    fn is_enabled(&self, unit: &str) -> io::Result<bool>;
    /// Whether the unit is active.
    fn is_active(&self, unit: &str) -> io::Result<bool>;
    /// Whether systemd is currently starting the unit.
    fn is_activating(&self, _unit: &str) -> io::Result<bool> {
        Ok(false)
    }
    /// Enable for subsequent boots.
    fn enable(&self, unit: &str) -> io::Result<()>;
    /// Disable and synchronously stop.
    fn disable_now(&self, unit: &str) -> io::Result<()>;
    /// Start or restart now.
    fn restart(&self, unit: &str) -> io::Result<()>;
}
/// Inputs and filesystem operations used by staging. Implementations may use temporary paths.
pub trait Env {
    /// Installed configuration destination.
    fn config_path(&self) -> &Path;
    /// Logical Unix time.
    fn now(&self) -> i64;
    /// Check that binaries/unit exist and deployment destinations are writable.
    fn installation(&self, cfg: &Config) -> io::Result<()>;
    /// Check directory existence and local filesystem membership.
    fn local_directory(&self, path: &Path) -> io::Result<()>;
    /// Check exact mount-point membership.
    fn mount_point(&self, path: &Path) -> io::Result<()>;
    /// Available and total state filesystem bytes.
    fn capacity(&self, state_dir: &Path) -> io::Result<(u64, u64)>;
    /// Fanotify and namespace availability.
    fn fanotify(&self, cfg: &Config) -> io::Result<()>;
    /// XFS v5 bulkstat availability on /data.
    fn bulkstat(&self, cfg: &Config) -> io::Result<()>;
    /// LVM reporting availability.
    fn lvm(&self, cfg: &Config) -> io::Result<()>;
    /// Read and validate the same source bytes that will be installed.
    fn source(&self, path: &Path) -> Result<(Config, Vec<u8>), DeployError> {
        source_from_read(path, |path| self.read(path))
    }

    /// Read one bounded deployment record or configuration.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        read_bytes(path)
    }
    /// Atomically write a private file and sync its directory.
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        atomic_write(path, bytes)
    }
    /// Publish configuration under its retained, verified parent.
    fn config_write(
        &self,
        parent: &std::os::fd::OwnedFd,
        name: &std::ffi::OsStr,
        bytes: &[u8],
    ) -> io::Result<()> {
        files::write_at(parent, name, bytes, false)
    }
    /// Publish under an already verified state directory handle.
    fn state_write(
        &self,
        parent: &std::os::fd::OwnedFd,
        name: &std::ffi::OsStr,
        bytes: &[u8],
    ) -> io::Result<()> {
        files::write_at(parent, name, bytes, false)
    }
    /// Create a private state directory, rejecting symlink ancestors.
    fn private_dir(&self, path: &Path) -> io::Result<()> {
        private_dir(path)
    }
    /// Remove a deployment-owned file during rollback.
    fn remove(&self, path: &Path) -> io::Result<()> {
        remove_if_exists(path)
    }
}
fn source_from_read(
    path: &Path,
    read: impl FnOnce(&Path) -> io::Result<Vec<u8>>,
) -> Result<(Config, Vec<u8>), DeployError> {
    let bytes = read(path).map_err(|error| {
        DeployError::Preflight(vec![check("config", CheckStatus::Fail, io_detail(&error))])
    })?;
    let cfg = Config::from_toml_bytes(&bytes).map_err(|error| {
        DeployError::Preflight(vec![check("config", CheckStatus::Fail, error.to_string())])
    })?;
    Ok((cfg, bytes))
}
/// Status ping boundary used by verification.
pub trait ApiPing {
    /// Contact the daemon's status method.
    fn ping(&self) -> io::Result<()>;
}
/// Options internal to staging; no frozen CLI flags are added.
#[derive(Debug, Clone, Default)]
pub struct StageOptions {
    /// Start or restart immediately.
    pub start_now: bool,
    /// Only warning-level checks may be skipped.
    pub skip_check: Vec<String>,
}
/// Verification evidence, also the normative JSON output shape.
#[derive(Debug, Clone, Serialize)]
pub struct VerifyResult {
    /// Every required item passed.
    pub healthy: bool,
    /// Required items with explanations.
    pub items: Vec<CheckResult>,
    /// First-start report, if readable.
    pub firstboot: Option<serde_json::Value>,
    /// Staging report, if readable.
    pub staged: Option<serde_json::Value>,
}
/// Failures with stable command-level exit statuses.
#[derive(Debug, thiserror::Error)]
pub enum DeployError {
    /// Compatibility variant retained from preparation.
    #[error("NotYetImplemented")]
    NotYetImplemented,
    /// No writes were made because a blocking check failed.
    #[error("deployment pre-flight failed")]
    Preflight(Vec<CheckResult>),
    /// Unsafe purge or invalid internal option.
    #[error("deployment guard: {0}")]
    Guard(String),
    /// Deployment I/O or service operation failed.
    #[error("{0}")]
    Io(#[from] io::Error),
    /// A record could not be encoded.
    #[error("{0}")]
    Json(#[from] serde_json::Error),
}
fn check(name: &'static str, status: CheckStatus, detail: impl Into<String>) -> CheckResult {
    CheckResult {
        name,
        status,
        detail: detail.into(),
    }
}
fn io_detail(error: &io::Error) -> String {
    if error.kind() == io::ErrorKind::PermissionDenied {
        format!("{error}; {}", crate::ACCESS_HINT)
    } else {
        error.to_string()
    }
}
fn outcome(name: &'static str, warning: bool, result: io::Result<()>) -> CheckResult {
    match result {
        Ok(()) => check(name, CheckStatus::Pass, "ok"),
        Err(e) => check(
            name,
            if warning {
                CheckStatus::Warn
            } else {
                CheckStatus::Fail
            },
            io_detail(&e),
        ),
    }
}
fn all_paths<'a>(
    paths: impl Iterator<Item = &'a PathBuf>,
    mut predicate: impl FnMut(&Path) -> io::Result<()>,
) -> io::Result<()> {
    let errors: Vec<_> = paths
        .filter_map(|path| {
            predicate(path).err().map(|e| {
                format!(
                    "{}: {}",
                    tidemark_core::path::display_lossy_bytes(path.as_os_str().as_encoded_bytes()),
                    io_detail(&e)
                )
            })
        })
        .collect();
    if errors.is_empty() {
        Ok(())
    } else {
        Err(io::Error::other(errors.join("; ")))
    }
}
/// Run every normative pre-flight check without changing the system.
pub fn preflight(cfg: &Config, env: &dyn Env) -> Vec<CheckResult> {
    preflight_with_skips(cfg, env, &[])
}
fn preflight_with_skips(cfg: &Config, env: &dyn Env, skips: &[String]) -> Vec<CheckResult> {
    let issues = cfg.validate();
    let warning = |name, probe: &dyn Fn() -> io::Result<()>| {
        if skips.iter().any(|skip| skip == name) {
            check(name, CheckStatus::Warn, "skipped by operator")
        } else {
            outcome(name, true, probe())
        }
    };
    vec![
        outcome("installation", false, state_guard(&cfg.general.state_dir, Some(cfg)).and_then(|()| files::inspect(env.config_path())).and_then(|()| env.installation(cfg))),
        check("config", if issues.is_empty() { CheckStatus::Pass } else { CheckStatus::Fail }, if issues.is_empty() { "valid".into() } else { issues.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ") }),
        outcome("roots", false, all_paths(cfg.roots.iter().map(|root| &root.path), |path| env.local_directory(path))),
        outcome("state_filesystem", false, all_paths(std::iter::once(&cfg.general.state_dir), |path| real::existing_ancestor(path).and_then(|ancestor| env.local_directory(ancestor)))),
        outcome("mounts", false, all_paths(cfg.sampler.mounts.iter().chain(&cfg.watch.mounts), |path| env.mount_point(path))),
        outcome("state_space", false, env.capacity(&cfg.general.state_dir).and_then(|(available, total)| {
            if total > 0 && available >= 2 * 1024 * 1024 * 1024 && (available as f64 / total as f64 * 100.0) >= cfg.retention.min_free_pct { Ok(()) }
            else { Err(io::Error::other("state filesystem needs at least 2 GiB free and the configured min_free_pct floor")) }
        })),
        warning("fanotify", &|| env.fanotify(cfg)),
        warning("bulkstat", &|| env.bulkstat(cfg)),
        warning("lvm", &|| env.lvm(cfg)),
    ]
}
/// Stage only after all blocking checks pass. Identical installed/staged bytes are a no-op.
pub fn stage(
    cfg_path: &Path,
    opts: &StageOptions,
    sys: &dyn Systemctl,
    env: &dyn Env,
) -> Result<StagedRecord, DeployError> {
    if opts
        .skip_check
        .iter()
        .any(|name| !["fanotify", "bulkstat", "lvm"].contains(&name.as_str()))
    {
        return Err(DeployError::Guard(
            "only fanotify, bulkstat and lvm warnings may be skipped".into(),
        ));
    }
    let (cfg, bytes) = env.source(cfg_path)?;
    state_guard(&cfg.general.state_dir, Some(&cfg))
        .and_then(|()| files::inspect(env.config_path()))
        .map_err(|e| {
            DeployError::Preflight(vec![check(
                "installation",
                CheckStatus::Fail,
                e.to_string(),
            )])
        })?;
    let mut checks = preflight_with_skips(&cfg, env, &opts.skip_check);
    if checks
        .iter()
        .any(|result| result.status == CheckStatus::Fail)
    {
        return Err(DeployError::Preflight(checks));
    }
    let (existing_state, existing_deploy) = files::state_handles(&cfg.general.state_dir, false)?;
    files::optional_at(existing_deploy.as_ref(), "firstboot.json")?;
    let installed = env.config_path();
    let config_parent_path = installed
        .parent()
        .ok_or_else(|| io::Error::other("missing config parent"))?;
    let config_parent = match files::directory(config_parent_path, false) {
        Ok(fd) => Some(fd),
        Err(e) if e.kind() == io::ErrorKind::NotFound => None,
        Err(e) => return Err(e.into()),
    };
    let config_name = installed
        .file_name()
        .ok_or_else(|| io::Error::other("missing config name"))?;
    let old_config = files::optional_name_at(config_parent.as_ref(), config_name)?;
    let old_record = files::optional_at(existing_deploy.as_ref(), "staged.json")?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    let enabled = sys.is_enabled(UNIT)?;
    let active = sys.is_active(UNIT)?;
    if enabled && (!opts.start_now || active) && old_config.as_deref() == Some(bytes.as_slice()) {
        if let Some(mut old) = old_record
            .as_ref()
            .and_then(|data| serde_json::from_slice::<StagedRecord>(data).ok())
            .filter(|old| old.config_sha256 == hash && old.start_now == opts.start_now)
        {
            if let Some(info) = old.checks.iter_mut().find(|item| item.name == "previous") {
                info.detail = "already staged; identical installed config, no changes".into();
            } else {
                old.checks.push(check(
                    "previous",
                    CheckStatus::Pass,
                    "already staged; no changes",
                ));
            }
            return Ok(old);
        }
    }
    checks.push(check(
        "previous",
        CheckStatus::Pass,
        format!("enabled={enabled}, active={active}; restart only with --start-now"),
    ));
    let record = StagedRecord {
        version: env!("CARGO_PKG_VERSION").into(),
        staged_at: env.now(),
        config_sha256: hash,
        checks,
        start_now: opts.start_now,
    };
    let old_marker = files::optional_at(existing_state.as_ref(), MARKER)?;
    let marker = Marker {
        format: "tidemark-state-v1".into(),
        state_dir: tidemark_core::path::RawPath::from_path(&cfg.general.state_dir)
            .map_err(|error| DeployError::Guard(error.to_string()))?,
        config_path: tidemark_core::path::RawPath::from_path(installed)
            .map_err(|error| DeployError::Guard(error.to_string()))?,
    };
    let marker_bytes = serde_json::to_vec(&marker)?;
    let record_bytes = serde_json::to_vec(&record)?;
    let mut backup_name = installed.file_name().unwrap_or_default().to_os_string();
    backup_name.push(format!(".{}.bak", record.staged_at));
    let differing = old_config.as_ref().is_some_and(|old| *old != bytes);
    // Never overwrite an earlier backup when the injected timestamp is reused.
    if differing && files::optional_name_at(config_parent.as_ref(), &backup_name)?.is_some() {
        return Err(DeployError::Guard("config backup already exists".into()));
    }
    let config_parent = match config_parent {
        Some(fd) => fd,
        None => files::directory(config_parent_path, true)?,
    };
    let (state_fd, deploy_fd) =
        files::complete_handles(&cfg.general.state_dir, existing_state, existing_deploy)?;
    let result = (|| -> Result<(), DeployError> {
        if differing {
            if let Some(old) = &old_config {
                env.config_write(&config_parent, &backup_name, old)?;
            }
        }
        env.config_write(&config_parent, config_name, &bytes)?;
        sys.enable(UNIT)?;
        env.state_write(&state_fd, std::ffi::OsStr::new(MARKER), &marker_bytes)?;
        env.state_write(
            &deploy_fd,
            std::ffi::OsStr::new("staged.json"),
            &record_bytes,
        )?;
        // First-boot reporting decides whether to publish from these records.
        if opts.start_now {
            sys.restart(UNIT)?;
        }
        Ok(())
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        if let Err(e) = restore_at(env, &config_parent, config_name, old_config.as_deref()) {
            failures.push(e.to_string());
        }
        for (fd, name, old) in [
            (&deploy_fd, "staged.json", old_record.as_deref()),
            (&state_fd, MARKER, old_marker.as_deref()),
        ] {
            let result = match old {
                Some(bytes) => env.state_write(fd, std::ffi::OsStr::new(name), bytes),
                None => match rustix::fs::unlinkat(fd, name, rustix::fs::AtFlags::empty()) {
                    Ok(()) | Err(rustix::io::Errno::NOENT) => Ok(()),
                    Err(e) => Err(e.into()),
                },
            };
            if let Err(e) = result {
                failures.push(e.to_string());
            }
        }
        if differing {
            if let Err(e) = files::remove_at(&config_parent, &backup_name) {
                failures.push(e.to_string());
            }
        }
        if !enabled || (!active && opts.start_now) {
            if let Err(e) = sys.disable_now(UNIT) {
                failures.push(e.to_string());
            }
        }
        if enabled {
            if let Err(e) = sys.enable(UNIT) {
                failures.push(e.to_string());
            }
        }
        if active && (opts.start_now || !enabled) {
            if let Err(e) = sys.restart(UNIT) {
                failures.push(e.to_string());
            }
        }
        for (label, desired, observed) in [
            ("enabled", enabled, sys.is_enabled(UNIT)),
            ("active", active, sys.is_active(UNIT)),
        ] {
            match observed {
                Ok(value) if value == desired => {}
                Ok(value) => failures.push(format!(
                    "service compensation {label}={value}, expected {desired}"
                )),
                Err(e) => failures.push(e.to_string()),
            }
        }
        return Err(DeployError::Io(io::Error::other(format!(
            "{error}; compensation failures: {}; re-run deploy stage or deploy unstage",
            failures.join("; ")
        ))));
    }
    Ok(record)
}
fn restore_at(
    env: &dyn Env,
    parent: &std::os::fd::OwnedFd,
    name: &std::ffi::OsStr,
    old: Option<&[u8]>,
) -> io::Result<()> {
    match old {
        Some(bytes) => env.config_write(parent, name, bytes),
        None => files::remove_at(parent, name),
    }
}

/// Poll all required verification items every five seconds, up to the requested deadline.
pub fn verify(
    state_dir: &Path,
    wait: Duration,
    sys: &dyn Systemctl,
    api: &dyn ApiPing,
) -> Result<VerifyResult, DeployError> {
    state_guard(state_dir, None)?;
    let started = Instant::now();
    loop {
        let result = verify_once(state_dir, sys, api);
        if result.healthy
            || started.elapsed() >= wait
            || result
                .items
                .iter()
                .any(|item| item.detail == SERVICE_START_HINT)
        {
            return Ok(result);
        }
        std::thread::sleep(Duration::from_secs(5).min(wait.saturating_sub(started.elapsed())));
    }
}
const SERVICE_START_HINT: &str = "the service is enabled but not running: start it with 'systemctl start tidemarkd' (or reboot), or stage with --start-now";
fn verify_once(state_dir: &Path, sys: &dyn Systemctl, api: &dyn ApiPing) -> VerifyResult {
    let staged = read_json(&state_dir.join("deploy/staged.json"));
    let firstboot = read_json(&state_dir.join("deploy/firstboot.json"));
    let mut items = vec![outcome(
        "service",
        false,
        sys.is_active(UNIT).and_then(|active| {
            if active {
                Ok(())
            } else if sys.is_activating(UNIT)? {
                Err(io::Error::other("service activating, waiting for start"))
            } else if sys.is_enabled(UNIT)? {
                Err(io::Error::other(SERVICE_START_HINT))
            } else {
                Err(io::Error::other("service is not enabled or running"))
            }
        }),
    )];
    if items
        .first()
        .is_some_and(|item| item.detail == SERVICE_START_HINT)
    {
        items[0].detail = "staged, waiting for start".into();
        items.push(check(
            "service_start",
            CheckStatus::Warn,
            SERVICE_START_HINT,
        ));
    }
    let expected = staged
        .as_ref()
        .ok()
        .and_then(|value| value.get("config_sha256"))
        .and_then(|v| v.as_str());
    let matches = expected.is_some_and(|hash| {
        firstboot
            .as_ref()
            .ok()
            .and_then(|value| value.get("config_sha256"))
            .and_then(|v| v.as_str())
            == Some(hash)
    });
    items.push(check(
        "config_hash",
        if matches {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        if matches {
            "report matches staged config".into()
        } else {
            format!(
                "missing, invalid or stale report/staging record: staged={}, firstboot={}",
                staged
                    .as_ref()
                    .err()
                    .map(ToString::to_string)
                    .unwrap_or_default(),
                firstboot
                    .as_ref()
                    .err()
                    .map(ToString::to_string)
                    .unwrap_or_default()
            )
        },
    ));
    let components = firstboot
        .as_ref()
        .ok()
        .and_then(|value| value.get("components"))
        .and_then(|v| v.as_array());
    let healthy = firstboot
        .as_ref()
        .ok()
        .and_then(|value| value.get("healthy"))
        .and_then(|v| v.as_bool())
        == Some(true)
        && ["sampler", "scheduler", "api", "tswriter"]
            .iter()
            .all(|name| {
                let mut matching = components.into_iter().flatten().filter(|component| {
                    component.get("name").and_then(|v| v.as_str()) == Some(*name)
                });
                matching.next().is_some_and(|component| {
                    component.get("state").and_then(|v| v.as_str()) == Some("ok")
                }) && matching.next().is_none()
            });
    let core_details = ["sampler", "scheduler", "api", "tswriter"]
        .iter()
        .map(|name| {
            let states: Vec<_> = components
                .into_iter()
                .flatten()
                .filter(|component| component.get("name").and_then(|v| v.as_str()) == Some(*name))
                .map(|component| {
                    component
                        .get("state")
                        .and_then(|v| v.as_str())
                        .unwrap_or("invalid")
                })
                .collect();
            format!(
                "{name}={}",
                if states.is_empty() {
                    "missing".into()
                } else {
                    states.join(",")
                }
            )
        })
        .collect::<Vec<_>>()
        .join("; ");
    items.push(check(
        "core_components",
        if healthy {
            CheckStatus::Pass
        } else {
            CheckStatus::Fail
        },
        core_details,
    ));
    let stopped = items.iter().any(|item| item.detail == SERVICE_START_HINT);
    items.push(outcome(
        "api",
        false,
        if stopped {
            Err(io::Error::other("service is not running"))
        } else {
            api.ping()
        },
    ));
    VerifyResult {
        healthy: items.iter().all(|item| item.status == CheckStatus::Pass),
        items,
        firstboot: firstboot.ok(),
        staged: staged.ok(),
    }
}
fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    files::read(path)
}
fn read_json(path: &Path) -> io::Result<serde_json::Value> {
    serde_json::from_slice(&files::read_state(path)?).map_err(io::Error::other)
}
fn remove_if_exists(path: &Path) -> io::Result<()> {
    files::remove(path)
}
fn private_dir(path: &Path) -> io::Result<()> {
    files::private_dir(path, false)
}
fn reject_symlink_ancestors(path: &Path) -> io::Result<()> {
    match files::directory(path, false) {
        Ok(_) => Ok(()),
        Err(e) if e.kind() == io::ErrorKind::NotFound => {
            files::validate_state(path, std::iter::empty::<&Path>())
        }
        Err(_) => files::inspect(path),
    }
}
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write_owned(path, bytes, false)
}
fn atomic_write_owned(path: &Path, bytes: &[u8], root_owner: bool) -> io::Result<()> {
    files::write(path, bytes, root_owner)
}
mod files;
pub(crate) fn state_guard(path: &Path, cfg: Option<&Config>) -> io::Result<()> {
    files::validate_state(
        path,
        cfg.into_iter().flat_map(|c| {
            c.roots
                .iter()
                .map(|r| &r.path)
                .chain(c.sampler.mounts.iter())
                .chain(c.watch.mounts.iter())
        }),
    )?;
    files::validate_state(&path.join("deploy"), std::iter::empty::<&Path>())?;
    let (state, deploy) = files::state_handles(path, false)?;
    if let Some(bytes) = files::optional_at(state.as_ref(), MARKER)? {
        let marker: Marker = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
        if marker.format != "tidemark-state-v1" || marker.state_dir.as_path() != path {
            return Err(io::Error::other("state marker path mismatch"));
        }
    }
    // Preserve the bounded-record admission contract before verify/service work.
    files::optional_at(deploy.as_ref(), "staged.json")?;
    files::optional_at(deploy.as_ref(), "firstboot.json")?;
    Ok(())
}
#[derive(Serialize, Deserialize)]
struct Marker {
    format: String,
    state_dir: tidemark_core::path::RawPath,
    config_path: tidemark_core::path::RawPath,
}

/// Stop and disable, then remove staging metadata; purge additionally requires an ownership-checked marker.
pub fn unstage(
    state_dir: &Path,
    purge_state: bool,
    sys: &dyn Systemctl,
) -> Result<(), DeployError> {
    state_guard(state_dir, None).map_err(|e| DeployError::Guard(e.to_string()))?;
    let (state, deploy) =
        files::state_handles(state_dir, false).map_err(|e| DeployError::Guard(e.to_string()))?;
    files::validate_marker_roots_at(state_dir, state.as_ref())
        .map_err(|e| DeployError::Guard(e.to_string()))?;
    files::optional_at(deploy.as_ref(), "staged.json")
        .map_err(|e| DeployError::Guard(e.to_string()))?;
    files::optional_at(deploy.as_ref(), "firstboot.json")
        .map_err(|e| DeployError::Guard(e.to_string()))?;
    let guarded = if purge_state {
        Some(purge_guard(state_dir)?)
    } else {
        None
    };

    let previous = if purge_state {
        Some((sys.is_enabled(UNIT)?, sys.is_active(UNIT)?))
    } else {
        None
    };
    let mut removed = false;
    let result = (|| -> Result<(), DeployError> {
        sys.disable_now(UNIT)?;
        if sys.is_active(UNIT)? {
            return Err(DeployError::Guard(
                "service is still active; state retained".into(),
            ));
        }
        if let Some((parent, name, root)) = guarded {
            let busy =
                || DeployError::Guard("state directory in use by another tidemark process".into());
            let _writer =
                tidemark_store::lease::RecoveryLease::acquire(state_dir, Duration::from_secs(10))
                    .map_err(|error| {
                    if error.kind() == io::ErrorKind::WouldBlock {
                        busy()
                    } else {
                        DeployError::Io(error)
                    }
                })?;
            let deadline = Instant::now() + Duration::from_secs(10);
            let _scan = loop {
                match tidemark_scan::orchestrator::ScanLock::acquire(state_dir) {
                    Ok(lock) => break lock,
                    Err(tidemark_scan::orchestrator::ScanError::Busy)
                        if Instant::now() < deadline =>
                    {
                        std::thread::sleep(
                            Duration::from_millis(25)
                                .min(deadline.saturating_duration_since(Instant::now())),
                        );
                    }
                    Err(tidemark_scan::orchestrator::ScanError::Busy) => return Err(busy()),
                    Err(error) => return Err(io::Error::other(error).into()),
                }
            };
            purge_directory(parent, name, root, &mut removed)?;
        } else {
            if let Some(fd) = deploy {
                match rustix::fs::unlinkat(&fd, "staged.json", rustix::fs::AtFlags::empty()) {
                    Ok(()) | Err(rustix::io::Errno::NOENT) => {}
                    Err(e) => return Err(io::Error::from(e).into()),
                }
            }
        }
        Ok(())
    })();
    if let Err(error) = result {
        if let Some((enabled, active)) = previous.filter(|_| !removed) {
            return Err(restore_refused_purge(error, enabled, active, sys));
        }
        return Err(error);
    }
    Ok(())
}

// Purge guards may fail after stopping, but before the first successful unlink.
fn restore_refused_purge(
    error: DeployError,
    enabled: bool,
    active: bool,
    sys: &dyn Systemctl,
) -> DeployError {
    let mut failures = Vec::new();
    let current_enabled = sys.is_enabled(UNIT);
    let current_active = sys.is_active(UNIT);
    let needs_stop = (!enabled && current_enabled.as_ref().is_ok_and(|value| *value))
        || (!active && current_active.as_ref().is_ok_and(|value| *value));
    if needs_stop {
        if let Err(e) = sys.disable_now(UNIT) {
            failures.push(e.to_string());
        }
    }
    if enabled && (needs_stop || !current_enabled.as_ref().is_ok_and(|value| *value)) {
        if let Err(e) = sys.enable(UNIT) {
            failures.push(e.to_string());
        }
    }
    if active && (needs_stop || !current_active.as_ref().is_ok_and(|value| *value)) {
        if let Err(e) = sys.restart(UNIT) {
            failures.push(e.to_string());
        }
    }
    for error in [current_enabled, current_active]
        .into_iter()
        .filter_map(Result::err)
    {
        failures.push(error.to_string());
    }
    for (label, desired, observed) in [
        ("enabled", enabled, sys.is_enabled(UNIT)),
        ("active", active, sys.is_active(UNIT)),
    ] {
        match observed {
            Ok(value) if value == desired => {}
            Ok(value) => failures.push(format!(
                "service compensation {label}={value}, expected {desired}"
            )),
            Err(e) => failures.push(e.to_string()),
        }
    }
    let detail = if failures.is_empty() {
        format!(
            "nothing was removed; service left {} and {} as before",
            if enabled { "enabled" } else { "disabled" },
            if active { "running" } else { "stopped" },
        )
    } else {
        format!("nothing was removed; compensation failures: {}; inspect service state and re-run deploy stage or deploy unstage", failures.join("; "))
    };
    match error {
        DeployError::Guard(message) => DeployError::Guard(format!("{message}; {detail}")),
        other => DeployError::Io(io::Error::other(format!("{other}; {detail}"))),
    }
}

/// Production service-manager implementation. Never used by unprivileged tests.
pub struct RealSystemctl;
impl RealSystemctl {
    fn wait(
        command: &mut std::process::Command,
        timeout: Duration,
    ) -> io::Result<std::process::ExitStatus> {
        let mut child = command.spawn()?;
        let started = Instant::now();
        loop {
            match child.try_wait() {
                Ok(Some(status)) => return Ok(status),
                Ok(None) if started.elapsed() < timeout => std::thread::sleep(
                    Duration::from_millis(20).min(timeout.saturating_sub(started.elapsed())),
                ),
                Ok(None) => {
                    let _ = child.kill();
                    child.wait()?;
                    return Err(io::Error::new(
                        io::ErrorKind::TimedOut,
                        "systemctl operation timed out",
                    ));
                }
                Err(error) => {
                    let _ = child.kill();
                    let _ = child.wait();
                    return Err(error);
                }
            }
        }
    }
    fn status(operation: &str, unit: &str) -> io::Result<bool> {
        let status = Self::wait(
            std::process::Command::new("/usr/bin/systemctl").args([operation, "--quiet", unit]),
            Duration::from_secs(3),
        )?;
        match status.code() {
            Some(0) => Ok(true),
            Some(1) if operation == "is-enabled" => Ok(false),
            Some(3 | 4) if operation == "is-active" => Ok(false),
            _ => Err(io::Error::other(format!(
                "systemctl {operation} failed: {status}"
            ))),
        }
    }
    fn action(args: &[&str]) -> io::Result<()> {
        // Bound start/stop waits while allowing systemd's usual 90-second job timeout.
        let status = Self::wait(
            std::process::Command::new("/usr/bin/systemctl").args(args),
            Duration::from_secs(120),
        )?;
        if status.success() {
            Ok(())
        } else {
            Err(io::Error::other(format!("systemctl failed: {status}")))
        }
    }
}
impl Systemctl for RealSystemctl {
    fn is_enabled(&self, unit: &str) -> io::Result<bool> {
        Self::status("is-enabled", unit)
    }
    fn is_active(&self, unit: &str) -> io::Result<bool> {
        Self::status("is-active", unit)
    }
    fn is_activating(&self, unit: &str) -> io::Result<bool> {
        // Read ActiveState rather than treating systemctl is-active's exit 3
        // (which includes activating) as proof that nobody is starting it.
        let output = std::process::Command::new("/usr/bin/systemctl")
            .args(["show", "--property=ActiveState", "--value", unit])
            .stdout(std::process::Stdio::piped())
            .spawn()?;
        let mut child = output;
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| io::Error::other("missing systemctl output"))?;
        let deadline = Instant::now() + Duration::from_secs(1);
        loop {
            if let Some(status) = child.try_wait()? {
                if !status.success() {
                    return Err(io::Error::other("systemctl show failed"));
                }
                use std::io::Read;
                let mut state = String::new();
                stdout.by_ref().take(128).read_to_string(&mut state)?;
                return Ok(state.trim() == "activating");
            }
            if Instant::now() >= deadline {
                let _ = child.kill();
                let _ = child.wait();
                return Err(io::Error::new(
                    io::ErrorKind::TimedOut,
                    "systemctl show timed out",
                ));
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    fn enable(&self, unit: &str) -> io::Result<()> {
        Self::action(&["enable", unit])
    }
    fn disable_now(&self, unit: &str) -> io::Result<()> {
        Self::action(&["disable", "--now", unit])
    }
    fn restart(&self, unit: &str) -> io::Result<()> {
        Self::action(&["restart", unit])
    }
}

mod purge;
use purge::{purge_directory, purge_guard};
mod real;
pub use real::{RealEnv, SocketPing};
#[cfg(test)]
mod tests;
#[cfg(test)]
use std::fs;
