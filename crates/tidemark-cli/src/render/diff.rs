//! Owned by: W3-A
//! Escaped text and byte-preserving JSON snapshot comparisons.
use std::io::Write;
use tidemark_core::{path::RawPath, units::fmt_bytes};
use tidemark_store::diff::{DiffClass, DiffRow, Explanation};
fn signed(delta: i64, files: bool) -> String {
    let amount = if files {
        delta.unsigned_abs().to_string()
    } else {
        fmt_bytes(delta.unsigned_abs())
    };
    format!("{}{amount}", if delta < 0 { "−" } else { "+" })
}
fn class(class: DiffClass) -> (&'static str, &'static str) {
    match class {
        DiffClass::Grown => ("grown", "+"),
        DiffClass::Shrunk => ("shrunk", "−"),
        DiffClass::New => ("new", "★"),
        DiffClass::Removed => ("removed", "✗"),
        DiffClass::Same => ("same", "="),
    }
}
fn row_json(row: &DiffRow) -> serde_json::Value {
    let mut value = row.path.to_json();
    if let Some(map) = value.as_object_mut() {
        map.insert("depth_rel".into(), row.depth_rel.into());
        map.insert("new".into(), serde_json::json!(row.new));
        map.insert("old".into(), serde_json::json!(row.old));
        map.insert("delta".into(), row.delta.into());
        map.insert("class".into(), class(row.class).0.into());
    }
    value
}
/// Describe both catalog inputs with an injected observation time.
/// Call before the text comparison; JSON keeps its existing payload shape.
pub fn inputs_header(
    old: &tidemark_store::catalog::ScanEntry,
    new: &tidemark_store::catalog::ScanEntry,
    now: i64,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    for (label, input) in [("From", old), ("To", new)] {
        writeln!(
            out,
            "{label}: Snapshot {} · {:?} · {}s ago · {}{}",
            input.scan_id,
            input.mode,
            now.saturating_sub(input.started_at).max(0),
            input.status,
            if input.degraded { " (degraded)" } else { "" }
        )?;
    }
    Ok(())
}
/// Warn when either explicitly selected input has incomplete coverage.
pub fn coverage_notice(
    old: &tidemark_store::catalog::ScanEntry,
    new: &tidemark_store::catalog::ScanEntry,
) -> Option<String> {
    let affected: Vec<_> = [("from", old), ("to", new)]
        .into_iter()
        .filter(|(_, input)| input.status == "partial" || input.degraded)
        .map(|(label, input)| {
            format!(
                "{label} snapshot {} is {}{}",
                input.scan_id,
                input.status,
                if input.degraded { " (degraded)" } else { "" }
            )
        })
        .collect();
    (!affected.is_empty()).then(|| {
        format!(
            "WARNING: {}; deltas may reflect incomplete coverage.",
            affected.join("; ")
        )
    })
}
/// Render a comparison with optional descent and snapshot-age notice.
pub fn report(
    target: &RawPath,
    rows: &[DiffRow],
    explanation: Option<&Explanation>,
    files: bool,
    notice: Option<&str>,
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    if json {
        let explanation = explanation.map(|e| serde_json::json!({
            "target_delta":e.target_delta,"own_delta":e.own_delta,"spread_children":e.spread_children,
            "steps":e.steps.iter().map(|s| { let mut v=s.path.to_json(); if let Some(m)=v.as_object_mut() { m.insert("delta".into(),s.delta.into()); m.insert("share_of_parent".into(),serde_json::json!(s.share_of_parent)); } v }).collect::<Vec<_>>(),
            "spread":e.spread.iter().map(row_json).collect::<Vec<_>>()
        }));
        return super::json(
            "tidemark.diff.v1",
            serde_json::json!({"diff":rows.iter().map(row_json).collect::<Vec<_>>(),"explanation":explanation,"notice":notice}),
            out,
        );
    }
    if let Some(notice) = notice {
        if notice.starts_with("WARNING:") {
            writeln!(out, "{notice}")?;
        } else {
            writeln!(out, "Notice: {notice}")?;
        }
    }
    if rows
        .iter()
        .any(|r| r.path == *target && matches!(r.class, DiffClass::Removed))
    {
        writeln!(out, "Target removed: {}", target.display_lossy())?;
    }
    writeln!(out, "CLASS  DELTA  PATH")?;
    for row in rows {
        // Bound indentation independently of potentially adversarial stored depths.
        writeln!(
            out,
            "{}  {}  {}{}",
            class(row.class).1,
            signed(row.delta, files),
            "  ".repeat(row.depth_rel.min(64) as usize),
            row.path.display_lossy()
        )?;
    }
    if let Some(e) = explanation {
        writeln!(
            out,
            "Explanation: {} {}",
            target.display_lossy(),
            signed(e.target_delta, files)
        )?;
        for (depth, step) in e.steps.iter().enumerate() {
            writeln!(
                out,
                "{}└─ {} {} ({:.0}%)",
                "  ".repeat(depth.min(64)),
                step.path.display_lossy(),
                signed(step.delta, files),
                step.share_of_parent * 100.0
            )?;
        }
        writeln!(
            out,
            "Change is spread across {} subdirectories; top {}:",
            e.spread_children,
            e.spread.len()
        )?;
        for row in &e.spread {
            writeln!(
                out,
                "  {} {}",
                row.path.display_lossy(),
                signed(row.delta, files)
            )?;
        }
        writeln!(out, "Own charged bytes: {}", signed(e.own_delta, false))?;
    }
    Ok(())
}
