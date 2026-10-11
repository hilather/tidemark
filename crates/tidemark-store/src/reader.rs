//! Owned by: W1-E
//! Cross-task signatures reserved for W1-E.

use crate::StoreError;
use std::path::Path;
use tidemark_core::{
    path::RawPath,
    snapshot::{
        DirRow, ErrorRow, FileFilter, FileHit, LinkFilter, LinkInstanceRow, LinkSetHit, SkipRow,
        SnapshotMeta, SortKey,
    },
};
type Result<T> = std::result::Result<T, StoreError>;
/// Shape left to W1-E; metadata is retained for the infallible accessor.
pub struct Snapshot {
    meta: SnapshotMeta,
}
impl Snapshot {
    /// Reserved for W1-E.
    pub fn open(path: &Path) -> Result<Snapshot> {
        let _ = path;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn meta(&self) -> &SnapshotMeta {
        &self.meta
    }
    /// Reserved for W1-E.
    pub fn root(&self) -> Result<DirRow> {
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn dir(&self, id: u32) -> Result<DirRow> {
        let _ = id;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn dir_by_path(&self, path: &RawPath) -> Result<Option<DirRow>> {
        let _ = path;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn children(&self, id: u32, sort: SortKey, limit: usize) -> Result<Vec<DirRow>> {
        let _ = id;
        let _ = sort;
        let _ = limit;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn subtree_dirs(
        &self,
        id: u32,
        max_depth_rel: u32,
        sort: SortKey,
        limit: usize,
    ) -> Result<Vec<DirRow>> {
        let _ = id;
        let _ = max_depth_rel;
        let _ = sort;
        let _ = limit;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn subtree_files(
        &self,
        id: u32,
        filter: &FileFilter,
        limit: usize,
    ) -> Result<Vec<FileHit>> {
        let _ = id;
        let _ = filter;
        let _ = limit;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn wide_dirs(&self, id: u32, min_entries: u32, limit: usize) -> Result<Vec<DirRow>> {
        let _ = id;
        let _ = min_entries;
        let _ = limit;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn link_sets_under(
        &self,
        id: u32,
        filter: &LinkFilter,
        limit: usize,
    ) -> Result<Vec<LinkSetHit>> {
        let _ = id;
        let _ = filter;
        let _ = limit;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn link_instances(&self, ino: u64) -> Result<Vec<LinkInstanceRow>> {
        let _ = ino;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn skipped_under(&self, id: u32) -> Result<Vec<SkipRow>> {
        let _ = id;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn errors_under(&self, id: u32, limit: usize) -> Result<Vec<ErrorRow>> {
        let _ = id;
        let _ = limit;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn annotations(&self, id: u32) -> Result<Vec<(String, String)>> {
        let _ = id;
        Err(StoreError::NotYetImplemented)
    }
}

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        let raw = tidemark_core::path::RawPath::new(b"/".to_vec()).unwrap();
        assert_eq!(
            Snapshot::open(std::path::Path::new("/"))
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .meta(),
            &crate::test_meta()
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .root()
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .dir(0)
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .dir_by_path(&raw)
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .children(0, SortKey::Name, 0)
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .subtree_dirs(0, 0, SortKey::Name, 0)
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .subtree_files(0, &FileFilter::default(), 0)
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .wide_dirs(0, 0, 0)
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .link_sets_under(0, &LinkFilter::default(), 0)
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .link_instances(0)
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .skipped_under(0)
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .errors_under(0, 0)
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (Snapshot {
                meta: crate::test_meta()
            })
            .annotations(0)
            .err()
            .unwrap()
            .to_string(),
            "NotYetImplemented"
        );
    }
}
