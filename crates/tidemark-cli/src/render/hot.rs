//! Owned by: W6-A3
//! Deterministic hot activity table.
use std::io::Write;
/// Render activity ordered by descending growth and raw key bytes.
pub fn write(
    rows: &[tidemark_core::live::HotEntry],
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    let mut rows: Vec<_> = rows.iter().collect();
    rows.sort_by(|a, b| b.bytes_delta.cmp(&a.bytes_delta).then(a.key.cmp(&b.key)));
    if json {
        return crate::render::json("tidemark.hot.v1", rows, out);
    }
    let mut table = vec![
        ["KEY", "EVENTS", "FILES", "DELTA BYTES", "TOP UNIT", "LOSSY"]
            .map(str::to_owned)
            .to_vec(),
    ];
    for r in rows {
        table.push(vec![
            tidemark_core::path::display_lossy_bytes(&r.key),
            r.events.to_string(),
            r.files.to_string(),
            format!("{:+}", r.bytes_delta),
            tidemark_core::path::display_lossy_bytes(
                r.top_unit.as_deref().unwrap_or("?").as_bytes(),
            ),
            r.lossy.to_string(),
        ]);
    }
    out.write_all(crate::render::table::aligned(&table).as_bytes())?;
    Ok(())
}
