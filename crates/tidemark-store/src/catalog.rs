//! Owned by: W1-E
//! Cross-task signatures reserved for W1-E.

use crate::StoreError;
use std::path::{Path, PathBuf};
use tidemark_core::{path::RawPath, scan::ScanMode};
/// Reserved for W1-E.
pub fn root_slug(root: &RawPath) -> String {
    let _ = root;
    String::new()
}

/// Reserved for W1-E.
pub struct ScanEntry {
    #[doc = "Field specified by the owning task."]
    pub scan_id: String,
    #[doc = "Field specified by the owning task."]
    pub root: RawPath,
    #[doc = "Field specified by the owning task."]
    pub mode: ScanMode,
    #[doc = "Field specified by the owning task."]
    pub started_at: i64,
    #[doc = "Field specified by the owning task."]
    pub finished_at: Option<i64>,
    #[doc = "Field specified by the owning task."]
    pub status: String,
    #[doc = "Field specified by the owning task."]
    pub degraded: bool,
    #[doc = "Field specified by the owning task."]
    pub file: Option<PathBuf>,
    #[doc = "Field specified by the owning task."]
    pub t_disk: Option<u64>,
    #[doc = "Field specified by the owning task."]
    pub t_inodes: Option<u64>,
    #[doc = "Field specified by the owning task."]
    pub t_dirs: Option<u64>,
    #[doc = "Field specified by the owning task."]
    pub errors: Option<u64>,
    #[doc = "Field specified by the owning task."]
    pub duration_ms: Option<u64>,
}
/// Reserved for W1-E.
pub struct Catalog;
impl Catalog {
    /// Reserved for W1-E.
    pub fn open(state_dir: &Path, rw: bool) -> Result<Self, StoreError> {
        let _ = state_dir;
        let _ = rw;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn list(&self, root: Option<&RawPath>) -> Result<Vec<ScanEntry>, StoreError> {
        let _ = root;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn latest(&self, root: &RawPath) -> Result<Option<ScanEntry>, StoreError> {
        let _ = root;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn nearest_before(&self, root: &RawPath, ts: i64) -> Result<Option<ScanEntry>, StoreError> {
        let _ = root;
        let _ = ts;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn get(&self, scan_id: &str) -> Result<Option<ScanEntry>, StoreError> {
        let _ = scan_id;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn record_start(&self, e: &ScanEntry) -> Result<(), StoreError> {
        let _ = e;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn record_finish(&self, e: &ScanEntry) -> Result<(), StoreError> {
        let _ = e;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn rebuild_from_files(&self) -> Result<usize, StoreError> {
        Err(StoreError::NotYetImplemented)
    }
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        let raw = tidemark_core::path::RawPath::new(b"/".to_vec()).unwrap();
        assert!(root_slug(&raw).is_empty());
        assert_eq!(
            Catalog::open(std::path::Path::new("/"), false)
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Catalog).list(Some(&raw)).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Catalog).latest(&raw).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Catalog).nearest_before(&raw, 0).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Catalog).get("").err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Catalog)
                .record_start(&ScanEntry {
                    scan_id: String::new(),
                    root: raw.clone(),
                    mode: tidemark_core::scan::ScanMode::Walk,
                    started_at: 0,
                    finished_at: None,
                    status: String::new(),
                    degraded: false,
                    file: None,
                    t_disk: None,
                    t_inodes: None,
                    t_dirs: None,
                    errors: None,
                    duration_ms: None
                })
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Catalog)
                .record_finish(&ScanEntry {
                    scan_id: String::new(),
                    root: raw.clone(),
                    mode: tidemark_core::scan::ScanMode::Walk,
                    started_at: 0,
                    finished_at: None,
                    status: String::new(),
                    degraded: false,
                    file: None,
                    t_disk: None,
                    t_inodes: None,
                    t_dirs: None,
                    errors: None,
                    duration_ms: None
                })
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Catalog).rebuild_from_files().err().unwrap().to_string(),
            "NotYetImplemented"
        );
    }
}
