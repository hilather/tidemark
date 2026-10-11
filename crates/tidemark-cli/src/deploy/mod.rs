//! Owned by: W4-B
//! Staged deployment checks, durable records, verification and guarded rollback.

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs,
    io::{self, Read, Write},
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tidemark_core::config::Config;

const UNIT: &str = "tidemarkd";
const MAX_RECORD_BYTES: u64 = 1024 * 1024;
/// State ownership marker, created only after successful staging.
pub const MARKER: &str = ".tidemark-state.json";
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
        let before = read_bytes(path).map_err(|error| {
            DeployError::Preflight(vec![check("config", CheckStatus::Fail, io_detail(&error))])
        })?;
        let cfg = Config::load(path).map_err(|error| {
            DeployError::Preflight(vec![check("config", CheckStatus::Fail, error.to_string())])
        })?;
        if before != read_bytes(path)? {
            return Err(DeployError::Guard("source changed while parsing".into()));
        }
        Ok((cfg, before))
    }
    /// Read one bounded deployment record or configuration.
    fn read(&self, path: &Path) -> io::Result<Vec<u8>> {
        read_bytes(path)
    }
    /// Atomically write a private file and sync its directory.
    fn atomic_write(&self, path: &Path, bytes: &[u8]) -> io::Result<()> {
        atomic_write(path, bytes)
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
        outcome("installation", false, env.installation(cfg)),
        check("config", if issues.is_empty() { CheckStatus::Pass } else { CheckStatus::Fail }, if issues.is_empty() { "valid".into() } else { issues.iter().map(ToString::to_string).collect::<Vec<_>>().join("; ") }),
        outcome("roots", false, all_paths(cfg.roots.iter().map(|root| &root.path), |path| env.local_directory(path))),
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
    let mut checks = preflight_with_skips(&cfg, env, &opts.skip_check);
    if checks
        .iter()
        .any(|result| result.status == CheckStatus::Fail)
    {
        return Err(DeployError::Preflight(checks));
    }
    let installed = env.config_path();
    let old_config = optional_read(env, installed)?;
    let record_path = cfg.general.state_dir.join("deploy/staged.json");
    let old_record = optional_read(env, &record_path)?;
    let hash = format!("{:x}", Sha256::digest(&bytes));
    if old_config.as_deref() == Some(bytes.as_slice()) {
        if let Some(mut old) = old_record
            .as_ref()
            .and_then(|data| serde_json::from_slice::<StagedRecord>(data).ok())
            .filter(|old| old.config_sha256 == hash)
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
    let enabled = sys.is_enabled(UNIT)?;
    let active = sys.is_active(UNIT)?;
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
    let marker_path = cfg.general.state_dir.join(MARKER);
    let old_marker = optional_read(env, &marker_path)?;
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
    let backup = installed.with_file_name(backup_name);
    let differing = old_config.as_ref().is_some_and(|old| *old != bytes);
    // Never overwrite an earlier backup when the injected timestamp is reused.
    if differing && optional_read(env, &backup)?.is_some() {
        return Err(DeployError::Guard("config backup already exists".into()));
    }
    env.private_dir(&cfg.general.state_dir)?;
    env.private_dir(&cfg.general.state_dir.join("deploy"))?;
    let result = (|| -> Result<(), DeployError> {
        if differing {
            if let Some(old) = &old_config {
                env.atomic_write(&backup, old)?;
            }
        }
        env.atomic_write(installed, &bytes)?;
        sys.enable(UNIT)?;
        if opts.start_now {
            sys.restart(UNIT)?;
        }
        env.atomic_write(&marker_path, &marker_bytes)?;
        env.atomic_write(&record_path, &record_bytes)?;
        Ok(())
    })();
    if let Err(error) = result {
        let mut failures = Vec::new();
        for (path, old) in [
            (installed, old_config.as_deref()),
            (&record_path, old_record.as_deref()),
            (&marker_path, old_marker.as_deref()),
        ] {
            if let Err(e) = restore(env, path, old) {
                failures.push(e.to_string());
            }
        }
        if differing {
            if let Err(e) = env.remove(&backup) {
                failures.push(e.to_string());
            }
        }
        let rollback_service = (|| -> io::Result<()> {
            if !enabled {
                sys.disable_now(UNIT)?;
                if active {
                    sys.restart(UNIT)?;
                }
            } else if opts.start_now {
                if active {
                    sys.restart(UNIT)?;
                } else {
                    sys.disable_now(UNIT)?;
                    sys.enable(UNIT)?;
                }
            }
            Ok(())
        })();
        if let Err(e) = rollback_service {
            failures.push(e.to_string());
        }
        if failures.is_empty() {
            return Err(error);
        }
        return Err(DeployError::Io(io::Error::other(format!(
            "{error}; rollback failures: {}",
            failures.join("; ")
        ))));
    }
    Ok(record)
}
fn optional_read(env: &dyn Env, path: &Path) -> io::Result<Option<Vec<u8>>> {
    match env.read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(e),
    }
}
fn restore(env: &dyn Env, path: &Path, old: Option<&[u8]>) -> io::Result<()> {
    match old {
        Some(bytes) => env.atomic_write(path, bytes),
        None => env.remove(path),
    }
}

