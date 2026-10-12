//! Owned by: W6-A3
//! Daemon health, error and scan schedule rendering.
use std::io::Write;
use tidemark_core::{api::StatusResult, live::ScanStatus, path::display_lossy_bytes as display};
/// Render daemon health with separately queried scan schedules. Show-config
/// uses the CLI's validated effective config; the wire has no config getter.
pub fn write(
    report: &StatusResult,
    scans: &[ScanStatus],
    context: &crate::Context,
    show_config: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    write_width(
        report,
        scans,
        context,
        show_config,
        crate::render::table::terminal_width(),
        out,
    )
}

/// Render using an injected terminal width.
pub fn write_width(
    report: &StatusResult,
    scans: &[ScanStatus],
    context: &crate::Context,
    show_config: bool,
    width: usize,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if context.json {
        return crate::render::json("tidemark.status.v1", report, out);
    }
    let destination = out;
    let mut buffer = Vec::new();
    let out = &mut buffer;
    writeln!(
        out,
        "tidemarkd {} · uptime {}s",
        report.version, report.uptime_s
    )?;
    let mut rows: Vec<_> = report.components.iter().collect();
    rows.sort_by(|a, b| a.name.cmp(&b.name));
    let mut table = vec![["COMPONENT", "STATE", "DETAIL"].map(str::to_owned).to_vec()];
    for r in rows {
        table.push(vec![
            display(r.name.as_bytes()),
            display(r.state.as_bytes()),
            display(r.detail.as_bytes()),
        ]);
    }
    out.write_all(crate::render::table::aligned_width(&table, width).as_bytes())?;
    let mut modes = report.watch_modes.clone();
    modes.sort();
    for (mount, mode) in modes {
        writeln!(
            out,
            "watch {}: {}",
            display(mount.as_bytes()),
            display(mode.as_bytes())
        )?;
    }
    let mut errors = report.last_errors.clone();
    errors.sort();
    for (name, ts, message) in errors {
        writeln!(
            out,
            "last error {} at {}: {}",
            display(name.as_bytes()),
            ts,
            display(message.as_bytes())
        )?;
    }
    writeln!(out, "ROOT  RUNNING  NEXT RUN  LAST COMPLETE")?;
    let mut scans: Vec<_> = scans.iter().collect();
    scans.sort_by(|a, b| a.root.cmp(&b.root));
    for s in scans {
        writeln!(
            out,
            "{}  {}  {:?}  {:?}",
            display(s.root.as_bytes()),
            s.running.is_some(),
            s.next_due,
            s.last_complete.as_ref().map(|s| s.finished_at)
        )?;
    }
    if show_config {
        writeln!(
            out,
            "CLI effective config (daemon config may differ after reload):"
        )?;
        serde_json::to_writer_pretty(&mut *out, &context.config)?;
        writeln!(out)?;
    }
    destination
        .write_all(crate::render::table::wrap(&String::from_utf8(buffer)?, width).as_bytes())?;
    Ok(())
}

/// Render the completed daemon scan status without manufacturing scan metadata.
pub fn scan_complete(status: &ScanStatus, json: bool, out: &mut impl Write) -> anyhow::Result<()> {
    if json {
        return crate::render::json("tidemark.scan.v1", status, out);
    }
    if let Some(last) = &status.last_complete {
        writeln!(
            out,
            "scan {} completed: {} · {} · {}ms",
            display(last.scan_id.as_bytes()),
            display(status.root.as_bytes()),
            display(last.mode.as_bytes()),
            last.duration_ms
        )?;
    }
    Ok(())
}
/// Render the daemon cancellation acknowledgement.
pub fn scan_cancel(
    result: &tidemark_core::api::ScanCancelResult,
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if json {
        return crate::render::json("tidemark.scan.v1", result, out);
    }
    writeln!(out, "scan cancellation requested: {}", result.cancelled)?;
    Ok(())
}
