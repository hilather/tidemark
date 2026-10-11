//! Owned by: W1-E
//! Cross-task signatures reserved for W1-E.

use crate::StoreError;
use std::path::Path;
use tidemark_core::{
    config::SampleRetention,
    live::{AlertEvent, FsSample, HotMinute, LvmSample, PgSample, PhantomSample, WriterSample},
    path::RawPath,
    snapshot::DirRow,
};
/// Reserved for W1-E.
pub struct TimeSeries;
impl TimeSeries {
    /// Reserved for W1-E.
    pub fn open(path: &Path, rw: bool) -> Result<Self, StoreError> {
        let _ = path;
        let _ = rw;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn insert_fs(&self, s: &[FsSample]) -> Result<(), StoreError> {
        let _ = s;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn insert_lvm(&self, s: &[LvmSample]) -> Result<(), StoreError> {
        let _ = s;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn insert_dir_series(
        &self,
        root: &RawPath,
        scan_id: &str,
        ts: i64,
        rows: &[DirRow],
    ) -> Result<(), StoreError> {
        let _ = root;
        let _ = scan_id;
        let _ = ts;
        let _ = rows;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn insert_hot_minutes(&self, rows: &[HotMinute]) -> Result<(), StoreError> {
        let _ = rows;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn insert_phantom(&self, rows: &[PhantomSample]) -> Result<(), StoreError> {
        let _ = rows;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn insert_writers(&self, rows: &[WriterSample]) -> Result<(), StoreError> {
        let _ = rows;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn insert_pg(&self, s: &[PgSample]) -> Result<(), StoreError> {
        let _ = s;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn insert_alert(&self, a: &AlertEvent) -> Result<(), StoreError> {
        let _ = a;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn query_fs(&self, mount: &str, from: i64, to: i64) -> Result<Vec<FsSample>, StoreError> {
        let _ = mount;
        let _ = from;
        let _ = to;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn query_lvm(
        &self,
        vg: &str,
        lv: &str,
        from: i64,
        to: i64,
    ) -> Result<Vec<LvmSample>, StoreError> {
        let _ = vg;
        let _ = lv;
        let _ = from;
        let _ = to;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn query_pg(
        &self,
        kind: &str,
        name: Option<&str>,
        from: i64,
        to: i64,
    ) -> Result<Vec<PgSample>, StoreError> {
        let _ = kind;
        let _ = name;
        let _ = from;
        let _ = to;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn query_hot(
        &self,
        mount: &str,
        scope: &str,
        from: i64,
        to: i64,
        limit: usize,
    ) -> Result<Vec<HotMinute>, StoreError> {
        let _ = mount;
        let _ = scope;
        let _ = from;
        let _ = to;
        let _ = limit;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn query_dir_series(
        &self,
        root: &RawPath,
        path: &RawPath,
        from: i64,
        to: i64,
    ) -> Result<Vec<(i64, u64)>, StoreError> {
        let _ = root;
        let _ = path;
        let _ = from;
        let _ = to;
        Err(StoreError::NotYetImplemented)
    }
    /// Reserved for W1-E.
    pub fn compact(&self, now: i64, policy: &SampleRetention) -> Result<CompactStats, StoreError> {
        let _ = now;
        let _ = policy;
        Err(StoreError::NotYetImplemented)
    }
}

/// Shape left to W1-E.
pub struct CompactStats;

#[cfg(test)]
mod signature_tests {
    use super::*;
    #[test]
    fn cross_task_signatures_are_unavailable() {
        let raw = tidemark_core::path::RawPath::new(b"/".to_vec()).unwrap();
        assert_eq!(
            TimeSeries::open(std::path::Path::new("/"), false)
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries).insert_fs(&[]).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries).insert_lvm(&[]).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries)
                .insert_dir_series(&raw, "", 0, &[])
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries)
                .insert_hot_minutes(&[])
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries).insert_phantom(&[]).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries).insert_writers(&[]).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries).insert_pg(&[]).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries)
                .insert_alert(&AlertEvent {
                    ts: 0,
                    rule: String::new(),
                    subject: String::new(),
                    state: tidemark_core::live::AlertState::Resolved,
                    value: None,
                    message: String::new()
                })
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries).query_fs("", 0, 0).err().unwrap().to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries)
                .query_lvm("", "", 0, 0)
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries)
                .query_pg("", None, 0, 0)
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries)
                .query_hot("", "", 0, 0, 0)
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries)
                .query_dir_series(&raw, &raw, 0, 0)
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
        assert_eq!(
            (TimeSeries)
                .compact(0, &SampleRetention::default())
                .err()
                .unwrap()
                .to_string(),
            "NotYetImplemented"
        );
    }
}
