//! Owned by: W5-D
//! Assemble core live reports and render deterministic terminal text.

use crate::rates::MountRates;
use tidemark_core::live::{
    AlertSummary, HotEntry, LvmNow, NowReport, OpenWriter, PhantomSummary, ScanStatus,
};

/// Borrowed collector inputs for an injected-time report.
/// None is not measured; Some(empty) is a complete, measured empty section.
/// Unavailable sections and alert diagnostics survive in NowReport.degraded.
pub struct NowInputs<'a> {
    /// Contract field.
    pub host: &'a str,
    /// Contract field.
    pub mounts: Option<&'a [MountRates]>,
    /// Contract field.
    pub lvm: Option<&'a [(String, LvmNow)]>,
    /// Contract field.
    pub hot_dirs_5m: Option<&'a [(String, Vec<HotEntry>)]>,
    /// Contract field.
    pub writers_5m: Option<&'a [(String, Vec<HotEntry>)]>,
    /// Contract field.
    pub phantom: Option<&'a [PhantomSummary]>,
    /// Contract field.
    pub open_writers: Option<&'a [(String, Vec<OpenWriter>)]>,
    /// Contract field.
    pub alerts: Option<&'a [AlertSummary]>,
    /// Contract field.
    pub scans: Option<&'a [ScanStatus]>,
    /// Contract field.
    pub watch_modes: Option<&'a [(String, String, u32)]>,
    /// Contract field.
    pub unevaluable: &'a [crate::alerts::AlertDiagnostic],
    /// Explicit collector degradation.
    pub degraded: &'a [String],
}
/// Join collector facts by mount and preserve explicit degradation.
pub fn build_now(i: &NowInputs, now: i64) -> NowReport {
    let mounts = i.mounts.unwrap_or(&[]);
    let lvm = i.lvm.unwrap_or(&[]);
    let hot_dirs_5m = i.hot_dirs_5m.unwrap_or(&[]);
    let writers_5m = i.writers_5m.unwrap_or(&[]);
    let phantom = i.phantom.unwrap_or(&[]);
    let open_writers = i.open_writers.unwrap_or(&[]);
    let alerts = i.alerts.unwrap_or(&[]);
    let scans = i.scans.unwrap_or(&[]);
    let watch_modes = i.watch_modes.unwrap_or(&[]);
    use tidemark_core::live::MountNow;
    let mut mounts: Vec<_> = mounts
        .iter()
        .map(|m| {
            let watch = watch_modes.iter().find(|(name, _, _)| name == &m.mount);
            MountNow {
                mount: m.mount.clone(),
                size: m.size,
                used: m.used,
                avail: m.avail,
                rate_15m: m.rate_15m,
                rate_1h: m.rate_1h,
                ttf_secs: m.ttf.map(|d| d.as_secs()),
                lvm: lvm
                    .iter()
                    .find(|(name, _)| name == &m.mount)
                    .map(|(_, v)| v.clone()),
                hot_dirs_5m: entries(hot_dirs_5m, &m.mount),
                writers_5m: entries(writers_5m, &m.mount),
                phantom: phantom
                    .iter()
                    .find(|p| p.mount == m.mount)
                    .cloned()
                    .unwrap_or(PhantomSummary {
                        mount: m.mount.clone(),
                        total_bytes: 0,
                        files: 0,
                        holders: vec![],
                    }),
                open_writers_top: open_writers
                    .iter()
                    .find(|(name, _)| name == &m.mount)
                    .map(|(_, v)| v.clone())
                    .unwrap_or_default(),
                watch_mode: watch
                    .map(|(_, mode, _)| mode.clone())
                    .unwrap_or_else(|| "unavailable".into()),
                lossy_minutes_15m: watch.map(|(_, _, lossy)| *lossy).unwrap_or(0),
            }
        })
        .collect();
    mounts.sort_by(|a, b| a.mount.as_bytes().cmp(b.mount.as_bytes()));
    let mut degraded = i.degraded.to_vec();
    for (section, available) in [
        ("mounts", i.mounts.is_some()),
        ("lvm", i.lvm.is_some()),
        ("hot_dirs_5m", i.hot_dirs_5m.is_some()),
        ("writers_5m", i.writers_5m.is_some()),
        ("phantom", i.phantom.is_some()),
        ("open_writers", i.open_writers.is_some()),
        ("alerts", i.alerts.is_some()),
        ("scans", i.scans.is_some()),
        ("watch_modes", i.watch_modes.is_some()),
    ] {
        if !available {
            degraded.push(format!("{section}: not measured"));
        }
    }
    for d in i.unevaluable {
        degraded.push(format!("alert {}: not evaluable: {}", d.rule, d.reason));
    }
    for m in &mounts {
        if i.phantom.is_none() {
            degraded.push(format!("phantom {}: not measured", m.mount));
        }

        if m.watch_mode == "unavailable" {
            degraded.push(format!("watcher {}: unavailable", m.mount));
        } else if m.watch_mode.starts_with("fd") {
            degraded.push(format!("watcher {}: fd mode only", m.mount));
        }
        if m.lossy_minutes_15m > 0 {
            degraded.push(format!(
                "watcher {}: {} lossy minutes (15m)",
                m.mount, m.lossy_minutes_15m
            ));
        }
    }
    degraded.sort();
    degraded.dedup();
    let mut scans = scans.to_vec();
    scans.sort_by(|a, b| a.root.cmp(&b.root));
    let mut alerts = alerts.to_vec();
    alerts.sort_by(|a, b| (&a.rule, &a.subject).cmp(&(&b.rule, &b.subject)));
    NowReport {
        generated_at: now,
        host: i.host.into(),
        mounts,
        alerts_firing: alerts,
        scans,
        degraded,
    }
}
fn entries(source: &[(String, Vec<HotEntry>)], mount: &str) -> Vec<HotEntry> {
    let mut v = source
        .iter()
        .find(|(name, _)| name == mount)
        .map(|(_, v)| v.clone())
        .unwrap_or_default();
    v.sort_by(|a, b| {
        b.bytes_delta
            .cmp(&a.bytes_delta)
            .then_with(|| a.key.cmp(&b.key))
    });
    v
}
/// Render a deterministic, width-wrapped text view. Color is optional and currently plain.
pub fn render_now_text(r: &NowReport, width: usize, color: bool) -> String {
    use tidemark_core::units::{fmt_bytes, fmt_rate};
    let _ = color;
    let mut lines = vec![format!(
        "tidemark now - {} - {}",
        escape(&r.host),
        timestamp(r.generated_at)
    )];
    let missing = |section: &str| {
        r.degraded
            .iter()
            .any(|d| d == &format!("{section}: not measured"))
    };
    for m in &r.mounts {
        let pct = if m.size == 0 {
            "unknown".into()
        } else {
            format!("{:.1}%", 100.0 * m.used as f64 / m.size as f64)
        };
        let trend = match (m.ttf_secs, m.rate_1h) {
            (Some(t), _) => format!(
                "full in ~{} (1h trend)",
                tidemark_core::time::fmt_duration(std::time::Duration::from_secs(t))
            ),
            (_, Some(v)) if v <= 0.0 => "not filling".into(),
            _ => "trend unavailable".into(),
        };
        lines.push(format!(
            "{} used {} / {} ({})  {} (15m)  {}",
            escape(&m.mount),
            fmt_bytes(m.used),
            fmt_bytes(m.size),
            pct,
            m.rate_15m
                .map(|r| fmt_rate(r * 3600.0).replace("/s", "/h"))
                .unwrap_or_else(|| "unknown".into()),
            trend
        ));
        lines.push(match &m.lvm {
            None if missing("lvm") => "  lvm not measured".into(),
            None => "  lvm n/a".into(),
            Some(l) => format!(
                "  lvm {}/{} {}  VG free {}{}",
                escape(&l.vg),
                escape(&l.lv),
                fmt_bytes(l.lv_size),
                fmt_bytes(l.vg_free),
                l.thin_pool
                    .as_ref()
                    .map(|p| format!(
                        "  thin pool {} data {} meta {}",
                        escape(p),
                        l.data_pct
                            .map(|p| format!("{p:.1}%"))
                            .unwrap_or_else(|| "unknown".into()),
                        l.meta_pct
                            .map(|p| format!("{p:.1}%"))
                            .unwrap_or_else(|| "unknown".into())
                    ))
                    .unwrap_or_default()
            ),
        });
        for (title, list) in [
            ("hot dirs (5m)", &m.hot_dirs_5m),
            ("writers (5m)", &m.writers_5m),
        ] {
            for e in list {
                lines.push(format!(
                    "  {} {}  {}  {} files  {}{}",
                    title,
                    bytes_display(&e.key),
                    signed_bytes(e.bytes_delta),
                    e.files,
                    escape(e.top_unit.as_deref().unwrap_or("unknown")),
                    if e.lossy { " [lossy]" } else { "" }
                ));
            }
        }
        if missing("phantom") {
            lines.push("  deleted-but-open not measured".into());
        } else {
            lines.push(format!(
                "  deleted-but-open {} ({} files)",
                fmt_bytes(m.phantom.total_bytes),
                m.phantom.files
            ));
        }
        for h in &m.phantom.holders {
            lines.push(format!(
                "    {} (pid {}, {}) {} {}",
                escape(&h.comm),
                h.pid,
                escape(h.unit.as_deref().unwrap_or("unknown")),
                bytes_display(&h.path),
                fmt_bytes(h.bytes)
            ));
        }
        for w in &m.open_writers_top {
            lines.push(format!(
                "  open writer {} (pid {}) {} {} B / {}s",
                escape(&w.comm),
                w.pid,
                bytes_display(&w.path),
                w.growth,
                w.window_secs
            ));
        }
    }
    for s in &r.scans {
        lines.push(format!(
            "last scan {} {}",
            escape(&s.root),
            s.last_complete
                .as_ref()
                .map(|l| format!(
                    "{} ({}s ago, {}, {}ms)",
                    l.finished_at,
                    r.generated_at.saturating_sub(l.finished_at).max(0),
                    escape(&l.mode),
                    l.duration_ms
                ))
                .unwrap_or_else(|| "none".into())
        ));
        if let Some(p) = &s.running {
            lines.push(format!(
                "  running {}: {} dirs, {} entries",
                escape(&p.phase),
                p.dirs,
                p.entries
            ));
        }
    }
    for a in &r.alerts_firing {
        lines.push(format!(
            "ALERT {} {}: {}",
            escape(&a.rule),
            escape(&a.subject),
            escape(&a.message)
        ));
    }
    for d in &r.degraded {
        lines.push(format!("degraded: {}", escape(d)));
    }
    let mut out = String::new();
    for line in lines {
        let mut count = 0;
        for ch in line.chars() {
            if count == width.max(1) {
                out.push('\n');
                count = 0;
            }
            out.push(ch);
            count += 1;
        }
        out.push('\n');
    }
    out
}
fn escape(s: &str) -> String {
    tidemark_core::path::display_lossy_bytes(s.as_bytes())
}
fn bytes_display(b: &[u8]) -> String {
    tidemark_core::path::display_lossy_bytes(b)
}

fn signed_bytes(bytes: i64) -> String {
    format!(
        "{}{}",
        if bytes < 0 { "-" } else { "+" },
        tidemark_core::units::fmt_bytes(bytes.unsigned_abs())
    )
}

// Civil date arithmetic in 400-year Gregorian eras, with Euclidean division
// so dates before the Unix epoch have the same behavior as later dates.
fn timestamp(ts: i64) -> String {
    let z = ts.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    let second = ts.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02} UTC",
        second / 3600,
        (second / 60) % 60,
        second % 60
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn utc_dates() {
        assert_eq!(timestamp(0), "1970-01-01 00:00:00 UTC");
        assert_eq!(timestamp(-1), "1969-12-31 23:59:59 UTC");
        assert_eq!(timestamp(951_782_400), "2000-02-29 00:00:00 UTC");
        assert_eq!(timestamp(4_107_542_400), "2100-03-01 00:00:00 UTC");
        assert!(!timestamp(i64::MAX).is_empty());
        assert!(!timestamp(i64::MIN).is_empty());
    }
}
