//! Owned by: W2-A
//! Cross-task interface stubs.

use tidemark_core::{
    path::{RawName, RawPath},
    scan::{DirId, DirRecord, GoneRecord, Ino, InodeAttrs, NsRecord},
    snapshot::{AnnotationRow, DirRow, ErrorRow, FileRow, LinkInstanceRow, LinkSetRow, SkipRow},
};
/// Reserved for W2-A.
pub struct AggConfig {
    /// Contract field.
    pub root: RawPath,
    /// Contract field.
    pub scan_start: i64,
    /// Contract field.
    pub top_files: usize,
    /// Contract field.
    pub big_file_threshold: u64,
    /// Contract field.
    pub max_notable: usize,
    /// Contract field.
    pub link_instances: bool,
    /// Contract field.
    pub max_stored_errors: usize,
}

/// Reserved for W2-A.
pub struct Aggregator;
impl Aggregator {
    /// Reserved for W2-A.
    pub fn new(cfg: AggConfig) -> Self {
        let _ = cfg;
        Self
    }

    /// Reserved for W2-A.
    pub fn add_dir_record(&mut self, rec: DirRecord) {
        let _ = rec;
    }
    /// Reserved for W2-A.
    pub fn add_gone(&mut self, g: &GoneRecord) {
        let _ = g;
    }

    /// Reserved for W2-A.
    pub fn add_skeleton(&mut self, ns: &NsRecord) {
        let _ = ns;
    }
    /// Reserved for W2-A.
    pub fn set_dir_inode(&mut self, dir: DirId, a: &InodeAttrs) {
        let _ = dir;
        let _ = a;
    }
    /// Reserved for W2-A.
    pub fn add_single(&mut self, dir: DirId, a: &InodeAttrs) {
        let _ = dir;
        let _ = a;
    }
    /// Reserved for W2-A.
    pub fn add_link_set(&mut self, a: &InodeAttrs, dirs: &[DirId]) {
        let _ = a;
        let _ = dirs;
    }
    /// Reserved for W2-A.
    pub fn add_races(&mut self, dir: Option<DirId>, n: u32) {
        let _ = dir;
        let _ = n;
    }

    /// Reserved for W2-A.
    pub fn prepare(self) -> Result<Prepared, AggError> {
        Err(AggError::NotYetImplemented)
    }
}

/// Reserved for W2-A.
pub struct Prepared;
impl Prepared {
    /// Reserved for W2-A.
    pub fn summary(&self) -> AggSummary {
        AggSummary::default()
    }

    /// Reserved for W2-A.
    pub fn needs_names(&self) -> Vec<(RawPath, Ino)> {
        Default::default()
    }
    /// Reserved for W2-A.
    pub fn set_names(&mut self, names: Vec<(RawPath, Ino, RawName)>) {
        let _ = names;
    }

    /// Reserved for W2-A.
    pub fn annotate(&mut self, path: &RawPath, key: &str, value: &str) {
        let _ = path;
        let _ = key;
        let _ = value;
    }

    /// Reserved for W2-A.
    pub fn write(self, sink: &mut dyn RowSink) -> Result<AggSummary, AggError> {
        let _ = sink;
        Err(AggError::NotYetImplemented)
    }
}

/// Reserved for W2-A.
pub trait RowSink {
    /// Reserved for W2-A.
    fn dirs(&mut self, rows: &[DirRow]) -> Result<(), SinkError>;
    /// Reserved for W2-A.
    fn files(&mut self, rows: &[FileRow]) -> Result<(), SinkError>;
    /// Reserved for W2-A.
    fn link_sets(&mut self, rows: &[LinkSetRow]) -> Result<(), SinkError>;
    /// Reserved for W2-A.
    fn link_instances(&mut self, rows: &[LinkInstanceRow]) -> Result<(), SinkError>;
    /// Reserved for W2-A.
    fn skipped(&mut self, rows: &[SkipRow]) -> Result<(), SinkError>;
    /// Reserved for W2-A.
    fn errors(&mut self, rows: &[ErrorRow]) -> Result<(), SinkError>;
    /// Reserved for W2-A.
    fn annotations(&mut self, rows: &[AnnotationRow]) -> Result<(), SinkError>;
}
#[derive(Default)]
/// Reserved for W2-A.
pub struct VecSink {
    /// Contract field.
    pub dirs: Vec<DirRow>,
    /// Contract field.
    pub files: Vec<FileRow>,
    /// Contract field.
    pub link_sets: Vec<LinkSetRow>,
    /// Contract field.
    pub link_instances: Vec<LinkInstanceRow>,
    /// Contract field.
    pub skipped: Vec<SkipRow>,
    /// Contract field.
    pub errors: Vec<ErrorRow>,
    /// Contract field.
    pub annotations: Vec<AnnotationRow>,
}

#[derive(Default)]
/// Reserved for W2-A.
pub struct AggSummary {
    /// Contract field.
    pub dirs: u64,
    /// Contract field.
    pub files: u64,
    /// Contract field.
    pub inodes: u64,
    /// Contract field.
    pub t_disk: u64,
    /// Contract field.
    pub t_apparent: u64,
    /// Contract field.
    pub t_reclaimable: u64,
    /// Contract field.
    pub link_sets: u64,
    /// Contract field.
    pub incomplete_link_sets: u64,
    /// Contract field.
    pub errors: u64,
    /// Contract field.
    pub races: u64,
    /// Contract field.
    pub skipped: u64,
    /// Contract field.
    pub degraded: bool,
}

mod lca;
mod linkset;
mod notable;
mod rollup;

/// Reserved error shape for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum AggError {
    /// Implementation is not available.
    #[error("NotYetImplemented")]
    NotYetImplemented,
}

/// Reserved error shape for the owning task.
#[derive(Debug, thiserror::Error)]
pub enum SinkError {
    /// Storage failure.
    #[error(transparent)]
    Store(#[from] tidemark_store::StoreError),
    /// Other sink failure.
    #[error("{0}")]
    Other(String),
}
impl RowSink for VecSink {
    fn dirs(&mut self, rows: &[DirRow]) -> Result<(), SinkError> {
        let _ = rows;
        Err(SinkError::Store(
            tidemark_store::StoreError::NotYetImplemented,
        ))
    }
    fn files(&mut self, rows: &[FileRow]) -> Result<(), SinkError> {
        let _ = rows;
        Err(SinkError::Store(
            tidemark_store::StoreError::NotYetImplemented,
        ))
    }
    fn link_sets(&mut self, rows: &[LinkSetRow]) -> Result<(), SinkError> {
        let _ = rows;
        Err(SinkError::Store(
            tidemark_store::StoreError::NotYetImplemented,
        ))
    }
    fn link_instances(&mut self, rows: &[LinkInstanceRow]) -> Result<(), SinkError> {
        let _ = rows;
        Err(SinkError::Store(
            tidemark_store::StoreError::NotYetImplemented,
        ))
    }
    fn skipped(&mut self, rows: &[SkipRow]) -> Result<(), SinkError> {
        let _ = rows;
        Err(SinkError::Store(
            tidemark_store::StoreError::NotYetImplemented,
        ))
    }
    fn errors(&mut self, rows: &[ErrorRow]) -> Result<(), SinkError> {
        let _ = rows;
        Err(SinkError::Store(
            tidemark_store::StoreError::NotYetImplemented,
        ))
    }
    fn annotations(&mut self, rows: &[AnnotationRow]) -> Result<(), SinkError> {
        let _ = rows;
        Err(SinkError::Store(
            tidemark_store::StoreError::NotYetImplemented,
        ))
    }
}
