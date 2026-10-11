//! Owned by: W4-A
//! Read-only bounded LVM reporting, parsing and device-number mapping.

use crate::mount::MountInfo;
/// Reporting-command or report validation failure.
#[derive(Debug, thiserror::Error)]
pub enum LvmError {
    /// Compatibility variant; real operations no longer return this.
    #[error("NotYetImplemented")]
    NotYetImplemented,
    /// LVM reporting tools are absent.
    #[error("n/a (LVM reporting tool not installed)")]
    NotInstalled,
    /// The child exceeded its deadline and was killed and reaped.
    #[error("LVM reporting command timed out")]
    Timeout,
    /// LVM collection is disabled by configuration.
    #[error("n/a (LVM disabled)")]
    Disabled,
    /// Invalid or oversized report.
    #[error("invalid LVM report: {0}")]
    Invalid(String),
    /// Failed reporting command.
    #[error("LVM reporting command failed: {0}")]
    Command(String),
    /// Process or pipe I/O failed.
    #[error(transparent)]
    Io(#[from] std::io::Error),
}
/// Shared core VG information.
pub use tidemark_core::api::VgInfo;
use tidemark_core::{
    config::LvmConfig,
    live::{LvmNow, LvmSample},
    path::RawPath,
};
/// LVM metadata from reporting commands.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LvInfo {
    /// Volume group name as reported.
    pub vg: String,
    /// Raw logical volume name, including brackets used for hidden volumes.
    pub lv: String,
    /// LVM attribute string.
    pub attr: String,
    /// Segment type, for example linear, thin, or thin-pool.
    pub segtype: String,
    /// Logical size in bytes.
    pub size: u64,
    /// Data usage percentage when applicable.
    pub data_pct: Option<f64>,
    /// Metadata usage percentage when applicable.
    pub meta_pct: Option<f64>,
    /// Raw thin-pool name when applicable.
    pub pool_lv: Option<String>,
    /// Reported device-mapper path; never used for matching.
    pub dm_path: Option<String>,
    /// Active kernel device major number.
    pub major: Option<u32>,
    /// Active kernel device minor number.
    pub minor: Option<u32>,
}

/// LVM metadata from reporting commands.
#[derive(Debug, Clone, serde::Serialize)]
pub struct LvmReport {
    /// Logical volumes sorted by VG and raw LV name.
    pub lvs: Vec<LvInfo>,
    /// Volume groups sorted by name.
    pub vgs: Vec<VgInfo>,
    /// Injected Unix sampling timestamp in seconds.
    pub sampled_at: i64,
}
/// LVM metadata from reporting commands.
#[derive(Debug, Clone, serde::Serialize)]
pub struct MountLv {
    /// Mounted filesystem path as raw bytes.
    pub mount: RawPath,
    /// Volume group name as reported.
    pub vg: String,
    /// Raw logical volume name, including brackets used for hidden volumes.
    pub lv: String,
    /// Raw pool name for a thin logical volume.
    pub thin_pool: Option<String>,
}

// Reports and child output are capped at 16 MiB; at most 100,000 rows.
const MAX_BYTES: usize = 16 * 1024 * 1024;
const MAX_ROWS: usize = 100_000;

