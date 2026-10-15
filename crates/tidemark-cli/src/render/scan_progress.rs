//! Owned by: W2-C
//! Portable progress output and schema-tagged final scan summaries.
use std::io::{IsTerminal, Write};
use tidemark_scan::{
    orchestrator::ScanOutcome,
    progress::{Cadence, Progress},
};
/// Text renderer with a ten-second cadence for non-terminal logs.
pub struct Display {
    tty: bool,
    cadence: Cadence,
    wrote: bool,
}
impl Display {
    /// Detect stderr's terminal status once.
    pub fn stderr() -> Self {
        Self::new(std::io::stderr().is_terminal())
    }
    /// Construct with explicit terminal policy.
    pub fn new(tty: bool) -> Self {
        Self {
            tty,
            cadence: Cadence::new(std::time::Duration::from_secs(10)),
            wrote: false,
        }
    }
    /// Render into stderr, reporting I/O failure to the caller.
    pub fn update(&mut self, p: &Progress) -> std::io::Result<()> {
        self.write(p, &mut std::io::stderr().lock())
    }
    /// Render a report when due; terminals replace their current line.
    pub fn write(&mut self, p: &Progress, out: &mut impl Write) -> std::io::Result<()> {
        if !self.tty && !self.cadence.due(p.elapsed) {
            return Ok(());
        }
        let line = format_progress(p);
        if self.tty {
            write!(out, "\r\x1b[2K{line}")?;
        } else {
            writeln!(out, "{line}")?;
        }
        self.wrote = true;
        out.flush()
    }
    /// End the updating terminal line.
    pub fn finish(&mut self) -> std::io::Result<()> {
        if self.tty && self.wrote {
            writeln!(std::io::stderr().lock())?;
        }
        Ok(())
    }
}
/// Stable progress text with escaped values supplied by the scanner.
pub fn format_progress(p: &Progress) -> String {
    format!(
        "{}: {} {dirs}, {} {entries}, {} {errors}, {} {races}; rate {:.0} {rate_unit}/s; elapsed {:.0}s; ETA {}",
        p.phase,
        p.dirs,
        p.entries,
        p.errors,
        p.races,
        p.rate,
        p.elapsed.as_secs_f64(),
        p.eta
            .map(|d| format!("{:.0}s", d.as_secs_f64()))
            .unwrap_or_else(|| "unknown".into())
        ,dirs = super::unit(p.dirs,"dir","dirs"), entries = super::unit(p.entries,"entry","entries"), errors = super::unit(p.errors,"error","errors"), races = super::unit(p.races,"race","races"), rate_unit = if p.rate.round_ties_even() == 1.0 { "unit" } else { "units" }
    )
}
/// Write a final summary or a versioned machine document.
pub fn outcome(o: &ScanOutcome, json: bool, out: &mut impl Write) -> anyhow::Result<()> {
    if json {
        let file = o
            .file
            .as_ref()
            .map(|p| tidemark_core::path::RawPath::from_path(p))
            .transpose()?;
        crate::render::json(
            "tidemark.scan.v1",
            serde_json::json!({
                "scan_id": o.scan_id, "file": file.map(|p| p.to_json()), "status": o.status,
                "degraded": o.degraded, "mode": o.mode, "mode_reason": o.mode_reason,
                "duration_ms": u64::try_from(o.duration.as_millis()).unwrap_or(u64::MAX),
                "summary": { "dirs":o.summary.dirs,"files":o.summary.files,"inodes":o.summary.inodes,
                    "t_disk":o.summary.t_disk,"t_apparent":o.summary.t_apparent,"t_reclaimable":o.summary.t_reclaimable,
                    "link_sets":o.summary.link_sets,"incomplete_link_sets":o.summary.incomplete_link_sets,
                    "errors":o.summary.errors,"races":o.summary.races,"skipped":o.summary.skipped,"degraded":o.summary.degraded }
            }),
            out,
        )
    } else {
        writeln!(out,"Mode: {} ({})\nDuration: {:.2}s\nDirectories: {}  Files: {}\nDisk: {}  Reclaimable: {}\nLink sets: {} ({} incomplete)\nErrors: {}  Races: {}\nSnapshot: {}",
            super::enum_label(o.mode),o.mode_reason,o.duration.as_secs_f64(),o.summary.dirs,o.summary.files,
            crate::render::size::bytes(o.summary.t_disk),crate::render::size::bytes(o.summary.t_reclaimable),
            o.summary.link_sets,o.summary.incomplete_link_sets,o.summary.errors,o.summary.races,
            o.file.as_ref().map(|p| tidemark_core::path::RawPath::from_path(p).map(|p|p.display_lossy())).transpose()?.unwrap_or_else(|| "none".into()))?;
        Ok(())
    }
}
#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;
    #[test]
    fn log_cadence() {
        let mut d = Display::new(false);
        let mut buf = Vec::new();
        let mut p = Progress {
            phase: "walk",
            dirs: 1,
            entries: 2,
            errors: 0,
            races: 0,
            rate: 0.0,
            elapsed: std::time::Duration::from_secs(2),
            eta: None,
        };
        d.write(&p, &mut buf).unwrap();
        assert!(buf.is_empty());
        p.elapsed = std::time::Duration::from_secs(10);
        d.write(&p, &mut buf).unwrap();
        let size = buf.len();
        d.write(&p, &mut buf).unwrap();
        assert_eq!(buf.len(), size);
        insta::assert_snapshot!(String::from_utf8(buf).unwrap(), @"walk: 1 dir, 2 entries, 0 errors, 0 races; rate 0 units/s; elapsed 10s; ETA unknown");
    }
}
