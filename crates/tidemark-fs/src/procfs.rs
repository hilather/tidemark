//! Owned by: W1-D
//! Bounded, metadata-only Linux procfs readers and pure parsers.

use std::{
    fs,
    io::{self, Read},
    os::unix::fs::MetadataExt,
};
use tidemark_core::scan::{DevId, Ino};

// Explicit resource limits: never return a silently truncated scan.
const MAX_PROC_TEXT: u64 = 1024 * 1024;
const MAX_IDS: usize = 1_048_576;
const MAX_OPEN_FILES: usize = 1_048_576;
const ID_SCRATCH_BYTES: usize = 2 * MAX_IDS * std::mem::size_of::<u32>();
/// Maximum retained inventory allocation, including record capacity and target capacity.
/// Includes a conservative reservation for both MAX_IDS enumeration vectors.
pub const MAX_INVENTORY_BYTES: usize = 128 * 1024 * 1024;

/// A complete process inventory could not fit its memory budget.
#[derive(Debug, thiserror::Error)]
#[error("proc inventory byte budget exceeded: {count} descriptors, {bytes} bytes retained")]
pub struct InventoryBudgetExceeded {
    /// Descriptors retained before the rejected allocation.
    pub count: usize,
    /// Accounted allocation bytes before the rejected allocation.
    pub bytes: usize,
}
fn budget_error(count: usize, bytes: usize) -> io::Error {
    io::Error::new(
        io::ErrorKind::OutOfMemory,
        InventoryBudgetExceeded { count, bytes },
    )
}

struct Inventory {
    files: Vec<OpenFile>,
    paths: usize,
    limit: usize,
    scratch: usize,
}
impl Inventory {
    fn bytes(&self) -> usize {
        self.scratch + self.paths + self.files.capacity() * std::mem::size_of::<OpenFile>()
    }
    fn push(&mut self, file: OpenFile) -> io::Result<()> {
        let before = self.bytes();
        let grow = if self.files.len() == self.files.capacity() {
            self.files.capacity().max(256)
        } else {
            0
        };
        // Charge both allocations during a moving reallocation. Geometric growth
        // avoids quadratic record copying without an uncharged transient old buffer.
        let allocation = if grow == 0 {
            0
        } else {
            (self.files.capacity() + grow) * std::mem::size_of::<OpenFile>()
        };
        if before
            .saturating_add(allocation)
            .saturating_add(file.target.capacity())
            > self.limit
        {
            return Err(budget_error(self.files.len(), before));
        }
        if grow != 0 {
            self.files
                .try_reserve_exact(grow)
                .map_err(|_| budget_error(self.files.len(), before))?;
        }
        // Account the actual capacity returned by the allocator as well.
        if self.bytes().saturating_add(file.target.capacity()) > self.limit {
            return Err(budget_error(self.files.len(), self.bytes()));
        }
        self.paths += file.target.capacity();
        self.files.push(file);
        Ok(())
    }
    fn truncate(&mut self, len: usize) {
        self.paths -= self.files[len..]
            .iter()
            .map(|f| f.target.capacity())
            .sum::<usize>();
        self.files.truncate(len);
    }
}

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
    Ok(String::from_utf8_lossy(&bytes(path)?).into_owned())
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
            if result.len() == result.capacity() {
                result
                    .try_reserve_exact(256)
                    .map_err(|_| io::Error::from_raw_os_error(libc::ENOMEM))?;
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
// A descriptor can point at a broken remote filesystem. Only resource
// shortages invalidate the inventory and require the caller to retry.
fn per_fd<T>(result: io::Result<T>) -> io::Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error)
            if matches!(
                error.raw_os_error(),
                Some(libc::ENOMEM | libc::EMFILE | libc::ENFILE)
            ) =>
        {
            Err(error)
        }
        Err(_) => Ok(None),
    }
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
    read_starttime_from(fs::File::open(format!("/proc/{pid}/stat"))?)
}
fn read_starttime_from(mut reader: impl Read) -> io::Result<u64> {
    let mut buffer = [0; 512];
    let count = reader.read(&mut buffer)?;
    // A complete procfs stat record ends in newline. Full/short incomplete
    // records fail closed rather than parsing a truncated identity.
    if count == buffer.len() || !buffer[..count].ends_with(b"\n") {
        return Err(invalid());
    }
    parse_stat_starttime(&String::from_utf8_lossy(&buffer[..count])).ok_or_else(invalid)
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
    use std::os::unix::ffi::OsStringExt;
    Ok(fs::read_link(format!("/proc/{pid}/fd/{fd}"))?
        .into_os_string()
        .into_vec())
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
    parse_uid(&bytes(format!("/proc/{pid}/status"))?).ok_or_else(invalid)
}
fn parse_uid(status: &[u8]) -> Option<u32> {
    status.split(|b| *b == b'\n').find_map(|line| {
        let fields = line.strip_prefix(b"Uid:")?;
        let uid = fields
            .split(u8::is_ascii_whitespace)
            .find(|s| !s.is_empty())?;
        std::str::from_utf8(uid).ok()?.parse().ok()
    })
}