fn rows(
    bytes: &[u8],
    section: &str,
) -> Result<Vec<serde_json::Map<String, serde_json::Value>>, LvmError> {
    if bytes.len() > MAX_BYTES {
        return Err(LvmError::Invalid("report exceeds 16 MiB".into()));
    }
    let value: serde_json::Value =
        serde_json::from_slice(bytes).map_err(|e| LvmError::Invalid(e.to_string()))?;
    let reports = value
        .get("report")
        .and_then(|v| v.as_array())
        .ok_or_else(|| LvmError::Invalid("missing report array".into()))?;
    let mut out = Vec::new();
    for report in reports {
        let entries = report
            .get(section)
            .and_then(|v| v.as_array())
            .ok_or_else(|| LvmError::Invalid(format!("missing {section} array")))?;
        for entry in entries {
            if out.len() >= MAX_ROWS {
                return Err(LvmError::Invalid("too many rows".into()));
            }
            out.push(
                entry
                    .as_object()
                    .ok_or_else(|| LvmError::Invalid("row is not an object".into()))?
                    .clone(),
            );
        }
    }
    Ok(out)
}
fn field<'a>(
    row: &'a serde_json::Map<String, serde_json::Value>,
    key: &str,
) -> Result<&'a str, LvmError> {
    row.get(key)
        .and_then(|v| v.as_str())
        .map(str::trim)
        .ok_or_else(|| LvmError::Invalid(format!("missing/string field {key}")))
}
fn number<T: std::str::FromStr>(value: &str) -> Result<T, LvmError> {
    value
        .parse()
        .map_err(|_| LvmError::Invalid(format!("invalid number {value:?}")))
}
fn optional(value: &str) -> Option<String> {
    (!value.is_empty()).then(|| value.to_owned())
}
fn percent(value: &str) -> Result<Option<f64>, LvmError> {
    if value.is_empty() {
        return Ok(None);
    }
    let n: f64 = number(value)?;
    if !n.is_finite() || !(0.0..=100.0).contains(&n) {
        return Err(LvmError::Invalid("percentage outside 0..100".into()));
    }
    Ok(Some(n))
}
fn kernel_number(value: &str) -> Result<Option<u32>, LvmError> {
    if value.is_empty() || value == "-1" {
        Ok(None)
    } else {
        number(value).map(Some)
    }
}
/// Parse string-valued lvmreport LV JSON without I/O; unknown fields are ignored.
pub fn parse_lvs_json(bytes: &[u8]) -> Result<Vec<LvInfo>, LvmError> {
    let mut result = Vec::new();
    for row in rows(bytes, "lv")? {
        let f = |key| field(&row, key);
        result.push(LvInfo {
            vg: f("vg_name")?.into(),
            lv: f("lv_name")?.into(),
            attr: f("lv_attr")?.into(),
            segtype: f("segtype")?.into(),
            size: number(f("lv_size")?)?,
            data_pct: percent(f("data_percent")?)?,
            meta_pct: percent(f("metadata_percent")?)?,
            pool_lv: optional(f("pool_lv")?),
            dm_path: optional(f("lv_dm_path")?),
            major: kernel_number(f("lv_kernel_major")?)?,
            minor: kernel_number(f("lv_kernel_minor")?)?,
        });
    }
    result.sort_by(|a, b| (&a.vg, &a.lv).cmp(&(&b.vg, &b.lv)));
    Ok(result)
}
/// Parse string-valued lvmreport VG JSON without I/O.
pub fn parse_vgs_json(bytes: &[u8]) -> Result<Vec<VgInfo>, LvmError> {
    let mut result = Vec::new();
    for row in rows(bytes, "vg")? {
        result.push(VgInfo {
            vg: field(&row, "vg_name")?.into(),
            size: number(field(&row, "vg_size")?)?,
            free: number(field(&row, "vg_free")?)?,
        });
    }
    result.sort_by(|a, b| a.vg.cmp(&b.vg));
    Ok(result)
}
fn executable(default: &str, variable: &str) -> std::ffi::OsString {
    if cfg!(any(debug_assertions, feature = "privileged")) {
        if let Some(path) = std::env::var_os(variable) {
            return path;
        }
    }
    default.into()
}
fn drain(pipe: &mut impl std::io::Read, bytes: &mut Vec<u8>) -> Result<bool, LvmError> {
    let mut buffer = [0; 8192];
    // Bounded work per poll, so continuously writing children cannot evade timeout.
    for _ in 0..32 {
        match pipe.read(&mut buffer) {
            Ok(0) => return Ok(true),
            Ok(n) => {
                if bytes.len() + n > MAX_BYTES {
                    return Err(LvmError::Invalid("child output exceeds 16 MiB".into()));
                }
                bytes.extend_from_slice(&buffer[..n]);
            }
            Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => return Ok(false),
            Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(e) => return Err(e.into()),
        }
    }
    Ok(false)
}
fn command(
    binary: std::ffi::OsString,
    columns: &str,
    timeout: std::time::Duration,
) -> Result<Vec<u8>, LvmError> {
    use std::process::{Command, Stdio};
    let mut child = Command::new(binary)
        .args([
            "--reportformat",
            "json",
            "--units",
            "b",
            "--nosuffix",
            "-o",
            columns,
        ])
        .env("LC_ALL", "C")
        .env("LVM_SUPPRESS_FD_WARNINGS", "1")
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::NotFound {
                LvmError::NotInstalled
            } else {
                e.into()
            }
        })?;
    let result = (|| {
        let mut stdout = child
            .stdout
            .take()
            .ok_or_else(|| LvmError::Command("missing stdout".into()))?;
        let mut stderr = child
            .stderr
            .take()
            .ok_or_else(|| LvmError::Command("missing stderr".into()))?;
        for fd in [
            &stdout as &dyn std::os::fd::AsFd,
            &stderr as &dyn std::os::fd::AsFd,
        ] {
            let flags = rustix::fs::fcntl_getfl(fd).map_err(std::io::Error::from)?;
            rustix::fs::fcntl_setfl(fd, flags | rustix::fs::OFlags::NONBLOCK)
                .map_err(std::io::Error::from)?;
        }
        let started = std::time::Instant::now();
        let (mut out, mut err) = (Vec::new(), Vec::new());
        loop {
            if started.elapsed() >= timeout {
                return Err(LvmError::Timeout);
            }
            let out_done = drain(&mut stdout, &mut out)?;
            let err_done = drain(&mut stderr, &mut err)?;
            if let Some(status) = child.try_wait()? {
                if out_done && err_done {
                    return if status.success() {
                        Ok(out)
                    } else {
                        Err(LvmError::Command(format!(
                            "{status}: {}",
                            tidemark_core::path::display_lossy_bytes(&err)
                        )))
                    };
                }
            }
            std::thread::sleep(
                std::time::Duration::from_millis(5).min(timeout.saturating_sub(started.elapsed())),
            );
        }
    })();
    // Always reap, including timeout, invalid output and pipe errors.
    if result.is_err() {
        let _ = child.kill();
    }
    let waited = child.wait();
    match result {
        Ok(bytes) => {
            waited?;
            Ok(bytes)
        }
        Err(e) => Err(e),
    }
}
/// Run only the two reporting commands, with bounded output and a kill/reap deadline per child.
pub fn collect(cfg: &LvmConfig, now: i64) -> Result<LvmReport, LvmError> {
    if !cfg.enabled {
        return Err(LvmError::Disabled);
    }
    let lvs = command(executable("/usr/sbin/lvs","TIDEMARK_LVS_BIN"),"vg_name,lv_name,lv_attr,segtype,lv_size,data_percent,metadata_percent,pool_lv,lv_dm_path,lv_kernel_major,lv_kernel_minor",cfg.timeout)?;
    let vgs = command(
        executable("/usr/sbin/vgs", "TIDEMARK_VGS_BIN"),
        "vg_name,vg_size,vg_free",
        cfg.timeout,
    )?;
    Ok(LvmReport {
        lvs: parse_lvs_json(&lvs)?,
        vgs: parse_vgs_json(&vgs)?,
        sampled_at: now,
    })
}
/// Match device numbers regardless of device-mapper source spelling.
pub fn map_mounts(report: &LvmReport, mounts: &[MountInfo]) -> Vec<MountLv> {
    let mut result = Vec::new();
    for mount in mounts {
        if let Some(lv) = report
            .lvs
            .iter()
            .find(|lv| lv.major == Some(mount.major) && lv.minor == Some(mount.minor))
        {
            result.push(MountLv {
                mount: mount.mount_point.clone(),
                vg: lv.vg.clone(),
                lv: lv.lv.clone(),
                thin_pool: if lv.segtype == "thin" {
                    lv.pool_lv.clone()
                } else {
                    None
                },
            });
        }
    }
    result.sort_by(|a, b| a.mount.as_bytes().cmp(b.mount.as_bytes()));
    result
}
/// Compose current LV/VG sizes and pool percentages (never the thin LV percentages).
pub fn lvm_now(report: &LvmReport, m: &MountLv) -> LvmNow {
    let lv = report.lvs.iter().find(|lv| lv.vg == m.vg && lv.lv == m.lv);
    let pool = m
        .thin_pool
        .as_ref()
        .and_then(|name| report.lvs.iter().find(|lv| lv.vg == m.vg && lv.lv == *name));
    LvmNow {
        vg: m.vg.clone(),
        lv: m.lv.clone(),
        lv_size: lv.map_or(0, |lv| lv.size),
        vg_free: report
            .vgs
            .iter()
            .find(|vg| vg.vg == m.vg)
            .map_or(0, |vg| vg.free),
        thin_pool: m.thin_pool.clone(),
        data_pct: pool.and_then(|p| p.data_pct),
        meta_pct: pool.and_then(|p| p.meta_pct),
        sampled_at: report.sampled_at,
    }
}
/// Produce one sample per tracked LV, deduplicating bind mounts.
pub fn to_samples(report: &LvmReport, tracked: &[MountLv]) -> Vec<LvmSample> {
    let mut result = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    for mount in tracked {
        if !seen.insert((&mount.vg, &mount.lv)) {
            continue;
        }
        let now = lvm_now(report, mount);
        result.push(LvmSample {
            ts: now.sampled_at,
            vg: now.vg,
            lv: now.lv,
            lv_size: now.lv_size,
            vg_free: now.vg_free,
            data_pct: now.data_pct,
            meta_pct: now.meta_pct,
        });
    }
    result.sort_by(|a, b| (&a.vg, &a.lv).cmp(&(&b.vg, &b.lv)));
    result
}
#[cfg(test)]
#[path = "../tests/w4a/lvm.rs"]
mod tests;
