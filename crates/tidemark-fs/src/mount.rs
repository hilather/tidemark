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
use std::io;
use tidemark_core::{path::RawPath, scan::DevId};
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
    pub root: RawPath,
    /// Mount target in the current namespace.
    pub mount_point: RawPath,
    /// Per-mount options.
    pub mount_opts: Vec<String>,
    /// Filesystem type.
    pub fs_type: String,
    /// Mount source; non-UTF8 absolute paths use escaped display.
    pub source: String,
    /// Filesystem superblock options.
    pub super_opts: Vec<String>,
}
/// Parse Linux mountinfo, preserving path bytes and rejecting malformed records.
pub fn parse_mountinfo(bytes: &[u8]) -> Result<Vec<MountInfo>, ParseError> {
    if bytes.len() > MAX_MOUNTINFO_BYTES {
        return Err(ParseError::Invalid("mountinfo exceeds 16 MiB"));
    }
    let mut mounts = Vec::new();
    for line in bytes.split(|b| *b == b'\n').filter(|l| !l.is_empty()) {
        let fields: Vec<_> = line.split(|b| *b == b' ').collect();
        let sep = fields
            .iter()
            .position(|f| *f == b"-")
            .ok_or(ParseError::Invalid("missing separator"))?;
        if sep < 6 || fields.len() != sep + 4 || fields.iter().any(|f| f.is_empty()) {
            return Err(ParseError::Invalid("invalid field count"));
        }
        let number = |b: &[u8]| {
            if b.is_empty() || !b.iter().all(u8::is_ascii_digit) {
                return Err(ParseError::Invalid("invalid number"));
            }
            std::str::from_utf8(b)
                .ok()
                .and_then(|s| s.parse::<u32>().ok())
                .ok_or(ParseError::Invalid("invalid number"))
        };
        let (major, minor) = fields[2]
            .split_once_byte(b':')
            .ok_or(ParseError::Invalid("invalid device"))?;
        let (major, minor) = (number(major)?, number(minor)?);
        let root_path = |b| {
            RawPath::new(decode(b)?).map_err(|_| ParseError::Invalid("root is not an absolute filesystem path (including nsfs object roots; SCR-001)"))
        };
        let mount_path =
            |b| RawPath::new(decode(b)?).map_err(|_| ParseError::Invalid("invalid mount target"));
        let text = |b| {
            String::from_utf8(decode(b)?).map_err(|_| ParseError::Invalid("non-UTF8 text field"))
        };
        mounts.push(MountInfo {
            id: number(fields[0])?,
            parent: number(fields[1])?,
            major,
            minor,
            dev: libc::makedev(major, minor),
            root: root_path(fields[3])?,
            mount_point: mount_path(fields[4])?,
            mount_opts: text(fields[5])?.split(',').map(str::to_owned).collect(),
            fs_type: text(fields[sep + 1])?,
            source: display_source(decode(fields[sep + 2])?)?,
            super_opts: text(fields[sep + 3])?
                .split(',')
                .map(str::to_owned)
                .collect(),
        });
    }
    Ok(mounts)
}
fn display_source(bytes: Vec<u8>) -> Result<String, ParseError> {
    match String::from_utf8(bytes) {
        Ok(source) => Ok(source),
        Err(e) => RawPath::new(e.into_bytes())
            .map(|path| path.display_lossy())
            .map_err(|_| ParseError::Invalid("non-UTF8 non-path mount source")),
    }
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
#[cfg(test)]
#[path = "../tests/w1a/mount.rs"]
mod tests;
