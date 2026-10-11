//! Owned by: W4-A
//! LVM text and schema-wrapped JSON output.
use serde::Serialize;
use std::io::Write;
use tidemark_core::{api::VgInfo, live::LvmNow, path::display_lossy_bytes};
use tidemark_fs::lvm::LvInfo;
/// Current LVM context for tracked filesystems.
#[derive(Serialize)]
pub struct Report {
    /// ok or n/a when LVM is disabled, absent or unmapped.
    pub status: String,
    /// Unix sampling timestamp.
    pub sampled_at: i64,
    /// Tracked mounted logical volumes.
    pub tracked: Vec<TrackedLv>,
    /// Relevant volume groups.
    pub vgs: Vec<VgInfo>,
    /// Other volumes in those groups.
    pub other_lvs: Vec<LvInfo>,
}
/// A mapped mount and its current metadata.
#[derive(Serialize)]
pub struct TrackedLv {
    /// Byte-preserving path/path_b64 object.
    pub mount: serde_json::Value,
    /// Terminal-safe mount label.
    #[serde(skip)]
    pub mount_display: String,
    /// Current LV, VG and pool state.
    pub current: LvmNow,
    /// LVM segment type.
    pub segtype: String,
    /// Optional history-derived slopes.
    pub trend: Option<Trend>,
}
/// Least-squares slopes over available samples in the last 24 hours.
#[derive(Serialize)]
pub struct Trend {
    /// Pool data percentage points per day, requiring three samples.
    pub data_pp_per_day: Option<f64>,
    /// Pool metadata percentage points per day, requiring three samples.
    pub meta_pp_per_day: Option<f64>,
}
fn display(s: &str) -> String {
    display_lossy_bytes(s.as_bytes())
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned()
}
/// Render a report to the supplied writer.
pub fn write(report: &Report, json: bool, output: &mut impl Write) -> anyhow::Result<()> {
    if json {
        return super::json("tidemark.lvm.v1", report, output);
    }
    if report.status == "n/a" {
        writeln!(output, "LVM: n/a")?;
        return Ok(());
    }
    for row in &report.tracked {
        let now = &row.current;
        writeln!(
            output,
            "{}: LV {}/{} size={} bytes segtype={}",
            row.mount_display,
            display(&now.vg),
            display(&now.lv),
            now.lv_size,
            display(&row.segtype)
        )?;
        if let Some(pool) = &now.thin_pool {
            writeln!(
                output,
                "  pool {} data={} metadata={}",
                display(pool),
                pct(now.data_pct),
                pct(now.meta_pct)
            )?;
            if let Some(trend) = &row.trend {
                writeln!(
                    output,
                    "  24h trend: data={} metadata={} (percentage points/day)",
                    rate(trend.data_pp_per_day),
                    rate(trend.meta_pp_per_day)
                )?;
            } else {
                writeln!(output, "  24h trend: n/a")?;
            }
        }
    }
    for vg in &report.vgs {
        writeln!(
            output,
            "VG {} size={} bytes free={} bytes (headroom for lvextend + xfs_growfs)",
            display(&vg.vg),
            vg.size,
            vg.free
        )?;
    }
    for lv in &report.other_lvs {
        writeln!(
            output,
            "  other LV {}/{} size={} bytes segtype={}",
            display(&lv.vg),
            display(&lv.lv),
            lv.size,
            display(&lv.segtype)
        )?;
    }
    Ok(())
}
fn pct(v: Option<f64>) -> String {
    v.map_or_else(|| "n/a".into(), |v| format!("{v:.2}%"))
}
fn rate(v: Option<f64>) -> String {
    v.map_or_else(|| "n/a".into(), |v| format!("{v:+.2}"))
}
