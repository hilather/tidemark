//! Owned by: W1-A
//! Byte-preserving mount namespace discovery.

/// Errors encountered while parsing the kernel mount table.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// Compatibility variant retained from Wave 0; this parser never returns it.
    #[error("NotYetImplemented")]
    NotYetImplemented,
    /// Malformed or oversized mount table.
    #[error("invalid mountinfo: {0}")]
    Invalid(&'static str),
}
use std::{
    io,
    os::fd::BorrowedFd,
    sync::atomic::{AtomicBool, Ordering},
};
pub(crate) use tidemark_core::path::display_lossy_bytes as display_field;
use tidemark_core::{
    path::RawPath,
    scan::{DevId, Gate},
};
/// Filesystem path or kernel object used as a mount root.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MountRoot {
    /// Root within a filesystem.
    Path(RawPath),
    /// Decoded kernel object identifier, preserved exactly.
    Object(Vec<u8>),
}
/// One Linux mountinfo record; paths preserve decoded bytes.
pub struct MountInfo {
    /// Kernel mount identifier.
    pub id: u32,
    /// Parent mount identifier.
    pub parent: u32,
    /// Device major number.
    pub major: u32,
    /// Device minor number.
    pub minor: u32,
    /// Linux encoded device identifier (makedev).
    pub dev: DevId,
    /// Root of this mount within its filesystem.
    pub root: MountRoot,
    /// Mount target in the current namespace.
    pub mount_point: RawPath,
    /// Per-mount options, escaped for display.
    pub mount_opts: Vec<String>,
    /// Filesystem type.
    pub fs_type: String,
    /// Mount source with controls, backslashes and invalid UTF-8 escaped for display.
    pub source: String,
    /// Filesystem superblock options, escaped for display.
    pub super_opts: Vec<String>,
}
/// Parse Linux mountinfo, preserving bytes and warning for each malformed line.
/// Undecodable boundaries and oversized tables are rejected; malformed later fields are empty.
pub fn parse_mountinfo(bytes: &[u8]) -> Result<Vec<MountInfo>, ParseError> {
    if bytes.len() > MAX_MOUNTINFO_BYTES {
        return Err(ParseError::Invalid("mountinfo exceeds 16 MiB"));
    }
    let mut mounts = Vec::new();
    for (index, line) in bytes.split(|b| *b == b'\n').enumerate() {
        if line.is_empty() {
            continue;
        }
        let fields: Vec<_> = line.split(|b| *b == b' ').collect();
        // Boundary fields must be trustworthy even when the rest is malformed.
        if fields.len() < 5 {
            return Err(ParseError::Invalid("missing boundary fields"));
        }
        let mount_point = RawPath::new(decode(fields[4])?)
            .map_err(|_| ParseError::Invalid("invalid mount target"))?;
        match parse_record(line) {
            Ok(mount) => mounts.push(mount),
            Err(error) => {
                let id = parse_number(fields[0])?;
                let parent = parse_number(fields[1])?;
                let (major, minor) = fields[2]
                    .split_once_byte(b':')
                    .ok_or(ParseError::Invalid("invalid device"))?;
                let (major, minor) = (parse_number(major)?, parse_number(minor)?);
                tracing::warn!(line = index + 1, %error, "retaining malformed mountinfo boundary");
                mounts.push(MountInfo {
                    id,
                    parent,
                    major,
                    minor,
                    dev: libc::makedev(major, minor),
                    root: MountRoot::Object(Vec::new()),
                    mount_point,
                    mount_opts: Vec::new(),
                    fs_type: String::new(),
                    source: String::new(),
                    super_opts: Vec::new(),
                });
            }
        }
    }
    Ok(mounts)
}
fn parse_number(b: &[u8]) -> Result<u32, ParseError> {
    if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
        return Err(ParseError::Invalid("invalid number"));
    }
    std::str::from_utf8(b)
        .ok()
        .and_then(|s| s.parse().ok())
        .ok_or(ParseError::Invalid("invalid number"))
}
fn parse_record(line: &[u8]) -> Result<MountInfo, ParseError> {
    let fields: Vec<_> = line.split(|b| *b == b' ').collect();
    let sep = fields
        .iter()
        .position(|f| *f == b"-")
        .ok_or(ParseError::Invalid("missing separator"))?;
    if sep < 6 || fields.len() != sep + 4 || fields.iter().any(|f| f.is_empty()) {
        return Err(ParseError::Invalid("invalid field count"));
    }
    let number = parse_number;
    let (major, minor) = fields[2]
        .split_once_byte(b':')
        .ok_or(ParseError::Invalid("invalid device"))?;
    let (major, minor) = (number(major)?, number(minor)?);
    let root_path = |b| {
        let bytes = decode(b)?;
        if bytes.starts_with(b"/") {
            RawPath::new(bytes)
                .map(MountRoot::Path)
                .map_err(|_| ParseError::Invalid("invalid filesystem root"))
        } else if bytes.contains(&0) {
            Err(ParseError::Invalid("NUL in object root"))
        } else {
            Ok(MountRoot::Object(bytes))
        }
    };
    let mount_path =
        |b| RawPath::new(decode(b)?).map_err(|_| ParseError::Invalid("invalid mount target"));
    let text =
        |b| String::from_utf8(decode(b)?).map_err(|_| ParseError::Invalid("non-UTF8 text field"));
    Ok(MountInfo {
        id: number(fields[0])?,
        parent: number(fields[1])?,
        major,
        minor,
        dev: libc::makedev(major, minor),
        root: root_path(fields[3])?,
        mount_point: mount_path(fields[4])?,
        mount_opts: display_field(&decode(fields[5])?)
            .split(',')
            .map(str::to_owned)
            .collect(),
        fs_type: text(fields[sep + 1])?,
        source: display_field(&decode(fields[sep + 2])?),
        super_opts: display_field(&decode(fields[sep + 3])?)
            .split(',')
            .map(str::to_owned)
            .collect(),
    })
}
// Linux's generated table is bounded to avoid allocating from arbitrary input.
const MAX_MOUNTINFO_BYTES: usize = 16 * 1024 * 1024;
trait SplitByte {
    fn split_once_byte(&self, byte: u8) -> Option<(&[u8], &[u8])>;
}
impl SplitByte for [u8] {
    fn split_once_byte(&self, byte: u8) -> Option<(&[u8], &[u8])> {
        let i = self.iter().position(|b| *b == byte)?;
        Some((&self[..i], &self[i + 1..]))
    }
}
fn decode(bytes: &[u8]) -> Result<Vec<u8>, ParseError> {
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'\\' {
            let digits = bytes
                .get(i + 1..i + 4)
                .ok_or(ParseError::Invalid("truncated escape"))?;
            if !digits.iter().all(|b| (b'0'..=b'7').contains(b)) {
                return Err(ParseError::Invalid("invalid escape"));
            }
            let value = (digits[0] - b'0') as u16 * 64
                + (digits[1] - b'0') as u16 * 8
                + (digits[2] - b'0') as u16;
            out.push(u8::try_from(value).map_err(|_| ParseError::Invalid("escape overflow"))?);
            i += 4;
        } else {
            out.push(bytes[i]);
            i += 1;
        }
    }
    Ok(out)
}
/// Read the current process's mount namespace table with a 16 MiB limit.
pub fn read_mountinfo() -> io::Result<Vec<MountInfo>> {
    use std::io::Read;
    let mut bytes = Vec::new();
    std::fs::File::open("/proc/self/mountinfo")?
        .take(MAX_MOUNTINFO_BYTES as u64 + 1)
        .read_to_end(&mut bytes)?;
    parse_mountinfo(&bytes).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}
