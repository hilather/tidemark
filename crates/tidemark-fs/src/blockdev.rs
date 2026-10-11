//! Owned by: W1-A
//! Cross-task signatures reserved for W1-A.

use std::io;
use tidemark_core::{
    dev::{BlockDevInfo, DiskStat},
    scan::DevId,
};
/// Reserved for W1-A.
pub fn resolve_leaves(dev: DevId) -> io::Result<Vec<BlockDevInfo>> {
    let _ = dev;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}
/// Reserved for W1-A.
pub fn parse_disk_stat(line: &str) -> Option<DiskStat> {
    let _ = line;
    None
}
/// Reserved for W1-A.
pub fn read_disk_stats(devs: &[BlockDevInfo]) -> io::Result<Vec<DiskStat>> {
    let _ = devs;
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "NotYetImplemented",
    ))
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        assert_eq!(
            resolve_leaves(0).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
        assert!(parse_disk_stat("").is_none());
        assert_eq!(
            read_disk_stats(&[]).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
    }
}
