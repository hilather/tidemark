//! Owned by: W6-A3
//! Shared live situation rendering at terminal width.
use std::io::Write;
/// Render the raw report as a machine document or the shared situation view.
pub fn write(
    report: &tidemark_core::live::NowReport,
    context: &crate::Context,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if context.json {
        return crate::render::json("tidemark.now.v1", report, out);
    }
    out.write_all(render_text(report, crate::render::table::terminal_width()).as_bytes())?;
    Ok(())
}
/// Render daemon deleted-open inventory using shared path escaping.
pub fn phantom(
    rows: &[tidemark_core::live::PhantomSummary],
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    phantom_width(rows, json, crate::render::table::terminal_width(), out)
}

/// Render using an injected terminal width.
pub fn phantom_width(
    rows: &[tidemark_core::live::PhantomSummary],
    json: bool,
    width: usize,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if json {
        return crate::render::json("tidemark.phantom.v1", rows, out);
    }
    let destination = out;
    let mut buffer = Vec::new();
    let out = &mut buffer;
    let mut rows: Vec<_> = rows.iter().collect();
    rows.sort_by(|a, b| a.mount.cmp(&b.mount));
    for r in rows {
        writeln!(
            out,
            "{}: {} bytes in {} deleted open files",
            tidemark_core::path::display_lossy_bytes(r.mount.as_bytes()),
            r.total_bytes,
            r.files
        )?;
        let mut holders: Vec<_> = r.holders.iter().collect();
        holders.sort_by(|a, b| (&a.path, a.pid).cmp(&(&b.path, b.pid)));
        for h in holders {
            writeln!(
                out,
                "  {} {} {} {} {} bytes",
                h.pid,
                tidemark_core::path::display_lossy_bytes(h.comm.as_bytes()),
                tidemark_core::path::display_lossy_bytes(
                    h.unit.as_deref().unwrap_or("?").as_bytes()
                ),
                tidemark_core::path::display_lossy_bytes(&h.path),
                h.bytes
            )?;
        }
    }
    destination
        .write_all(crate::render::table::wrap(&String::from_utf8(buffer)?, width).as_bytes())?;
    Ok(())
}
/// Render daemon open writers in descending growth order.
pub fn writers(
    rows: &[tidemark_core::live::OpenWriter],
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    writers_width(rows, json, crate::render::table::terminal_width(), out)
}

/// Render using an injected terminal width.
pub fn writers_width(
    rows: &[tidemark_core::live::OpenWriter],
    json: bool,
    width: usize,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if json {
        return crate::render::json("tidemark.writers.v1", rows, out);
    }
    let destination = out;
    let mut buffer = Vec::new();
    let out = &mut buffer;
    writeln!(out, "MOUNT  PID  COMMAND  UNIT  PATH  GROWTH  DISK")?;
    let mut rows: Vec<_> = rows.iter().collect();
    rows.sort_by(|a, b| {
        b.growth
            .cmp(&a.growth)
            .then((&a.path, a.pid).cmp(&(&b.path, b.pid)))
    });
    for r in rows {
        writeln!(
            out,
            "{} {} {} {} {} +{} bytes ({} bytes allocated)",
            tidemark_core::path::display_lossy_bytes(r.mount.as_bytes()),
            r.pid,
            tidemark_core::path::display_lossy_bytes(r.comm.as_bytes()),
            tidemark_core::path::display_lossy_bytes(r.unit.as_deref().unwrap_or("?").as_bytes()),
            tidemark_core::path::display_lossy_bytes(&r.path),
            r.growth,
            r.disk
        )?;
    }
    destination
        .write_all(crate::render::table::wrap(&String::from_utf8(buffer)?, width).as_bytes())?;
    Ok(())
}
/// Render only frozen LVM fields and separately read 24-hour trends.
pub fn lvm(
    report: &tidemark_core::api::LvmResult,
    trends: &[(String, Option<f64>, Option<f64>)],
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    lvm_width(
        report,
        trends,
        json,
        crate::render::table::terminal_width(),
        out,
    )
}

/// Render using an injected terminal width.
pub fn lvm_width(
    report: &tidemark_core::api::LvmResult,
    trends: &[(String, Option<f64>, Option<f64>)],
    json: bool,
    width: usize,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if json {
        return crate::render::json("tidemark.lvm.v1", report, out);
    }
    let destination = out;
    let mut buffer = Vec::new();
    let out = &mut buffer;
    writeln!(out, "View from daemon; segment types and other volumes are shown only when the daemon is not running.")?;
    writeln!(
        out,
        "MOUNT  VG/LV  SIZE  VG FREE  THIN POOL  DATA %  META %  DATA pp/day  META pp/day"
    )?;
    let mut rows: Vec<_> = report.mounts.iter().collect();
    rows.sort_by(|a, b| a.0.cmp(&b.0));
    for (m, r) in rows {
        let trend = trends.iter().find(|t| &t.0 == m);
        writeln!(
            out,
            "{}  {}/{}  {}  {}  {}  {:?}  {:?}  {:?}  {:?}",
            tidemark_core::path::display_lossy_bytes(m.as_bytes()),
            tidemark_core::path::display_lossy_bytes(r.vg.as_bytes()),
            tidemark_core::path::display_lossy_bytes(r.lv.as_bytes()),
            r.lv_size,
            r.vg_free,
            tidemark_core::path::display_lossy_bytes(
                r.thin_pool.as_deref().unwrap_or("?").as_bytes()
            ),
            r.data_pct,
            r.meta_pct,
            trend.and_then(|t| t.1),
            trend.and_then(|t| t.2)
        )?;
    }
    let mut vgs: Vec<_> = report.vgs.iter().collect();
    vgs.sort_by(|a, b| a.vg.cmp(&b.vg));
    for vg in vgs {
        writeln!(
            out,
            "VG {}: {} bytes, {} free",
            tidemark_core::path::display_lossy_bytes(vg.vg.as_bytes()),
            vg.size,
            vg.free
        )?;
    }
    destination
        .write_all(crate::render::table::wrap(&String::from_utf8(buffer)?, width).as_bytes())?;
    Ok(())
}

/// Emit the single direct-fallback notice even when routine logs are quiet.
pub fn fallback(command: &str) -> anyhow::Result<()> {
    writeln!(
        std::io::stderr().lock(),
        "daemon unavailable; falling back to direct {command} mode"
    )?;
    Ok(())
}

/// Render the report at an injected width without changing machine quantities.
pub fn render_text(r: &tidemark_core::live::NowReport, width: usize) -> String {
    use tidemark_core::units::{fmt_bytes, fmt_rate};
    let mut lines = vec![format!(
        "tidemark now - {} - {}",
        escape(&r.host),
        crate::render::size::timestamp(r.generated_at)
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
                    escape(e.top_unit.as_deref().unwrap_or("?")),
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
                escape(h.unit.as_deref().unwrap_or("?")),
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
                    crate::render::size::timestamp(l.finished_at),
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
    crate::render::table::wrap(&lines.join("\n"), width)
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
