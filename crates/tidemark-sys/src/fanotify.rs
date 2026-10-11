//! Owned by: W5-A
//! Cross-task signatures reserved for W5-A.

use std::{io, path::Path};
/// Reserved for W5-A.
pub struct FanotifyCaps {
    #[doc = "Field specified by the owning task."]
    pub available: bool,
    #[doc = "Field specified by the owning task."]
    pub fd_mode: bool,
    #[doc = "Field specified by the owning task."]
    pub report_fid: bool,
    #[doc = "Field specified by the owning task."]
    pub report_dfid_name: bool,
    #[doc = "Field specified by the owning task."]
    pub mark_filesystem: bool,
    #[doc = "Field specified by the owning task."]
    pub error: Option<String>,
}
/// Reserved for W5-A.
pub fn probe(mount: &Path) -> io::Result<FanotifyCaps> {
    let _ = mount;
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
            probe(std::path::Path::new("/")).err().unwrap().kind(),
            std::io::ErrorKind::Unsupported
        );
    }
}
