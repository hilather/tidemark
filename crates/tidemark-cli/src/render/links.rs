//! Owned by: W3-B
//! Hard-link topology sections and byte-preserving JSON output.
use std::io::Write;
use tidemark_core::path::RawPath;
use tidemark_store::links::{LinkReport, Regions};
fn region(
    r: Option<tidemark_store::links::RegionId>,
    regions: Option<&Regions>,
) -> serde_json::Value {
    r.and_then(|id| regions.map(|rs| rs.name(id).to_json()))
        .unwrap_or(serde_json::Value::Null)
}
/// Render totals, crossings, pinned directories, largest sets and shared directories.
pub fn report(
    report: &LinkReport,
    metadata: Option<(&tidemark_core::snapshot::SnapshotMeta, i64)>,
    regions: Option<&Regions>,
    paths: &[(u64, RawPath, &str)],
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let t = &report.totals;
    if json {
        let crossing:Vec<_>=report.crossing.iter().map(|s|serde_json::json!({"ino":s.ino,"disk":s.disk,"nlink":s.nlink,"found":s.found,"regions":s.regions.iter().map(|&r|region(r,regions)).collect::<Vec<_>>(),"sample_dirs":s.sample_dirs})).collect();
        let pairs:Vec<_>=report.region_pairs.iter().map(|p|serde_json::json!({"a":p.a.as_ref().map(RawPath::to_json),"b":p.b.as_ref().map(RawPath::to_json),"sets":p.sets,"disk":p.disk})).collect();
        let largest:Vec<_>=report.largest_sets.iter().map(|s|serde_json::json!({"ino":s.row.ino,"disk":s.row.disk,"found":s.row.found,"nlink":s.row.nlink,"complete":s.row.complete,"lca":s.lca_path.to_json(),"stored_instances":s.instance_count})).collect();
        let pinned: Vec<_> = report
            .incomplete_top_dirs
            .iter()
            .map(|(p, b, s)| serde_json::json!({"directory":p.to_json(),"disk":b,"sets":s}))
            .collect();
        let shared: Vec<_> = report
            .most_shared_dirs
            .iter()
            .map(|(p, b, d)| serde_json::json!({"directory":p.to_json(),"shared":b,"t_disk":d}))
            .collect();
        let paths:Vec<_>=paths.iter().map(|(ino,p,status)|serde_json::json!({"ino":ino,"instance":p.to_json(),"status":status})).collect();
        crate::render::json(
            "tidemark.links.v1",
            serde_json::json!({"snapshot":metadata.map(|(m,now)|serde_json::json!({"scan_id":m.scan_id,"mode":m.mode,"age_s":now.saturating_sub(m.finished_at).max(0),"degraded":m.degraded})),"totals":{"sets":t.sets,"instances":t.instances,"disk":t.disk,"incomplete_sets":t.incomplete_sets,"incomplete_disk":t.incomplete_disk,"crossing_sets":t.crossing.sets,"crossing_disk":t.crossing.disk},"crossing":crossing,"region_pairs":pairs,"incomplete_top_dirs":pinned,"largest_sets":largest,"most_shared_dirs":shared,"paths":paths}),
            out,
        )?;
    } else {
        writeln!(
            out,
            "Link sets: {}, instances: {}, bytes: {}",
            t.sets, t.instances, t.disk
        )?;
        writeln!(
            out,
            "Incomplete: {} sets, {} pinned bytes",
            t.incomplete_sets, t.incomplete_disk
        )?;
        if regions.is_some() {
            writeln!(
                out,
                "Crossing: {} sets, {} bytes",
                t.crossing.sets, t.crossing.disk
            )?;
            for s in &report.crossing {
                let names: Vec<_> = s
                    .regions
                    .iter()
                    .map(|&id| {
                        id.and_then(|id| regions.map(|r| r.name(id).display_lossy()))
                            .unwrap_or_else(|| "(outside regions)".into())
                    })
                    .collect();
                writeln!(
                    out,
                    "inode {}: {} bytes, {}/{} links, {}{}",
                    s.ino,
                    s.disk,
                    s.found,
                    s.nlink,
                    names.join(" ↔ "),
                    if s.found < s.nlink {
                        " (incomplete: links outside scan)"
                    } else {
                        ""
                    }
                )?;
            }
            for p in &report.region_pairs {
                writeln!(
                    out,
                    "{} ↔ {}: {} sets, {} bytes",
                    p.a.as_ref()
                        .map(RawPath::display_lossy)
                        .unwrap_or_else(|| "(outside regions)".into()),
                    p.b.as_ref()
                        .map(RawPath::display_lossy)
                        .unwrap_or_else(|| "(outside regions)".into()),
                    p.sets,
                    p.disk
                )?;
            }
        }
        writeln!(out, "Pinned directories:")?;
        for (p, b, s) in &report.incomplete_top_dirs {
            writeln!(out, "{b} bytes, {s} sets  {}", p.display_lossy())?;
        }
        writeln!(out, "Largest link sets:")?;
        for s in &report.largest_sets {
            writeln!(
                out,
                "inode {}: {} bytes, {}/{} links, LCA {}",
                s.row.ino,
                s.row.disk,
                s.row.found,
                s.row.nlink,
                s.lca_path.display_lossy()
            )?;
        }
        writeln!(out, "Most shared directories:")?;
        for (p, b, d) in &report.most_shared_dirs {
            writeln!(out, "{b} shared / {d} bytes  {}", p.display_lossy())?;
        }
        for (ino, p, status) in paths {
            writeln!(out, "inode {ino}: {} ({status})", p.display_lossy())?;
        }
    }
    Ok(())
}
