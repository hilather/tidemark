//! Owned by: W4-B
//! First-start health reports with atomic, durable publication.

use std::{io, path::Path};
/// Cross-task contract reserved for the owning task.
pub use tidemark_core::api::ComponentState;
/// Cross-task contract reserved for the owning task.
pub struct FirstbootInputs {
    #[doc = "Contract field."]
    pub version: String,
    #[doc = "Contract field."]
    pub config_sha256: String,
    #[doc = "Contract field."]
    pub daemon_started_at: i64,
    #[doc = "Contract field."]
    pub components: Vec<ComponentState>,
    #[doc = "Contract field."]
    pub watch_modes: Vec<(String, String)>,
    #[doc = "Contract field."]
    pub first_fs_sample: Option<i64>,
    #[doc = "Contract field."]
    pub first_lvm_sample: Option<i64>,
    #[doc = "Contract field."]
    pub next_scans: Vec<(String, i64)>,
}
/// Cross-task contract reserved for the owning task.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct FirstbootReport {
    #[doc = "Contract field."]
    pub version: String,
    #[doc = "Contract field."]
    pub config_sha256: String,
    #[doc = "Contract field."]
    pub daemon_started_at: i64,
    #[doc = "Contract field."]
    pub components: Vec<ComponentState>,
    #[doc = "Contract field."]
    pub watch_modes: Vec<(String, String)>,
    #[doc = "Contract field."]
    pub first_fs_sample: Option<i64>,
    #[doc = "Contract field."]
    pub first_lvm_sample: Option<i64>,
    #[doc = "Contract field."]
    pub next_scans: Vec<(String, i64)>,
    #[doc = "Contract field."]
    pub booted_at: i64,
    #[doc = "Contract field."]
    pub healthy: bool,
    #[doc = "Contract field."]
    pub written_at: i64,
}
/// Cross-task contract reserved for the owning task.
pub fn needs_report(state_dir: &Path, config_sha256: &str) -> bool {
    if files::read(&state_dir.join("deploy/staged.json")).is_err() {
        return false;
    }
    read_report(&state_dir.join("deploy/firstboot.json"))
        .map_or(true, |report| report.config_sha256 != config_sha256)
}
/// Retry an existing unhealthy report when component facts change, at most every ten seconds.
/// Initial publication remains coordinated by the scheduler's readiness deadline.
#[derive(Default)]
pub(crate) struct Followup {
    next: Option<i64>,
}
impl Followup {
    pub(crate) fn poll(
        &mut self,
        state_dir: &Path,
        inputs: &FirstbootInputs,
        now: i64,
    ) -> io::Result<()> {
        if self.next.is_some_and(|next| now < next) {
            return Ok(());
        }
        self.next = Some(now.saturating_add(10));
        if files::read(&state_dir.join("deploy/staged.json")).is_err() {
            return Ok(());
        }
        let Ok(previous) = read_report(&state_dir.join("deploy/firstboot.json")) else {
            return Ok(());
        };
        if previous.config_sha256 == inputs.config_sha256 && previous.healthy {
            return Ok(());
        }
        if previous.config_sha256 == inputs.config_sha256
            && now < previous.written_at.saturating_add(10)
        {
            return Ok(());
        }
        // Ignore volatile sample/scan timestamps: retries are driven by component changes.
        let same_components = previous.components.len() == inputs.components.len()
            && previous
                .components
                .iter()
                .zip(&inputs.components)
                .all(|(a, b)| a.name == b.name && a.state == b.state && a.detail == b.detail);
        if previous.config_sha256 == inputs.config_sha256 && same_components {
            return Ok(());
        }
        let report = build_with_boot(inputs, now, previous.booted_at);
        write(state_dir, &report)
    }
}
fn read_report(path: &Path) -> io::Result<FirstbootReport> {
    let bytes = files::read(path)?;
    serde_json::from_slice(&bytes).map_err(io::Error::other)
}
/// Build the report using the supplied logical clock and Linux boot timestamp.
pub fn build(inputs: &FirstbootInputs, now: i64) -> FirstbootReport {
    use std::io::Read;
    let booted_at = (|| -> io::Result<i64> {
        let mut text = String::new();
        std::fs::File::open("/proc/stat")?
            .take(16 * 1024 * 1024 + 1)
            .read_to_string(&mut text)?;
        if text.len() > 16 * 1024 * 1024 {
            return Err(io::Error::other("proc stat exceeds 16 MiB"));
        }
        text.lines()
            .find_map(|line| line.strip_prefix("btime ")?.trim().parse().ok())
            .ok_or_else(|| io::Error::other("btime absent or invalid"))
    })()
    .unwrap_or_else(|error| {
        tracing::warn!(%error, "boot timestamp unavailable");
        0
    });
    build_with_boot(inputs, now, booted_at)
}
fn build_with_boot(inputs: &FirstbootInputs, now: i64, booted_at: i64) -> FirstbootReport {
    let healthy = ["sampler", "scheduler", "api", "tswriter"]
        .iter()
        .all(|name| {
            let mut states = inputs.components.iter().filter(|c| c.name == *name);
            states.next().is_some_and(|c| c.state == "ok") && states.next().is_none()
        });
    FirstbootReport {
        version: inputs.version.clone(),
        config_sha256: inputs.config_sha256.clone(),
        daemon_started_at: inputs.daemon_started_at,
        components: inputs.components.clone(),
        watch_modes: inputs.watch_modes.clone(),
        first_fs_sample: inputs.first_fs_sample,
        first_lvm_sample: inputs.first_lvm_sample,
        next_scans: inputs.next_scans.clone(),
        booted_at,
        healthy,
        written_at: now,
    }
}
/// Publish a private report atomically and sync both the file and its parent.
pub fn write(state_dir: &Path, r: &FirstbootReport) -> io::Result<()> {
    files::validate_state(state_dir, std::iter::empty::<&Path>())?;
    let (state, deploy) = files::state_handles(state_dir, false)?;
    files::validate_marker_roots_at(state_dir, state.as_ref())?;
    files::validate_state(&state_dir.join("deploy"), std::iter::empty::<&Path>())?;
    files::optional_at(deploy.as_ref(), "staged.json")?;
    files::optional_at(deploy.as_ref(), "firstboot.json")?;
    let mut bytes = serde_json::to_vec(r).map_err(io::Error::other)?;
    bytes.push(b'\n');
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::other("report exceeds 1 MiB"));
    }
    let (_state, deploy) = files::complete_handles(state_dir, state, deploy)?;
    files::write_at(
        &deploy,
        std::ffi::OsStr::new("firstboot.json"),
        &bytes,
        rustix::process::geteuid().is_root(),
    )?;
    tracing::info!(healthy = r.healthy, version = %r.version, config_sha256 = %r.config_sha256, "Tidemark first-boot report published");
    Ok(())
}
#[path = "../../tidemark-cli/src/deploy/files.rs"]
#[allow(dead_code)]
mod files;

