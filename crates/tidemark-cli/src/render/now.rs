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
    // Coordinator must enable rustix termios for TIOCGWINSZ; use the frozen fallback.
    let width = 100;
    out.write_all(tidemark_live::now::render_now_text(report, width, context.color).as_bytes())?;
    Ok(())
}
/// Render daemon deleted-open inventory using shared path escaping.
pub fn phantom(
    rows: &[tidemark_core::live::PhantomSummary],
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if json {
        return crate::render::json("tidemark.phantom.v1", rows, out);
    }
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
    Ok(())
}
/// Render daemon open writers in descending growth order.
pub fn writers(
    rows: &[tidemark_core::live::OpenWriter],
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if json {
        return crate::render::json("tidemark.writers.v1", rows, out);
    }
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
    Ok(())
}
/// Render only frozen LVM fields and separately read 24-hour trends.
pub fn lvm(
    report: &tidemark_core::api::LvmResult,
    trends: &[(String, Option<f64>, Option<f64>)],
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if json {
        return crate::render::json("tidemark.lvm.v1", report, out);
    }
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
