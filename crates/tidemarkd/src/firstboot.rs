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
    files::validate_marker_roots(state_dir)?;
    files::validate_state(&state_dir.join("deploy"), std::iter::empty::<&Path>())?;
    for name in [
        ".tidemark-state.json",
        "deploy/staged.json",
        "deploy/firstboot.json",
    ] {
        files::inspect(&state_dir.join(name))?;
    }
    let mut bytes = serde_json::to_vec(r).map_err(io::Error::other)?;
    bytes.push(b'\n');
    if bytes.len() > 1024 * 1024 {
        return Err(io::Error::other("report exceeds 1 MiB"));
    }
    files::write(
        &state_dir.join("deploy/firstboot.json"),
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
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path();
        assert!(!needs_report(path, "abc"));
        std::fs::create_dir(path.join("deploy")).unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
        let dir = tempfile::tempdir().unwrap();
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
            let dir = tempfile::tempdir().unwrap();
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