/// Poll all required verification items every five seconds, up to the requested deadline.
pub fn verify(
    state_dir: &Path,
    wait: Duration,
    sys: &dyn Systemctl,
    api: &dyn ApiPing,
) -> Result<VerifyResult, DeployError> {
    let started = Instant::now();
    loop {
        let result = verify_once(state_dir, sys, api);
        if result.healthy || started.elapsed() >= wait {
            return Ok(result);
        }
        std::thread::sleep(Duration::from_secs(5).min(wait.saturating_sub(started.elapsed())));
    }
}
fn verify_once(state_dir: &Path, sys: &dyn Systemctl, api: &dyn ApiPing) -> VerifyResult {
    let staged = read_json(&state_dir.join("deploy/staged.json"));
    let firstboot = read_json(&state_dir.join("deploy/firstboot.json"));
    let mut items = vec![outcome(
        "service",
        false,
        sys.is_active(UNIT).and_then(|active| {
            if active {
                Ok(())
            } else {
                Err(io::Error::other("staged, waiting for start"))
            }
        }),
    )];
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
    items.push(outcome("api", false, api.ping()));
    VerifyResult {
        healthy: items.iter().all(|item| item.status == CheckStatus::Pass),
        items,
        firstboot: firstboot.ok(),
        staged: staged.ok(),
    }
}
fn read_bytes(path: &Path) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_RECORD_BYTES + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_RECORD_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "deployment file exceeds 1 MiB",
        ));
    }
    Ok(bytes)
}
fn read_json(path: &Path) -> io::Result<serde_json::Value> {
    serde_json::from_slice(&read_bytes(path)?).map_err(io::Error::other)
}
fn remove_if_exists(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        result => result,
    }
}
fn private_dir(path: &Path) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    reject_symlink_ancestors(path)?;
    fs::create_dir_all(path)?;
    fs::set_permissions(path, fs::Permissions::from_mode(0o700))
}
fn reject_symlink_ancestors(path: &Path) -> io::Result<()> {
    for ancestor in path.ancestors() {
        match fs::symlink_metadata(ancestor) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(io::Error::other("deployment path contains a symlink"))
            }
            Ok(_) => {}
            Err(e) if e.kind() == io::ErrorKind::NotFound => {}
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
fn atomic_write(path: &Path, bytes: &[u8]) -> io::Result<()> {
    atomic_write_owned(path, bytes, false)
}
fn atomic_write_owned(path: &Path, bytes: &[u8], root_owner: bool) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    let parent = path
        .parent()
        .ok_or_else(|| io::Error::other("file has no parent"))?;
    reject_symlink_ancestors(parent)?;
    fs::create_dir_all(parent)?;
    let mut temporary = tempfile::NamedTempFile::new_in(parent)?;
    temporary
        .as_file()
        .set_permissions(fs::Permissions::from_mode(0o600))?;
    if root_owner {
        rustix::fs::fchown(
            temporary.as_file(),
            Some(rustix::process::Uid::ROOT),
            Some(rustix::process::Gid::ROOT),
        )?;
    }
    temporary.write_all(bytes)?;
    temporary.as_file().sync_all()?;
    temporary.persist(path).map_err(|e| e.error)?;
    fs::File::open(parent)?.sync_all()
}
#[derive(Serialize, Deserialize)]
struct Marker {
    format: String,
    state_dir: tidemark_core::path::RawPath,
    config_path: tidemark_core::path::RawPath,
}

/// Stop and disable, then remove staging metadata; purge additionally requires an authentic marker.
pub fn unstage(
    state_dir: &Path,
    purge_state: bool,
    sys: &dyn Systemctl,
) -> Result<(), DeployError> {
    let guarded = if purge_state {
        Some(purge_guard(state_dir)?)
    } else {
        None
    };
    sys.disable_now(UNIT)?;
    if sys.is_active(UNIT)? {
        return Err(DeployError::Guard(
            "service is still active; state retained".into(),
        ));
    }
    if let Some((parent, name, root)) = guarded {
        purge_directory(parent, name, root)?;
    } else {
        reject_symlink_ancestors(state_dir)?;
        remove_if_exists(&state_dir.join("deploy/staged.json"))?;
    }
    Ok(())
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
            Duration::from_secs(10),
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
