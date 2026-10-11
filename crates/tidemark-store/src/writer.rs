//! Owned by: W1-E
//! Cross-task signatures reserved for W1-E.

use crate::{space_guard::SpaceVerdict, StoreError};
use std::path::{Path, PathBuf};
use tidemark_core::snapshot::{
    AnnotationRow, DirRow, ErrorRow, FileRow, LinkInstanceRow, LinkSetRow, SkipRow, SnapshotMeta,
};
/// Reserved for W1-E.
pub struct SnapshotWriter;
impl SnapshotWriter {
    /// Reserved for W1-E.
    pub fn create(dir: &Path, meta: &SnapshotMeta) -> Result<Self, StoreError> {
        let _ = dir;
        let _ = meta;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn put_dirs(&mut self, rows: &[DirRow]) -> Result<(), StoreError> {
        let _ = rows;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn put_files(&mut self, rows: &[FileRow]) -> Result<(), StoreError> {
        let _ = rows;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn put_link_sets(&mut self, rows: &[LinkSetRow]) -> Result<(), StoreError> {
        let _ = rows;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn put_link_instances(&mut self, rows: &[LinkInstanceRow]) -> Result<(), StoreError> {
        let _ = rows;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn put_skipped(&mut self, rows: &[SkipRow]) -> Result<(), StoreError> {
        let _ = rows;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn put_errors(&mut self, rows: &[ErrorRow]) -> Result<(), StoreError> {
        let _ = rows;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn put_annotations(&mut self, rows: &[AnnotationRow]) -> Result<(), StoreError> {
        let _ = rows;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn space_check_every(&mut self, rows: u64, check: Box<dyn FnMut() -> SpaceVerdict + Send>) {
        let _ = rows;
        let _ = check;
    }
    /// Reserved for W1-E.
    pub fn finish(self, final_meta: &SnapshotMeta) -> Result<PathBuf, StoreError> {
        let _ = final_meta;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn abort(self) {}
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        assert_eq!(
            SnapshotWriter::create(std::path::Path::new("/"), &crate::test_meta())
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (SnapshotWriter).put_dirs(&[]).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (SnapshotWriter).put_files(&[]).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (SnapshotWriter)
                .put_link_sets(&[])
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (SnapshotWriter)
                .put_link_instances(&[])
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (SnapshotWriter).put_skipped(&[]).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (SnapshotWriter).put_errors(&[]).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (SnapshotWriter)
                .put_annotations(&[])
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        (SnapshotWriter).space_check_every(
            0,
            Box::new(|| SpaceVerdict::Refuse {
                reason: "NotYetImplemented".into(),
            }),
        );
        assert_eq!(
            (SnapshotWriter)
                .finish(&crate::test_meta())
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        (SnapshotWriter).abort();
    }
}
