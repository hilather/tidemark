//! Owned by: W4-A
//! LVM text and schema-wrapped JSON output.
use serde::Serialize;
use std::io::Write;
use tidemark_core::{api::VgInfo, live::LvmNow, path::display_lossy_bytes};
use tidemark_fs::lvm::LvInfo;
/// Current LVM context for tracked filesystems.
#[derive(Serialize)]
pub(crate) struct Report {
    /// ok or n/a when LVM is disabled, absent or unmapped.
    pub(crate) status: String,
    /// Unix sampling timestamp.
    pub(crate) sampled_at: i64,
    /// Tracked mounted logical volumes.
    pub(crate) tracked: Vec<TrackedLv>,
    /// Relevant volume groups.
    pub(crate) vgs: Vec<VgInfo>,
    /// Other volumes in those groups.
    pub(crate) other_lvs: Vec<LvInfo>,
}
/// A mapped mount and its current metadata.
#[derive(Serialize)]
pub(crate) struct TrackedLv {
    /// Byte-preserving path/path_b64 object.
    pub(crate) mount: serde_json::Value,
    /// Terminal-safe mount label.
    #[serde(skip)]
    pub(crate) mount_display: String,
    /// Current LV, VG and pool state.
    pub(crate) current: LvmNow,
    /// LVM segment type.
    pub(crate) segtype: String,
    /// Optional history-derived slopes.
    pub(crate) trend: Option<Trend>,
}
/// Least-squares slopes over available samples in the last 24 hours.
#[derive(Serialize)]
pub(crate) struct Trend {
    /// Pool data percentage points per day, requiring three samples.
    pub(crate) data_pp_per_day: Option<f64>,
    /// Pool metadata percentage points per day, requiring three samples.
    pub(crate) meta_pp_per_day: Option<f64>,
}
fn display(s: &str) -> String {
    display_lossy_bytes(s.as_bytes())
        .trim_start_matches('[')
        .trim_end_matches(']')
        .to_owned()
}
/// Render a report to the supplied writer.
pub(crate) fn write(report: &Report, json: bool, output: &mut impl Write) -> anyhow::Result<()> {
    write_width(report, json, super::table::terminal_width(), output)
}
/// Render the direct report using an injected width.
pub(crate) fn write_width(
    report: &Report,
    json: bool,
    width: usize,
    output: &mut impl Write,
) -> anyhow::Result<()> {
    if json {
        return super::json("tidemark.lvm.v1", report, output);
    }
    let destination = output;
    let mut buffer = Vec::new();
    let output = &mut buffer;
    if report.status == "n/a" {
        writeln!(output, "LVM: n/a")?;
        destination.write_all(super::table::wrap(&String::from_utf8(buffer)?, width).as_bytes())?;
        return Ok(());
    }
    for row in &report.tracked {
        let now = &row.current;
        writeln!(
            output,
            "{}: LV {}/{} size={} bytes segtype={}",
            super::table::shorten(&row.mount_display, width / 2),
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
    destination
        .write_all(crate::render::table::wrap(&String::from_utf8(buffer)?, width).as_bytes())?;
    Ok(())
}
/// Shared direct/daemon percentage cell; a dash means no measurement.
pub(super) fn pct(v: Option<f64>) -> String {
    v.map_or_else(|| "-".into(), |v| format!("{v:.2}%"))
}
/// Shared signed percentage-point slope cell; the surrounding view supplies units.
pub(super) fn rate(v: Option<f64>) -> String {
    v.map_or_else(|| "-".into(), |v| format!("{v:+.2}"))
}

#[cfg(test)]
mod tests {
    #[test]
    fn direct_report_fits_width_without_changing_json() {
        let report = super::Report {
            status: "ok".into(),
            sampled_at: 0,
            tracked: vec![super::TrackedLv {
                mount: serde_json::json!({"path":"/data"}),
                mount_display: format!("/begin/{}/last", "x".repeat(200)),
                current: tidemark_core::live::LvmNow {
                    vg: "v".repeat(200),
                    lv: "lv".into(),
                    lv_size: 1,
                    vg_free: 2,
                    thin_pool: None,
                    data_pct: None,
                    meta_pct: None,
                    sampled_at: 0,
                },
                segtype: "linear".into(),
                trend: None,
            }],
            vgs: vec![],
            other_lvs: vec![],
        };
        for width in [80, 100, 200] {
            let mut out = Vec::new();
            super::write_width(&report, false, width, &mut out).unwrap();
            let text = String::from_utf8(out).unwrap();
            assert!(text
                .lines()
                .all(|line| line.chars().count() <= width || line.split_whitespace().count() == 1));
            assert!(text.contains("/begin/") && text.contains("/last") && text.contains("..."));
            let mut out = Vec::new();
            super::write_width(&report, true, width, &mut out).unwrap();
            let value: serde_json::Value = serde_json::from_slice(&out).unwrap();
            assert_eq!(value["schema"], "tidemark.lvm.v1");
            assert_eq!(
                value["data"]["tracked"][0]["current"]["vg"],
                "v".repeat(200)
            );
            assert!(value["data"].get("mounts").is_none());
        }
    }
}