/// Metadata for an open regular-file descriptor.
pub struct OpenFile {
    /// Process ID.
    pub pid: u32,
    /// Validated process start time during descriptor enumeration.
    pub starttime: u64,
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
// Cache lifetime is one inventory. Namespace and mount vectors share its byte
// budget; growth admits both old and new buffers before allocating.
struct MountNamespace {
    key: (u64, u64),
    mounts: Vec<(u64, DevId)>,
}
fn reserve_cache<T>(values: &mut Vec<T>, inventory: &mut Inventory) -> io::Result<()> {
    if values.len() < values.capacity() {
        return Ok(());
    }
    let before = inventory.bytes();
    let old = values.capacity() * std::mem::size_of::<T>();
    let grow = values.capacity().max(16);
    let new = (values.capacity() + grow) * std::mem::size_of::<T>();
    if before.saturating_add(new) > inventory.limit {
        return Err(budget_error(inventory.files.len(), before));
    }
    values
        .try_reserve_exact(grow)
        .map_err(|_| budget_error(inventory.files.len(), before))?;
    inventory.scratch += values.capacity() * std::mem::size_of::<T>() - old;
    if inventory.bytes() > inventory.limit {
        return Err(budget_error(inventory.files.len(), inventory.bytes()));
    }
    Ok(())
}
fn parse_mnt_id(fdinfo: &str) -> Option<u64> {
    fdinfo
        .lines()
        .find_map(|line| line.strip_prefix("mnt_id:")?.trim().parse().ok())
}
fn parse_mount_device(line: &str) -> Option<(u64, DevId)> {
    let mut fields = line.split_ascii_whitespace();
    let id = fields.next()?.parse().ok()?;
    fields.next()?.parse::<u64>().ok()?;
    let (major, minor) = fields.next()?.split_once(':')?;
    let major = major.parse::<u32>().ok()?;
    let minor = minor.parse::<u32>().ok()?;
    // Root, mount point and options precede optional fields and the separator.
    fields.next()?;
    fields.next()?;
    fields.next()?;
    fields.find(|field| *field == "-")?;
    fields.next()?;
    fields.next()?;
    fields.next()?;
    Some((id, libc::makedev(major, minor)))
}
// Transient lookup failures discard the fd. Other unavailable mappings fall
// back to stat; resource shortages must propagate for daemon retries.
fn mapping_read<T>(result: io::Result<T>) -> io::Result<Option<Option<T>>> {
    match result {
        Ok(value) => Ok(Some(Some(value))),
        Err(error) if transient(&error) => Ok(None),
        Err(error) => per_fd::<T>(Err(error)).map(Some),
    }
}
fn process_mounts(
    pid: u32,
    cache: &mut Vec<MountNamespace>,
    inventory: &mut Inventory,
) -> io::Result<Option<Option<usize>>> {
    let Some(key) = mapping_read(fs::metadata(format!("/proc/{pid}/ns/mnt")))? else {
        return Ok(None);
    };
    let Some(key) = key else {
        return Ok(Some(None));
    };
    let key = (key.dev(), key.ino());
    if let Some(index) = cache.iter().position(|entry| entry.key == key) {
        return Ok(Some(Some(index)));
    }
    let Some(data) = mapping_read(text(format!("/proc/{pid}/mountinfo")))? else {
        return Ok(None);
    };
    let Some(data) = data else {
        return Ok(Some(None));
    };
    let mut mounts = Vec::new();
    for line in data.lines() {
        if let Some(entry) = parse_mount_device(line) {
            reserve_cache(&mut mounts, inventory)?;
            mounts.push(entry);
        }
    }
    mounts.sort_unstable();
    reserve_cache(cache, inventory)?;
    let index = cache.len();
    cache.push(MountNamespace { key, mounts });
    Ok(Some(Some(index)))
}
fn mapped_device(mounts: &[(u64, DevId)], id: u64) -> Option<DevId> {
    mounts
        .binary_search_by_key(&id, |entry| entry.0)
        .ok()
        .map(|index| mounts[index].1)
}

fn stat_on_watched_mount<T>(
    fdinfo: &str,
    mounts: Option<&[(u64, DevId)]>,
    devs: &[DevId],
    stat: impl FnOnce() -> io::Result<T>,
) -> io::Result<Option<T>> {
    if let Some(device) = mounts.and_then(|map| mapped_device(map, parse_mnt_id(fdinfo)?)) {
        if !devs.contains(&device) {
            return Ok(None);
        }
    }
    per_fd(stat())
}

/// Collect regular-file metadata on selected devices, including unlinked open files.
/// Procfs does no disk I/O, so Gate charging and O_NOATIME do not apply.
/// Procfs fd magic links are stat-ed intentionally; readlink text is never followed
/// or opened. Exiting/inaccessible processes are skipped. Exceeding a cap errors.
pub fn scan_open_files(devs: &[DevId], want_flags: bool) -> io::Result<Vec<OpenFile>> {
    if devs.is_empty() {
        return Ok(Vec::new());
    }
    let mut inventory = Inventory {
        files: Vec::new(),
        paths: 0,
        limit: MAX_INVENTORY_BYTES,
        // Reserve for bounded proc text and its lossy UTF-8 conversion,
        // including simultaneous fdinfo and mountinfo buffers.
        scratch: ID_SCRATCH_BYTES + 8 * MAX_PROC_TEXT as usize,
    };
    let mut mount_cache = Vec::new();
    let pids = list_pids()?;
    for pid in pids {
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
        let Some(mounts) = process_mounts(pid, &mut mount_cache, &mut inventory)? else {
            continue;
        };
        let base = inventory.files.len();
        for fd in fds {
            let Some(fdinfo) = per_fd(text(format!("/proc/{pid}/fdinfo/{fd}")))? else {
                continue;
            };
            let map = mounts.map(|index| mount_cache[index].mounts.as_slice());
            let path = format!("/proc/{pid}/fd/{fd}");
            let Some(metadata) = stat_on_watched_mount(&fdinfo, map, devs, || fs::metadata(&path))?
            else {
                continue;
            };
            if !metadata.is_file() || !devs.contains(&metadata.dev()) {
                continue;
            }
            let Some(target) = per_fd(read_fd_target(pid, fd))? else {
                continue;
            };
            let flags = if want_flags {
                let Some(flags) = per_fd(parse_fdinfo_flags(&fdinfo).ok_or_else(invalid))? else {
                    continue;
                };
                Some(flags)
            } else {
                None
            };
            // Reject descriptor reuse detectable by inode/device changes during the reads.
            let Some(check_info) = per_fd(text(format!("/proc/{pid}/fdinfo/{fd}")))? else {
                continue;
            };
            let Some(check) =
                stat_on_watched_mount(&check_info, map, devs, || fs::metadata(&path))?
            else {
                continue;
            };
            if metadata.dev() != check.dev() || metadata.ino() != check.ino() {
                continue;
            }
            if inventory.files.len() == MAX_OPEN_FILES {
                return Err(invalid());
            }
            inventory.push(OpenFile {
                pid,
                starttime: start,
                fd,
                target,
                dev: metadata.dev(),
                ino: metadata.ino(),
                nlink: metadata.nlink(),
                size: metadata.len(),
                disk: metadata.blocks().saturating_mul(512),
                flags,
            })?;
        }
        if read_starttime(pid).ok() != Some(start) {
            inventory.truncate(base);
        }
    }
    Ok(inventory.files)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn prefilter_never_stats_known_unwatched_devices_and_falls_back() {
        let map = [(5, libc::makedev(8, 1))];
        let devs = [libc::makedev(8, 2)];
        assert!(
            stat_on_watched_mount::<()>("mnt_id: 5", Some(&map), &devs, || panic!(
                "unwatched stat"
            ))
            .unwrap()
            .is_none()
        );
        for info in ["mnt_id: 6", "mnt_id: invalid", ""] {
            assert_eq!(
                stat_on_watched_mount(info, Some(&map), &devs, || Ok(42)).unwrap(),
                Some(42)
            );
        }
        assert_eq!(
            stat_on_watched_mount("mnt_id: 5", None, &devs, || Ok(42)).unwrap(),
            Some(42)
        );
        assert_eq!(
            stat_on_watched_mount("mnt_id: 5", Some(&map), &[map[0].1], || Ok(42)).unwrap(),
            Some(42)
        );
        assert!(stat_on_watched_mount::<()>("", None, &devs, || Err(
            io::Error::from_raw_os_error(libc::ENOTCONN)
        ))
        .unwrap()
        .is_none());
    }
    #[test]
    fn mount_parsers_and_missing_mapping() {
        assert_eq!(
            parse_mnt_id("pos: 0\nflags: 0100002\nmnt_id:\t123\n"),
            Some(123)
        );
        for text in [
            "",
            "mnt_id: -1",
            "mnt_id: nope",
            "mnt_id: 18446744073709551616",
        ] {
            assert_eq!(parse_mnt_id(text), None);
        }
        let device = libc::makedev(259, 65537);
        let line = r"123 1 259:65537 / /space\040here rw shared:4 master:2 - xfs /dev/test rw";
        assert_eq!(parse_mount_device(line), Some((123, device)));
        assert_eq!(
            parse_mount_device("12 1 8:1 / / rw - xfs /dev/a rw"),
            Some((12, libc::makedev(8, 1)))
        );
        for line in [
            "",
            "bad 1 8:1 / / rw - xfs a rw",
            "1 2 nope / / rw - xfs a rw",
            "1 2 8:1",
            "1 2 8:1 / / rw",
        ] {
            assert_eq!(parse_mount_device(line), None);
        }
        assert_eq!(mapped_device(&[(123, device)], 123), Some(device));
        assert_eq!(mapped_device(&[(123, device)], 999), None);
        // The same mount id in two namespaces must retain distinct devices.
        assert_ne!(
            mapped_device(&[(123, device)], 123),
            mapped_device(&[(123, 7)], 123)
        );
    }
    #[test]
    fn cache_growth_is_budgeted_and_mapping_errors_preserve_retry() {
        let mut inventory = Inventory {
            files: Vec::new(),
            paths: 0,
            scratch: 0,
            limit: 1,
        };
        let mut values: Vec<MountNamespace> = Vec::new();
        let error = reserve_cache(&mut values, &mut inventory).unwrap_err();
        assert!(error.get_ref().unwrap().is::<InventoryBudgetExceeded>());
        assert_eq!(values.capacity(), 0);
        inventory.limit = 4096;
        reserve_cache(&mut values, &mut inventory).unwrap();
        assert_eq!(
            inventory.bytes(),
            values.capacity() * std::mem::size_of::<MountNamespace>()
        );
        assert!(
            mapping_read::<()>(Err(io::Error::from_raw_os_error(libc::ENOENT)))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            mapping_read::<()>(Err(io::Error::from_raw_os_error(libc::EIO))).unwrap(),
            Some(None)
        );
        assert_eq!(
            mapping_read::<()>(Err(io::Error::from_raw_os_error(libc::EMFILE)))
                .unwrap_err()
                .raw_os_error(),
            Some(libc::EMFILE)
        );
    }
    #[test]
    fn injected_descriptor_errors_skip_or_abort() {
        for errno in [
            libc::ENOTCONN,
            libc::EIO,
            libc::ESTALE,
            libc::ENOENT,
            libc::EPERM,
        ] {
            assert!(per_fd::<()>(Err(io::Error::from_raw_os_error(errno)))
                .unwrap()
                .is_none());
        }
        for errno in [libc::ENOMEM, libc::EMFILE, libc::ENFILE] {
            assert_eq!(
                per_fd::<()>(Err(io::Error::from_raw_os_error(errno)))
                    .unwrap_err()
                    .raw_os_error(),
                Some(errno)
            );
        }
        assert_eq!(per_fd(Ok(42)).unwrap(), Some(42));
        assert!(!transient(&io::Error::from_raw_os_error(libc::EIO)));
    }
    #[test]
    #[allow(clippy::unwrap_used)]
    fn starttime_single_read_and_oversize_failure() {
        struct Counted<'a> {
            bytes: &'a [u8],
            calls: usize,
        }
        impl Read for Counted<'_> {
            fn read(&mut self, out: &mut [u8]) -> io::Result<usize> {
                self.calls += 1;
                self.bytes.read(out)
            }
        }
        let stat = format!("1 (worker) S {} 42 0\n", "0 ".repeat(18));
        let mut reader = Counted {
            bytes: stat.as_bytes(),
            calls: 0,
        };
        assert_eq!(read_starttime_from(&mut reader).unwrap(), 42);
        assert_eq!(reader.calls, 1);
        let oversized = [b' '; 512];
        let mut reader = Counted {
            bytes: &oversized,
            calls: 0,
        };
        assert!(read_starttime_from(&mut reader).is_err());
        assert_eq!(reader.calls, 1);
        assert!(read_starttime_from(&b"1 (incomplete) S 0"[..]).is_err());
    }
    #[test]
    fn uid_parser_and_error_policy() {
        assert_eq!(
            parse_uid(b"Name:\tworker\nUid:\t1000\t1001\t1002\t1003\n"),
            Some(1000)
        );
        assert!(parse_uid(include_bytes!("../tests/w1d/data/status.txt")).is_some());
        assert_eq!(
            parse_uid(b"Name:\tbad-\xff-name\nUid:\t1000\t1001\t1002\t1003\n"),
            Some(1000)
        );
        assert_eq!(parse_uid(b"Uid: -1"), None);
        assert_eq!(parse_uid(b"Uid: 4294967296"), None);
        assert_eq!(parse_uid(b"Uid:"), None);
        assert!(transient(&io::Error::from_raw_os_error(libc::ESRCH)));
        assert!(!transient(&io::Error::from_raw_os_error(libc::EIO)));
    }
}

#[cfg(test)]
#[path = "../tests/w1d/budget.rs"]
mod budget_tests;
