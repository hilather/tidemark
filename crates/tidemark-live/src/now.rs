//! Owned by: W5-D
//! Cross-task interface stubs.

use crate::rates::MountRates;
use tidemark_core::live::{
    AlertSummary, HotEntry, LvmNow, NowReport, OpenWriter, PgNow, PhantomSummary, ScanStatus,
};

/// Reserved for W5-D.
pub struct NowInputs<'a> {
    /// Contract field.
    pub host: &'a str,
    /// Contract field.
    pub mounts: &'a [MountRates],
    /// Contract field.
    pub lvm: &'a [(String, LvmNow)],
    /// Contract field.
    pub hot_dirs_5m: &'a [(String, Vec<HotEntry>)],
    /// Contract field.
    pub writers_5m: &'a [(String, Vec<HotEntry>)],
    /// Contract field.
    pub phantom: &'a [PhantomSummary],
    /// Contract field.
    pub open_writers: &'a [(String, Vec<OpenWriter>)],
    /// Contract field.
    pub pg: Option<&'a PgNow>,
    /// Contract field.
    pub alerts: &'a [AlertSummary],
    /// Contract field.
    pub scans: &'a [ScanStatus],
    /// Contract field.
    pub watch_modes: &'a [(String, String, u32)],
    /// Contract field.
    pub degraded: &'a [String],
}
/// Reserved for W5-D.
pub fn build_now(i: &NowInputs, now: i64) -> NowReport {
    let _ = i;
    let _ = now;
    NowReport {
        generated_at: 0,
        host: String::new(),
        mounts: Vec::new(),
        pg: None,
        alerts_firing: Vec::new(),
        scans: Vec::new(),
        degraded: Vec::new(),
    }
}
/// Reserved for W5-D.
pub fn render_now_text(r: &NowReport, width: usize, color: bool) -> String {
    let _ = r;
    let _ = width;
    let _ = color;
    Default::default()
}
