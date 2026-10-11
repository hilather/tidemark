//! Owned by: W1-D
//! Bounded, metadata-only Linux procfs readers and pure parsers.

use std::{
    fs,
    io::{self, Read},
    os::unix::{ffi::OsStrExt, fs::MetadataExt},
};
use tidemark_core::scan::{DevId, Ino};

// Explicit resource limits: never return a silently truncated scan.
const MAX_PROC_TEXT: u64 = 1024 * 1024;
const MAX_IDS: usize = 1_048_576;
const MAX_OPEN_FILES: usize = 1_048_576;

fn invalid() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        "invalid or oversized procfs data",
    )
}
fn bytes(path: impl AsRef<std::path::Path>) -> io::Result<Vec<u8>> {
    let mut bytes = Vec::new();
    fs::File::open(path)?
        .take(MAX_PROC_TEXT + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_PROC_TEXT {
        return Err(invalid());
    }
    Ok(bytes)
}
fn text(path: impl AsRef<std::path::Path>) -> io::Result<String> {
    String::from_utf8(bytes(path)?).map_err(|_| invalid())
}
fn ids(path: &str) -> io::Result<Vec<u32>> {
    let mut result = Vec::new();
    for entry in fs::read_dir(path)? {
        let entry = match entry {
            Ok(e) => e,
            Err(e) if transient(&e) => continue,
            Err(e) => return Err(e),
        };
        if let Some(id) = entry.file_name().to_str().and_then(|s| s.parse().ok()) {
            if result.len() == MAX_IDS {
                return Err(invalid());
            }
            result.push(id);
        }
    }
    result.sort_unstable();
    Ok(result)
}
fn transient(e: &io::Error) -> bool {
    matches!(
        e.raw_os_error(),
        Some(libc::ENOENT | libc::ESRCH | libc::EACCES | libc::EPERM)
    )
}
/// Enumerate numeric process IDs, sorted numerically.
pub fn list_pids() -> io::Result<Vec<u32>> {
    ids("/proc")
}
/// Read a process comm, removing only its final newline.
pub fn read_comm(pid: u32) -> io::Result<String> {
    let mut comm = String::from_utf8_lossy(&bytes(format!("/proc/{pid}/comm"))?).into_owned();
    if comm.ends_with('\n') {
        comm.pop();
    }
    Ok(comm)
}
/// Parse field 22, after the last closing parenthesis of the comm field.
pub fn parse_stat_starttime(stat: &str) -> Option<u64> {
    let (head, tail) = stat.rsplit_once(')')?;
    let (pid, _) = head.split_once('(')?;
    pid.trim().parse::<u32>().ok()?;
    tail.split_whitespace().nth(19)?.parse().ok()
}
/// Read the kernel starttime (clock ticks since boot).
pub fn read_starttime(pid: u32) -> io::Result<u64> {
    parse_stat_starttime(&String::from_utf8_lossy(&bytes(format!(
        "/proc/{pid}/stat"
    ))?))
    .ok_or_else(invalid)
}
/// Select the systemd hierarchy and its last unit; user sessions become user-UID.
pub fn parse_cgroup_unit(cgroup: &str) -> Option<String> {
    for line in cgroup.lines() {
        let mut fields = line.splitn(3, ':');
        let (Some(hierarchy), Some(controllers), Some(path)) =
            (fields.next(), fields.next(), fields.next())
        else {
            continue;
        };
        if !(hierarchy == "0" && controllers.is_empty()
            || controllers.split(',').any(|c| c == "name=systemd"))
        {
            continue;
        }
        let components = path.split('/');
        if let Some(uid) = components.clone().find_map(|c| {
            c.strip_prefix("user-")
                .and_then(|s| s.strip_suffix(".slice"))
                .filter(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
        }) {
            return Some(format!("user-{uid}"));
        }
        if let Some(unit) = components
            .rev()
            .find(|c| c.ends_with(".service") || c.ends_with(".scope") || c.ends_with(".slice"))
        {
            return Some(unit.to_owned());
        }
    }
    None
}
/// Read a process's systemd unit, if present.
pub fn read_cgroup_unit(pid: u32) -> io::Result<Option<String>> {
    Ok(parse_cgroup_unit(&text(format!("/proc/{pid}/cgroup"))?))
}
/// Enumerate a process's numeric file descriptors.
pub fn list_fds(pid: u32) -> io::Result<Vec<u32>> {
    ids(&format!("/proc/{pid}/fd"))
}
/// Return readlink text as raw bytes, without interpreting it as a filesystem path.
pub fn read_fd_target(pid: u32, fd: u32) -> io::Result<Vec<u8>> {
    Ok(fs::read_link(format!("/proc/{pid}/fd/{fd}"))?
        .as_os_str()
        .as_bytes()
        .to_vec())
}
/// Parse the octal flags field in fdinfo.
pub fn parse_fdinfo_flags(fdinfo: &str) -> Option<u32> {
    fdinfo.lines().find_map(|line| {
        line.strip_prefix("flags:")
            .and_then(|s| u32::from_str_radix(s.trim(), 8).ok())
    })
}
/// Read a descriptor's open flags without opening the target.
pub fn read_fdinfo_flags(pid: u32, fd: u32) -> io::Result<u32> {
    parse_fdinfo_flags(&text(format!("/proc/{pid}/fdinfo/{fd}"))?).ok_or_else(invalid)
}
/// Read the real UID from process status.
pub fn read_uid(pid: u32) -> io::Result<u32> {
    parse_uid(&text(format!("/proc/{pid}/status"))?).ok_or_else(invalid)
}
fn parse_uid(status: &str) -> Option<u32> {
    status.lines().find_map(|l| {
        l.strip_prefix("Uid:")
            .and_then(|s| s.split_whitespace().next()?.parse().ok())
    })
}

/// Metadata for an open regular-file descriptor.
pub struct OpenFile {
    /// Process ID.
    pub pid: u32,
    /// Descriptor number.
    pub fd: u32,
    /// Raw readlink text, including a possible deleted suffix.
    pub target: Vec<u8>,
    /// Kernel device ID.
    pub dev: DevId,
    /// Inode number.
    pub ino: Ino,
    /// Current hard-link count; zero means unlinked.
    pub nlink: u64,
    /// Apparent size in bytes.
    pub size: u64,
    /// Allocated blocks in bytes (st_blocks times 512).
    pub disk: u64,
    /// Octal fdinfo open flags, if requested and available.
    pub flags: Option<u32>,
}
/// Collect regular-file metadata on selected devices, including unlinked open files.
/// Procfs fd magic links are stat-ed intentionally; readlink text is never followed
/// or opened. Exiting/inaccessible processes are skipped. Exceeding a cap errors.
pub fn scan_open_files(devs: &[DevId], want_flags: bool) -> io::Result<Vec<OpenFile>> {
    if devs.is_empty() {
        return Ok(Vec::new());
    }
    let mut files = Vec::new();
    for pid in list_pids()? {
        let start = match read_starttime(pid) {
            Ok(s) => s,
            Err(e) if transient(&e) => continue,
            Err(e) => return Err(e),
        };
        let fds = match list_fds(pid) {
            Ok(f) => f,
            Err(e) if transient(&e) => continue,
            Err(e) => return Err(e),
        };
        let base = files.len();
        for fd in fds {
            let path = format!("/proc/{pid}/fd/{fd}");
            let metadata = match fs::metadata(&path) {
                Ok(m) => m,
                Err(e) if transient(&e) => continue,
                Err(e) => return Err(e),
            };
            if !metadata.is_file() || !devs.contains(&metadata.dev()) {
                continue;
            }
            let target = match read_fd_target(pid, fd) {
                Ok(t) => t,
                Err(e) if transient(&e) => continue,
                Err(e) => return Err(e),
            };
            let flags = if want_flags {
                match read_fdinfo_flags(pid, fd) {
                    Ok(f) => Some(f),
                    Err(e) if transient(&e) => continue,
                    Err(_) => None,
                }
            } else {
                None
            };
            // Reject descriptor reuse detectable by inode/device changes during the reads.
            let check = match fs::metadata(&path) {
                Ok(m) => m,
                Err(e) if transient(&e) => continue,
                Err(e) => return Err(e),
            };
            if metadata.dev() != check.dev() || metadata.ino() != check.ino() {
                continue;
            }
            if files.len() == MAX_OPEN_FILES {
                return Err(invalid());
            }
            files.push(OpenFile {
                pid,
                fd,
                target,
                dev: metadata.dev(),
                ino: metadata.ino(),
                nlink: metadata.nlink(),
                size: metadata.len(),
                disk: metadata.blocks().saturating_mul(512),
                flags,
            });
        }
        if read_starttime(pid).ok() != Some(start) {
            files.truncate(base);
        }
    }
    Ok(files)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn uid_parser_and_error_policy() {
        assert_eq!(
            parse_uid("Name:\tworker\nUid:\t1000\t1001\t1002\t1003\n"),
            Some(1000)
        );
        assert!(parse_uid(include_str!("../tests/w1d/data/status.txt")).is_some());
        assert_eq!(parse_uid("Uid: -1"), None);
        assert_eq!(parse_uid("Uid: 4294967296"), None);
        assert_eq!(parse_uid("Uid:"), None);
        assert!(transient(&io::Error::from_raw_os_error(libc::ESRCH)));
        assert!(!transient(&io::Error::from_raw_os_error(libc::EIO)));
    }
}