/// Find the deepest component prefix; later overmounts win ties.
pub fn mount_for_path<'a>(m: &'a [MountInfo], path: &RawPath) -> Option<&'a MountInfo> {
    m.iter()
        .filter(|mount| path.starts_with(&mount.mount_point))
        .max_by_key(|mount| mount.mount_point.as_path().as_os_str().len())
}
/// Return sorted, deduplicated mount targets strictly below a root.
pub fn submounts_under(m: &[MountInfo], root: &RawPath) -> Vec<RawPath> {
    let mut paths: Vec<_> = m
        .iter()
        .filter(|mount| mount.mount_point != *root && mount.mount_point.starts_with(root))
        .map(|mount| mount.mount_point.clone())
        .collect();
    paths.sort();
    paths.dedup();
    paths
}
/// Return other mount targets of a device, in deterministic path order.
pub fn other_mounts_of_dev<'a>(
    m: &'a [MountInfo],
    dev: DevId,
    except: &RawPath,
) -> Vec<&'a MountInfo> {
    let mut mounts: Vec<_> = m
        .iter()
        .filter(|mount| mount.dev == dev && mount.mount_point != *except)
        .collect();
    mounts.sort_by(|a, b| a.mount_point.cmp(&b.mount_point).then(a.id.cmp(&b.id)));
    mounts
}
/// Whether an opened scan root is still on the mountinfo record chosen for it.
///
/// The descriptor's mount ID (statx `STATX_MNT_ID`, else `/proc/self/fdinfo`)
/// must equal the record's ID when known. The device must also match, except on
/// btrfs, where `stat` reports a per-subvolume device that mountinfo does not
/// show; there a known mount ID and a btrfs descriptor are required instead.
/// Without a mount ID only the device comparison is available. Each metadata
/// or proc read is gated, and cancellation is checked before issuing it.
pub fn descriptor_on_mount(
    fd: BorrowedFd<'_>,
    st_dev: DevId,
    mount: &MountInfo,
    gate: &dyn Gate,
    cancel: &AtomicBool,
) -> io::Result<bool> {
    let mount_id = mount_id_with(gate, cancel, || statx_mount_id(fd), || fdinfo_mount_id(fd))?;
    identity_matches(mount, st_dev, mount_id, || {
        charge(gate, cancel)?;
        Ok(is_btrfs(fd))
    })
}
// Mirrors walk::getdents::charge: acquire, then refuse I/O once cancelled.
fn charge(gate: &dyn Gate, cancel: &AtomicBool) -> io::Result<()> {
    gate.acquire(1);
    if cancel.load(Ordering::Acquire) {
        Err(io::Error::new(io::ErrorKind::Interrupted, "scan cancelled"))
    } else {
        Ok(())
    }
}
fn mount_id_with(
    gate: &dyn Gate,
    cancel: &AtomicBool,
    statx: impl FnOnce() -> Option<u64>,
    fdinfo: impl FnOnce() -> Option<u64>,
) -> io::Result<Option<u64>> {
    charge(gate, cancel)?;
    if let Some(id) = statx() {
        return Ok(Some(id));
    }
    charge(gate, cancel)?;
    Ok(fdinfo())
}
fn identity_matches(
    mount: &MountInfo,
    st_dev: DevId,
    mount_id: Option<u64>,
    descriptor_btrfs: impl FnOnce() -> io::Result<bool>,
) -> io::Result<bool> {
    if st_dev == mount.dev {
        return Ok(mount_id.is_none_or(|id| id == u64::from(mount.id)));
    }
    // btrfs subvolumes have their own anonymous st_dev; mountinfo shows the
    // filesystem's. Accept only a known matching mount ID on a btrfs descriptor.
    if mount.fs_type != "btrfs" || mount_id != Some(u64::from(mount.id)) {
        return Ok(false);
    }
    descriptor_btrfs()
}
fn statx_mount_id(fd: BorrowedFd<'_>) -> Option<u64> {
    use rustix::fs::{statx, AtFlags, StatxFlags};
    let stat = statx(
        fd,
        "",
        AtFlags::EMPTY_PATH | AtFlags::SYMLINK_NOFOLLOW,
        StatxFlags::MNT_ID,
    )
    .ok()?;
    (stat.stx_mask & StatxFlags::MNT_ID.bits() != 0).then_some(stat.stx_mnt_id)
}
// EL8 statx supplies no mount ID; fdinfo has carried it since Linux 3.15.
fn fdinfo_mount_id(fd: BorrowedFd<'_>) -> Option<u64> {
    use rustix::fs::{Mode, OFlags};
    use std::{io::Read, os::fd::AsRawFd};
    let info = std::fs::File::from(
        rustix::fs::open(
            format!("/proc/self/fdinfo/{}", fd.as_raw_fd()),
            OFlags::RDONLY | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::empty(),
        )
        .ok()?,
    );
    let mut text = String::new();
    info.take(4096).read_to_string(&mut text).ok()?;
    parse_fdinfo_mount_id(&text)
}
fn parse_fdinfo_mount_id(text: &str) -> Option<u64> {
    text.lines()
        .find_map(|line| line.strip_prefix("mnt_id:"))
        .and_then(|id| id.trim().parse().ok())
}
const BTRFS_SUPER_MAGIC: u64 = 0x9123_683E;
fn is_btrfs(fd: BorrowedFd<'_>) -> bool {
    rustix::fs::fstatfs(fd).is_ok_and(|s| s.f_type as u64 == BTRFS_SUPER_MAGIC)
}
#[cfg(test)]
#[path = "../tests/w1a/mount.rs"]
mod tests;
