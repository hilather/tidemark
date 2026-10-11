//! Owned by: W3-C
//! Wide-directory tables and schema-wrapped byte-safe JSON.
use std::io::Write;
use tidemark_core::snapshot::DirRow;
/// Explain wide-directory costs and who can address their layout.
pub const HINT: &str = "Very wide directories increase directory btree depth and ls sort cost; ask the app owner to shard the directory layout.";
/// Render direct entry counts, subtree disk usage and canonical paths.
pub fn directories(rows: &[DirRow], json: bool, out: &mut impl Write) -> anyhow::Result<()> {
    if json {
        let rows: Vec<_> = rows
            .iter()
            .map(|r| {
                let mut value = r.path.to_json();
                if let Some(obj) = value.as_object_mut() {
                    obj.insert("entries".into(), r.entries.into());
                    obj.insert("t_disk".into(), r.t_disk.into());
                }
                value
            })
            .collect();
        crate::render::json(
            "tidemark.wide.v1",
            serde_json::json!({"directories":rows,"hint":HINT}),
            out,
        )?;
    } else {
        writeln!(out, "ENTRIES  DISK  PATH")?;
        for row in rows {
            writeln!(
                out,
                "{}  {}  {}",
                row.entries,
                crate::render::size::bytes(row.t_disk),
                row.path.display_lossy()
            )?;
        }
        writeln!(out, "{HINT}")?;
    }
    Ok(())
}