#[cfg(test)]
mod tests {
    use super::*;
    fn private_tempdir() -> tempfile::TempDir {
        use std::os::unix::fs::PermissionsExt;
        let dir = tempfile::tempdir().unwrap();
        std::fs::set_permissions(dir.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        dir
    }
    fn inputs() -> FirstbootInputs {
        FirstbootInputs {
            version: "test".into(),
            config_sha256: "abc".into(),
            daemon_started_at: 10,
            components: ["sampler", "scheduler", "api", "tswriter"]
                .into_iter()
                .map(|name| ComponentState {
                    name: name.into(),
                    state: "ok".into(),
                    detail: String::new(),
                })
                .collect(),
            watch_modes: vec![("/data".into(), "disabled".into())],
            first_fs_sample: Some(11),
            first_lvm_sample: None,
            next_scans: vec![("/data".into(), 100)],
        }
    }
    #[test]
    fn unhealthy_followup_throttles_resumes_and_finalizes() {
        let dir = private_tempdir();
        let mut input = inputs();
        input.components[0].state = "degraded".into();
        write(dir.path(), &build_with_boot(&input, 20, 5)).unwrap();
        std::fs::write(dir.path().join("deploy/staged.json"), b"{}").unwrap();
        assert!(!needs_report(dir.path(), "abc"));
        let mut followup = Followup::default();
        followup.poll(dir.path(), &input, 30).unwrap();
        assert_eq!(
            read_report(&dir.path().join("deploy/firstboot.json"))
                .unwrap()
                .written_at,
            20
        );
        input.components[0].state = "ok".into();
        followup.poll(dir.path(), &input, 39).unwrap();
        assert!(
            !read_report(&dir.path().join("deploy/firstboot.json"))
                .unwrap()
                .healthy
        );
        // A new coordinator resumes a persisted unhealthy report.
        let mut restarted = Followup::default();
        restarted.poll(dir.path(), &input, 40).unwrap();
        let report = read_report(&dir.path().join("deploy/firstboot.json")).unwrap();
        assert!(report.healthy);
        assert_eq!(report.written_at, 40);
        assert_eq!(report.booted_at, 5);
        input.components[0].state = "degraded".into();
        restarted.poll(dir.path(), &input, 50).unwrap();
        assert_eq!(
            read_report(&dir.path().join("deploy/firstboot.json"))
                .unwrap()
                .written_at,
            40
        );
        input.config_sha256 = "changed".into();
        restarted.poll(dir.path(), &input, 60).unwrap();
        assert_eq!(
            read_report(&dir.path().join("deploy/firstboot.json"))
                .unwrap()
                .config_sha256,
            "changed"
        );
    }
    #[test]
    fn unsafe_existing_directory_permissions_refused() {
        use std::os::unix::fs::PermissionsExt;
        for mode in [0o777, 0o750] {
            for child in [false, true] {
                let dir = private_tempdir();
                std::fs::create_dir(dir.path().join("deploy")).unwrap();
                let bad = if child {
                    dir.path().join("deploy")
                } else {
                    dir.path().to_path_buf()
                };
                std::fs::set_permissions(&bad, std::fs::Permissions::from_mode(mode)).unwrap();
                let outside = dir.path().join("outside");
                std::fs::write(&outside, b"outside").unwrap();
                std::fs::write(dir.path().join("deploy/staged.json"), b"original").unwrap();
                assert!(write(dir.path(), &build_with_boot(&inputs(), 20, 5)).is_err());
                assert_eq!(std::fs::read(outside).unwrap(), b"outside");
                assert_eq!(
                    std::fs::read(dir.path().join("deploy/staged.json")).unwrap(),
                    b"original"
                );
                assert!(!dir.path().join("deploy/firstboot.json").exists());
            }
        }
    }
    #[cfg(feature = "privileged")]
    #[test]
    fn foreign_owned_directory_refused() {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref(),
            Ok("1"),
            "disposable VM only"
        );
        for child in [false, true] {
            let dir = private_tempdir();
            std::fs::create_dir(dir.path().join("deploy")).unwrap();
            std::fs::set_permissions(
                dir.path().join("deploy"),
                std::fs::Permissions::from_mode(0o700),
            )
            .unwrap();
            let bad = if child {
                dir.path().join("deploy")
            } else {
                dir.path().to_path_buf()
            };
            let fd = files::directory(&bad, false).unwrap();
            rustix::fs::fchown(&fd, Some(rustix::process::Uid::from_raw(65534)), None).unwrap();
            std::fs::write(dir.path().join("deploy/staged.json"), b"original").unwrap();
            let error = write(dir.path(), &build_with_boot(&inputs(), 20, 5))
                .unwrap_err()
                .to_string();
            assert!(error.contains("65534"));
            assert!(error.contains("0700"));
            assert!(error.contains(&bad.display().to_string()));
            assert!(!dir.path().join("deploy/firstboot.json").exists());
            assert_eq!(
                std::fs::read(dir.path().join("deploy/staged.json")).unwrap(),
                b"original"
            );
        }
    }
    #[test]
    fn core_health_requires_all_and_only_ok() {
        let mut input = inputs();
        input.components.push(ComponentState {
            name: "lvm".into(),
            state: "error".into(),
            detail: "absent".into(),
        });
        let report = build_with_boot(&input, 20, 5);
        assert!(report.healthy);
        assert_eq!(report.booted_at, 5);
        assert_eq!(report.written_at, 20);
        assert_eq!(report.first_fs_sample, Some(11));
        for state in ["disabled", "degraded", "error", "unknown"] {
            input.components[0].state = state.into();
            assert!(!build_with_boot(&input, 20, 5).healthy);
        }
        input.components.remove(0);
        assert!(!build_with_boot(&input, 20, 5).healthy);
        let mut input = inputs();
        input.components.push(input.components[0].clone());
        assert!(!build_with_boot(&input, 20, 5).healthy);
    }
    #[test]
    fn report_lifecycle_and_atomic_permissions() {
        use std::os::unix::fs::PermissionsExt;
        let dir = private_tempdir();
        let path = dir.path();
        assert!(!needs_report(path, "abc"));
        std::fs::create_dir(path.join("deploy")).unwrap();
        std::fs::set_permissions(path.join("deploy"), std::fs::Permissions::from_mode(0o700))
            .unwrap();
        std::fs::write(path.join("deploy/staged.json"), b"{}").unwrap();
        assert!(needs_report(path, "abc"));
        let report = build_with_boot(&inputs(), 20, 5);
        write(path, &report).unwrap();
        assert!(!needs_report(path, "abc"));
        assert!(needs_report(path, "new"));
        assert_eq!(
            std::fs::metadata(path.join("deploy/firstboot.json"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o600
        );
        assert_eq!(std::fs::read_dir(path.join("deploy")).unwrap().count(), 2);
        let mut report = report;
        report.written_at = 30;
        write(path, &report).unwrap();
        assert_eq!(
            read_report(&path.join("deploy/firstboot.json"))
                .unwrap()
                .written_at,
            30
        );
        std::fs::write(path.join("deploy/firstboot.json"), b"broken").unwrap();
        assert!(needs_report(path, "abc"));
    }
    #[test]
    fn failed_write_preserves_report_and_symlink_directory_is_rejected() {
        use std::os::unix::fs::symlink;
        let dir = private_tempdir();
        let report = build_with_boot(&inputs(), 20, 5);
        write(dir.path(), &report).unwrap();
        let original = std::fs::read(dir.path().join("deploy/firstboot.json")).unwrap();
        let mut oversized = report.clone();
        oversized.version = "x".repeat(1024 * 1024);
        assert!(write(dir.path(), &oversized).is_err());
        assert_eq!(
            std::fs::read(dir.path().join("deploy/firstboot.json")).unwrap(),
            original
        );
        assert_eq!(
            std::fs::read_dir(dir.path().join("deploy"))
                .unwrap()
                .count(),
            1
        );
        let linked = dir.path().join("linked");
        symlink(dir.path().join("deploy"), &linked).unwrap();
        assert!(write(&linked, &report).is_err());
    }
    #[test]
    fn maximum_report_is_readable_after_atomic_write() {
        let dir = private_tempdir();
        let mut report = build_with_boot(&inputs(), 20, 5);
        let base_size = serde_json::to_vec(&report).unwrap().len();
        report.version = "x".repeat(1024 * 1024 - 1 - base_size + report.version.len());
        assert_eq!(serde_json::to_vec(&report).unwrap().len(), 1024 * 1024 - 1);
        write(dir.path(), &report).unwrap();
        assert_eq!(
            read_report(&dir.path().join("deploy/firstboot.json"))
                .unwrap()
                .version
                .len(),
            report.version.len()
        );
        std::fs::write(dir.path().join("deploy/staged.json"), b"{}").unwrap();
        assert!(!needs_report(dir.path(), "abc"));
    }
    #[test]
    fn unsafe_state_and_record_symlinks_are_refused() {
        let report = build_with_boot(&inputs(), 20, 5);
        for path in [
            "/",
            "relative",
            "/tmp/../danger",
            "/etc",
            "/usr",
            "/var",
            "/var/lib",
            "/run",
            "/home",
            "/root",
            "/boot",
            "/dev",
            "/proc",
            "/sys",
            "/tmp",
        ] {
            assert!(write(Path::new(path), &report).is_err());
        }
        for name in [
            "deploy",
            ".tidemark-state.json",
            "deploy/staged.json",
            "deploy/firstboot.json",
        ] {
            let dir = private_tempdir();
            let state = dir.path().join("state");
            let outside = dir.path().join("outside");
            std::fs::create_dir_all(state.join("deploy")).unwrap();
            std::fs::create_dir(&outside).unwrap();
            std::fs::write(outside.join("firstboot.json"), b"outside").unwrap();
            let link = state.join(name);
            if link.is_dir() {
                std::fs::remove_dir(&link).unwrap();
            }
            std::os::unix::fs::symlink(
                if name == "deploy" {
                    outside.clone()
                } else {
                    outside.join("firstboot.json")
                },
                link,
            )
            .unwrap();
            assert!(write(&state, &report).is_err());
            assert_eq!(
                std::fs::read(outside.join("firstboot.json")).unwrap(),
                b"outside"
            );
        }
    }
}
