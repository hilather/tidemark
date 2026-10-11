//! Owned by: W1-A
//! Read-only host and root capability discovery with explicit unknown reasons.

use crate::{blockdev, canon, mount};
use serde::Serialize;
use std::{
    collections::BTreeMap,
    io::{self, Read},
    os::{fd::AsFd, unix::fs::MetadataExt},
    path::Path,
};
use tidemark_core::{config::Config, path::RawPath};

/// An optional fact with an explanation when discovery fails.
#[derive(Debug, Clone, Serialize)]
pub struct Fact {
    /// Displayable value; None means unknown.
    pub value: Option<String>,
    /// Reason for an unknown or qualified value.
    pub why: Option<String>,
}
impl Fact {
    fn known(value: impl ToString) -> Self {
        Self {
            value: Some(value.to_string()),
            why: None,
        }
    }
    fn unknown(why: impl ToString) -> Self {
        Self {
            value: None,
            why: Some(why.to_string()),
        }
    }
    fn result(value: io::Result<String>) -> Self {
        match value {
            Ok(value) => Self::known(value.trim()),
            Err(e) => Self::unknown(e),
        }
    }
}
/// Host facts keyed by stable descriptive names.
#[derive(Debug, Clone, Default, Serialize)]
pub struct HostFacts {
    /// OS, kernel, resources, privileges and watcher prerequisites.
    pub facts: BTreeMap<String, Fact>,
}
/// Facts about one canonicalized root.
#[derive(Debug, Clone, Serialize)]
pub struct RootFacts {
    /// Requested or canonical root, encoded as path/path_b64 in JSON.
    #[serde(serialize_with = "serialize_path")]
    pub root: RawPath,
    /// Mount, capacity, XFS, fanotify and LVM facts.
    pub facts: BTreeMap<String, Fact>,
    /// Physical whole-disk leaves, or unknown if resolution failed.
    pub leaves: Option<Vec<tidemark_core::dev::BlockDevInfo>>,
    /// Selected mount target with path/path_b64 byte-preserving encoding.
    pub mount_point: Option<serde_json::Value>,
    /// Same-device mounts; each preserves arbitrary path bytes.
    pub other_mounts: Option<Vec<serde_json::Value>>,
}
fn serialize_path<S: serde::Serializer>(path: &RawPath, serializer: S) -> Result<S::Ok, S::Error> {
    path.to_json().serialize(serializer)
}
/// A deterministic advisory derived solely from discovered facts.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct Recommendation {
    /// Stable rule identifier.
    pub code: String,
    /// Human-readable explanation.
    pub message: String,
}
/// Complete filesystem probe.
#[derive(Debug, Clone, Serialize)]
pub struct ProbeReport {
    /// Host prerequisites.
    pub host: HostFacts,
    /// One report per unique root.
    pub roots: Vec<RootFacts>,
    /// Pure advisory rules evaluated from these facts.
    pub recommendations: Vec<Recommendation>,
}
// Only kernel/configuration metadata files are read. Never read scanned file contents.
fn read_text(path: impl AsRef<Path>) -> io::Result<String> {
    let mut text = String::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_string(&mut text)?;
    if text.len() > 1024 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "metadata file exceeds 1 MiB",
        ));
    }
    Ok(text)
}
fn parse_os_release(text: &str) -> BTreeMap<String, String> {
    text.lines()
        .filter_map(|line| {
            let (key, value) = line.split_once('=')?;
            if key.is_empty() || !key.bytes().all(|b| b.is_ascii_uppercase() || b == b'_') {
                return None;
            }
            let value = value.trim();
            let value = value
                .strip_prefix('"')
                .and_then(|v| v.strip_suffix('"'))
                .or_else(|| value.strip_prefix('\'').and_then(|v| v.strip_suffix('\'')))
                .unwrap_or(value);
            Some((key.to_owned(), value.to_owned()))
        })
        .take(256)
        .collect()
}
fn parse_named(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (name, value) = line.split_once(':')?;
        (name == key).then(|| value.trim().to_owned())
    })
}
fn host_facts() -> HostFacts {
    let mut facts = BTreeMap::new();
    let os = read_text("/etc/os-release").map(|text| parse_os_release(&text));
    match &os {
        Ok(os) => {
            facts.insert(
                "os".into(),
                os.get("PRETTY_NAME")
                    .map(Fact::known)
                    .unwrap_or_else(|| Fact::unknown("PRETTY_NAME absent")),
            );
            facts.insert(
                "rocky_latest_minor".into(),
                match (os.get("ID"), os.get("VERSION_ID")) {
                    (Some(id), Some(version)) if id == "rocky" => match version
                        .strip_prefix("8.")
                        .and_then(|minor| minor.parse::<u32>().ok())
                    {
                        Some(minor) => Fact::known(minor >= 10),
                        None => Fact::unknown("Rocky latest-minor check applies to EL8 only"),
                    },
                    _ => Fact::unknown("host is not an identified Rocky Linux release"),
                },
            );
        }
        Err(e) => {
            facts.insert("os".into(), Fact::unknown(e));
            facts.insert("rocky_latest_minor".into(), Fact::unknown(e));
        }
    }
    facts.insert(
        "rocky_release".into(),
        Fact::result(read_text("/etc/rocky-release")),
    );
    facts.insert(
        "kernel".into(),
        Fact::known(rustix::system::uname().release().to_string_lossy()),
    );
    facts.insert(
        "cpus".into(),
        match std::thread::available_parallelism() {
            Ok(n) => Fact::known(n),
            Err(e) => Fact::unknown(e),
        },
    );
    facts.insert(
        "memory".into(),
        match read_text("/proc/meminfo") {
            Ok(text) => parse_named(&text, "MemTotal")
                .map(Fact::known)
                .unwrap_or_else(|| Fact::unknown("MemTotal absent")),
            Err(e) => Fact::unknown(e),
        },
    );
    facts.insert(
        "effective_capabilities".into(),
        match read_text("/proc/self/status") {
            Ok(text) => parse_named(&text, "CapEff")
                .map(Fact::known)
                .unwrap_or_else(|| Fact::unknown("CapEff absent")),
            Err(e) => Fact::unknown(e),
        },
    );
    facts.insert(
        "mount_namespace_equal".into(),
        match (
            std::fs::metadata("/proc/self/ns/mnt"),
            std::fs::metadata("/proc/1/ns/mnt"),
        ) {
            (Ok(a), Ok(b)) => Fact::known(a.dev() == b.dev() && a.ino() == b.ino()),
            (Err(e), _) | (_, Err(e)) => Fact::unknown(e),
        },
    );
    facts.insert(
        "cgroup_version".into(),
        match rustix::fs::statfs("/sys/fs/cgroup") {
            Ok(s) => Fact::known(if s.f_type as u64 == 0x6367_7270 {
                "2"
            } else if s.f_type as u64 == 0x0102_1994 {
                "1 (or hybrid)"
            } else {
                "unrecognized"
            }),
            Err(e) => Fact::unknown(e),
        },
    );
    facts.insert("btf".into(), presence("/sys/kernel/btf/vmlinux"));
    // RPM query reads the installed-package database; no package changes are made.
    facts.insert("bcc_tools".into(), rpm_bcc_tools());
    HostFacts { facts }
}
fn rpm_bcc_tools() -> Fact {
    let child = std::process::Command::new("rpm")
        .args(["-q", "--quiet", "bcc-tools"])
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn();
    let mut child = match child {
        Ok(child) => child,
        Err(e) => return Fact::unknown(e),
    };
    let start = std::time::Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => return Fact::known(true),
            Ok(Some(status)) if status.code() == Some(1) => return Fact::known(false),
            Ok(Some(status)) => return Fact::unknown(format!("rpm query exited {status}")),
            Err(e) => {
                let _ = child.kill();
                let _ = child.wait();
                return Fact::unknown(e);
            }
            Ok(None) => {}
        }
        if start.elapsed() >= std::time::Duration::from_secs(2) {
            let _ = child.kill();
            let _ = child.wait();
            return Fact::unknown("rpm query timed out after 2 seconds");
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
fn presence(path: &str) -> Fact {
    match std::fs::metadata(path) {
        Ok(_) => Fact::known(true),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Fact::known(false),
        Err(e) => Fact::unknown(e),
    }
}
/// Probe configured and extra roots without failing the overall report.
/// Discovery is capped at 1024 unique roots; excess inputs produce an advisory.
pub fn probe(cfg: &Config, extra_roots: &[RawPath]) -> ProbeReport {
    let host = host_facts();
    let mounts = mount::read_mountinfo();
    // W4-A LVM section: collect once for all roots; probe does not expose a timestamp.
    let lvm_report = std::cell::OnceCell::new();
    let mut roots = Vec::new();
    let mut seen = std::collections::BTreeSet::new();
    let configured = cfg.roots.iter().map(|r| r.path.clone());
    let mut truncated = false;
    for requested in configured.chain(extra_roots.iter().map(|r| r.as_path().to_path_buf())) {
        if seen.len() == 1024 {
            truncated = true;
            break;
        }
        let canonical = canon::canonicalize(&requested);
        // An unresolved label is for display only, never for discovery.
        let root = match &canonical {
            Ok(root) => root.clone(),
            Err(_) => {
                let absolute = if requested.is_absolute() {
                    requested.clone()
                } else {
                    std::env::current_dir()
                        .unwrap_or_else(|_| std::path::PathBuf::from("/"))
                        .join(&requested)
                };
                match RawPath::from_path(&absolute) {
                    Ok(label) => label,
                    Err(error) => {
                        tracing::warn!(%error, "invalid probe root label");
                        continue;
                    }
                }
            }
        };
        let canonical_error = canonical.as_ref().err().map(ToString::to_string);
        if !seen.insert((root.clone(), canonical_error.clone())) {
            continue;
        }
        let mut report = RootFacts {
            root,
            facts: BTreeMap::new(),
            leaves: None,
            mount_point: None,
            other_mounts: None,
        };
        if let Err(e) = &canonical {
            report
                .facts
                .insert("canonical_path".into(), Fact::unknown(e));
        }
        let selected = canonical.as_ref().ok().and_then(|_| {
            mounts
                .as_ref()
                .ok()
                .and_then(|m| mount::mount_for_path(m, &report.root))
        });
        match selected {
            Some(m) => {
                report.mount_point = Some(m.mount_point.to_json());
                report.facts.insert(
                    "mount_point".into(),
                    Fact::known(m.mount_point.display_lossy()),
                );
                report.facts.insert(
                    "fs_type".into(),
                    Fact::known(mount::display_field(m.fs_type.as_bytes())),
                );
                report.facts.insert("source".into(), Fact::known(&m.source));
                report
                    .facts
                    .insert("mount_options".into(), Fact::known(m.mount_opts.join(",")));
                report
                    .facts
                    .insert("super_options".into(), Fact::known(m.super_opts.join(",")));
                report.facts.insert(
                    "atime".into(),
                    Fact::known(
                        m.mount_opts
                            .iter()
                            .find(|o| ["noatime", "relatime", "strictatime"].contains(&o.as_str()))
                            .map(String::as_str)
                            .unwrap_or("strictatime"),
                    ),
                );
                match blockdev::resolve_details(m.dev) {
                    Ok((leaves, levels)) => {
                        report.facts.insert(
                            "md_level".into(),
                            Fact::known(if levels.is_empty() {
                                "n/a".into()
                            } else {
                                levels.join(", ")
                            }),
                        );
                        report
                            .facts
                            .insert("leaf_devices".into(), Fact::known(leaves.len()));
                        report.leaves = Some(leaves);
                    }
                    Err(e) => {
                        report
                            .facts
                            .insert("leaf_devices".into(), Fact::unknown(&e));
                        report.facts.insert("md_level".into(), Fact::unknown(e));
                    }
                }
                if let Ok(mounts) = &mounts {
                    report.other_mounts = Some(
                        mount::other_mounts_of_dev(mounts, m.dev, &m.mount_point)
                            .iter()
                            .map(|m| m.mount_point.to_json())
                            .collect(),
                    );
                }
                xfs_facts(&mut report, &m.fs_type);
                match tidemark_sys::fanotify::probe(m.mount_point.as_path()) {
                    Ok(c) => {
                        for (key, value) in [
                            ("fanotify_available", c.available),
                            ("fanotify_fd_mode", c.fd_mode),
                            ("fanotify_fid", c.report_fid),
                            ("fanotify_dfid_name", c.report_dfid_name),
                            ("fanotify_mark_filesystem", c.mark_filesystem),
                        ] {
                            report.facts.insert(
                                key.into(),
                                Fact {
                                    value: Some(value.to_string()),
                                    why: c.error.clone(),
                                },
                            );
                        }
                    }
                    Err(e) => {
                        for key in [
                            "fanotify_available",
                            "fanotify_fd_mode",
                            "fanotify_fid",
                            "fanotify_dfid_name",
                            "fanotify_mark_filesystem",
                        ] {
                            report.facts.insert(key.into(), Fact::unknown(&e));
                        }
                    }
                }
            }
            None => {
                let why = canonical_error
                    .clone()
                    .or_else(|| mounts.as_ref().err().map(ToString::to_string))
                    .unwrap_or_else(|| "no containing mount found".into());
                for key in [
                    "mount_point",
                    "fs_type",
                    "source",
                    "mount_options",
                    "super_options",
                    "atime",
                    "leaf_devices",
                    "md_level",
                    "xfs_geometry",
                    "xfs_v5",
                    "xfs_ftype",
                    "xfs_reflink",
                    "bulkstat_v1",
                    "bulkstat_v5",
                    "fanotify_available",
                    "fanotify_fd_mode",
                    "fanotify_fid",
                    "fanotify_dfid_name",
                    "fanotify_mark_filesystem",
                ] {
                    report.facts.insert(key.into(), Fact::unknown(&why));
                }
            }
        }
        if let Some(why) = canonical_error {
            for key in [
                "total_bytes",
                "free_bytes",
                "available_bytes",
                "total_inodes",
                "free_inodes",
                "lvm",
            ] {
                report.facts.insert(key.into(), Fact::unknown(&why));
            }
            roots.push(report);
            continue;
        }
        capacity_facts(&mut report);
        // W4-A LVM section: failures remain advisory and do not affect other probe facts.
        let fact = match lvm_report.get_or_init(|| crate::lvm::collect(&cfg.lvm, 0)) {
            Ok(_) if mounts.is_err() => Fact::unknown("mount table unavailable for LVM mapping"),
            Ok(lvm) => {
                let mapped = mounts.as_ref().ok().and_then(|mounts| {
                    mount::mount_for_path(mounts, &report.root).and_then(|mount| {
                        crate::lvm::map_mounts(lvm, std::slice::from_ref(mount))
                            .into_iter()
                            .next()
                    })
                });
                match mapped {
                    Some(mapped) => {
                        let current = crate::lvm::lvm_now(lvm, &mapped);
                        Fact::known(format!(
                            "LV {}/{}; thin pool {}; VG free {} bytes",
                            tidemark_core::path::display_lossy_bytes(current.vg.as_bytes()),
                            tidemark_core::path::display_lossy_bytes(current.lv.as_bytes()),
                            current
                                .thin_pool
                                .as_deref()
                                .map(|p| tidemark_core::path::display_lossy_bytes(p.as_bytes()))
                                .unwrap_or_else(|| "n/a".into()),
                            current.vg_free
                        ))
                    }
                    None => Fact::known("n/a"),
                }
            }
            Err(crate::lvm::LvmError::NotInstalled | crate::lvm::LvmError::Disabled) => {
                Fact::known("n/a")
            }
            Err(e) => Fact::unknown(e),
        };
        report.facts.insert("lvm".into(), fact);
        roots.push(report);
    }
    let mut report = ProbeReport {
        host,
        roots,
        recommendations: Vec::new(),
    };
    report.recommendations = recommendations(&report.host, &report.roots);
    if truncated {
        report.recommendations.push(Recommendation {
            code: "root_limit".into(),
            message: "Probe limited to 1024 unique roots".into(),
        });
    }
    report
}
fn capacity_facts(report: &mut RootFacts) {
    match rustix::fs::statvfs(report.root.as_path()) {
        Ok(s) => {
            for (key, value) in [
                ("total_bytes", s.f_blocks.checked_mul(s.f_frsize)),
                ("free_bytes", s.f_bfree.checked_mul(s.f_frsize)),
                ("available_bytes", s.f_bavail.checked_mul(s.f_frsize)),
                ("total_inodes", Some(s.f_files)),
                ("free_inodes", Some(s.f_ffree)),
            ] {
                report.facts.insert(
                    key.into(),
                    value
                        .map(Fact::known)
                        .unwrap_or_else(|| Fact::unknown("statvfs byte count overflow")),
                );
            }
        }
        Err(e) => {
            for key in [
                "total_bytes",
                "free_bytes",
                "available_bytes",
                "total_inodes",
                "free_inodes",
            ] {
                report.facts.insert(key.into(), Fact::unknown(e));
            }
        }
    }
}
fn xfs_facts(report: &mut RootFacts, fs_type: &str) {
    let keys = [
        "xfs_geometry",
        "xfs_v5",
        "xfs_ftype",
        "xfs_reflink",
        "bulkstat_v1",
        "bulkstat_v5",
    ];
    if fs_type != "xfs" {
        for key in keys {
            report.facts.insert(
                key.into(),
                Fact {
                    value: Some("n/a".into()),
                    why: Some("filesystem is not XFS".into()),
                },
            );
        }
        return;
    }
    let open = |flags| rustix::fs::open(report.root.as_path(), flags, rustix::fs::Mode::empty());
    let flags = rustix::fs::OFlags::RDONLY
        | rustix::fs::OFlags::DIRECTORY
        | rustix::fs::OFlags::NOFOLLOW
        | rustix::fs::OFlags::CLOEXEC;
    let fd = match open(flags | rustix::fs::OFlags::NOATIME) {
        Err(rustix::io::Errno::PERM) => open(flags),
        result => result,
    };
    let result = fd
        .map_err(io::Error::from)
        .and_then(|fd| tidemark_sys::xfs::geometry(fd.as_fd()).map(|geo| (fd, geo)));
    match result {
        Ok((fd, g)) => {
            report.facts.insert(
                "xfs_geometry".into(),
                Fact::known(format!(
                    "blocksize={} agblocks={} agcount={} inodesize={} dirblocksize={} uuid={:02x?}",
                    g.blocksize, g.agblocks, g.agcount, g.inodesize, g.dirblocksize, g.uuid
                )),
            );
            for (key, value) in [
                ("xfs_v5", g.v5),
                ("xfs_ftype", g.ftype),
                ("xfs_reflink", g.reflink),
            ] {
                report.facts.insert(
                    key.into(),
                    value
                        .map(Fact::known)
                        .unwrap_or_else(|| Fact::unknown("geometry does not expose feature")),
                );
            }
            let support = tidemark_sys::xfs::bulkstat_probe(fd.as_fd());
            report
                .facts
                .insert("bulkstat_v1".into(), Fact::known(support.v1));
            report
                .facts
                .insert("bulkstat_v5".into(), Fact::known(support.v5));
        }
        Err(e) => {
            for key in keys {
                report.facts.insert(
                    key.into(),
                    Fact::unknown(format!(
                        "XFS geometry unavailable: {e} (implementation or permissions)"
                    )),
                );
            }
        }
    }
}
/// Apply advisory rules without I/O; unknown facts never imply negative capabilities.
pub fn recommendations(host: &HostFacts, roots: &[RootFacts]) -> Vec<Recommendation> {
    let mut rules = BTreeMap::new();
    let value = |facts: &BTreeMap<String, Fact>, key: &str, expected: &str| {
        facts.get(key).and_then(|f| f.value.as_deref()) == Some(expected)
    };
    if value(&host.facts, "rocky_latest_minor", "false") {
        rules.insert(
            "update_rocky",
            "Update Rocky Linux to the target EL8 minor release 8.10",
        );
    }
    if value(&host.facts, "mount_namespace_equal", "false") {
        rules.insert(
            "mount_namespace",
            "Mount namespace differs from PID 1; the watcher will refuse to run",
        );
    }
    for root in roots {
        if root.leaves.as_ref().is_some_and(|leaves| {
            leaves
                .iter()
                .any(|d| d.scheduler.as_deref().is_some_and(|s| s != "bfq"))
        }) {
            rules.insert(
                "scheduler",
                "ionice ineffective; Tidemark's throttle is used",
            );
        }
        if root
            .other_mounts
            .as_ref()
            .is_some_and(|mounts| !mounts.is_empty())
        {
            rules.insert("bind_mounts","Other mounts of the same device exist; watcher fd mode misses writes through other mounts");
        }
        if value(&root.facts, "xfs_ftype", "false") {
            rules.insert("ftype", "ftype=0; bulk mode unavailable");
        }
        if value(&root.facts, "xfs_reflink", "true") {
            rules.insert(
                "reflink",
                "t_disk may overstate physical use for reflinked copies",
            );
        }
    }
    rules
        .into_iter()
        .map(|(code, message)| Recommendation {
            code: code.into(),
            message: message.into(),
        })
        .collect()
}
#[cfg(test)]
#[path = "../tests/w1a/probe.rs"]
mod tests;
