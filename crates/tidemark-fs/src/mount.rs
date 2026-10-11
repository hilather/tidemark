//! Owned by: W1-A
//! Cross-task signatures reserved for W1-A.

/// Stub error reserved for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum ParseError {
    /// Cross-task implementation is not available yet.
    #[error("NotYetImplemented")]
    NotYetImplemented,
}
use std::io;
use tidemark_core::{path::RawPath, scan::DevId};
/// Reserved for W1-A.
pub struct MountInfo {
    #[doc = "Field specified by the owning task."]
    pub id: u32,
    #[doc = "Field specified by the owning task."]
    pub parent: u32,
    #[doc = "Field specified by the owning task."]
    pub major: u32,
    #[doc = "Field specified by the owning task."]
    pub minor: u32,
    #[doc = "Field specified by the owning task."]
    pub dev: DevId,
    #[doc = "Field specified by the owning task."]
    pub root: RawPath,
    #[doc = "Field specified by the owning task."]
    pub mount_point: RawPath,
    #[doc = "Field specified by the owning task."]
    pub mount_opts: Vec<String>,
    #[doc = "Field specified by the owning task."]
    pub fs_type: String,
    #[doc = "Field specified by the owning task."]
    pub source: String,
    #[doc = "Field specified by the owning task."]
    pub super_opts: Vec<String>,
}
/// Reserved for W1-A.
pub fn parse_mountinfo(bytes: &[u8]) -> Result<Vec<MountInfo>, ParseError> {
    let _ = bytes;
    Err(ParseError::NotYetImplemented)
}
/// Reserved for W1-A.
pub fn read_mountinfo() -> io::Result<Vec<MountInfo>> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-A.
pub fn mount_for_path<'a>(m: &'a [MountInfo], path: &RawPath) -> Option<&'a MountInfo> {
    let _ = m;
    let _ = path;
    None
}
/// Reserved for W1-A.
pub fn submounts_under(m: &[MountInfo], root: &RawPath) -> Vec<RawPath> {
    let _ = m;
    let _ = root;
    Vec::new()
}
/// Reserved for W1-A.
pub fn other_mounts_of_dev<'a>(
    m: &'a [MountInfo],
    dev: DevId,
    except: &RawPath,
) -> Vec<&'a MountInfo> {
    let _ = m;
    let _ = dev;
    let _ = except;
    Vec::new()
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        let raw = tidemark_core::path::RawPath::new(b"/".to_vec()).unwrap();
        assert_eq!(
            parse_mountinfo(b"").err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            read_mountinfo().err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert!(mount_for_path(&[], &raw).is_none());
        assert!(submounts_under(&[], &raw).is_empty());
        assert!(other_mounts_of_dev(&[], 0, &raw).is_empty());
    }
}
