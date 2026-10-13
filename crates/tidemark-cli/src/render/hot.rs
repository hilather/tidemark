//! Owned by: W6-A3
//! Deterministic hot activity table.
use std::io::Write;
/// Render activity ordered by descending growth and raw key bytes.
pub fn write(
    rows: &[tidemark_core::live::HotEntry],
    json: bool,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    write_width(rows, json, crate::render::table::terminal_width(), out)
}

/// Render using an injected terminal width.
pub fn write_width(
    rows: &[tidemark_core::live::HotEntry],
    json: bool,
    width: usize,
    out: &mut impl Write,
) -> anyhow::Result<()> {
    write_grouped(rows, json, width, false, out)
}

/// Render activity, omitting the redundant unit column when grouped by unit.
pub fn write_grouped(
    rows: &[tidemark_core::live::HotEntry],
    json: bool,
    width: usize,
    by_unit: bool,
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
            crate::render::now::signed_bytes(r.bytes_delta),
            tidemark_core::path::display_lossy_bytes(
                r.top_unit.as_deref().unwrap_or("?").as_bytes(),
            ),
            r.lossy.to_string(),
        ]);
    }
    if by_unit {
        for row in &mut table {
            row.remove(4);
        }
    }
    out.write_all(
        crate::render::table::aligned_wrapped_right(
            &table,
            width,
            if by_unit { &[0] } else { &[0, 4] },
            &[1, 2, 3],
        )
        .as_bytes(),
    )?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn signed_sizes_align_and_json_remains_integer() {
        let rows: Vec<_> = [8_388_608, -1024, 0, i64::MIN]
            .into_iter()
            .enumerate()
            .map(|(i, bytes_delta)| tidemark_core::live::HotEntry {
                key: format!("unit-{i}").into_bytes(),
                events: 1,
                files: 1,
                bytes_delta,
                top_unit: None,
                lossy: false,
            })
            .collect();
        for width in [80, 100] {
            let mut out = Vec::new();
            super::write_width(&rows[..3], false, width, &mut out).unwrap();
            let text = String::from_utf8(out).unwrap();
            let ends: Vec<_> = text
                .lines()
                .skip(1)
                .map(|line| {
                    let size = ["+8.00 MiB", "-1.00 KiB", "+0 B"]
                        .into_iter()
                        .find(|s| line.contains(s))
                        .unwrap();
                    line.find(size).unwrap() + size.len()
                })
                .collect();
            assert!(ends.iter().all(|end| *end == ends[0]), "{text}");
            assert!(!text.contains("8388608"));
        }
        let mut out = Vec::new();
        super::write_width(&rows, true, 80, &mut out).unwrap();
        let json: serde_json::Value = serde_json::from_slice(&out).unwrap();
        assert_eq!(json["data"][0]["bytes_delta"], 8_388_608);
        assert_eq!(json["data"][3]["bytes_delta"], i64::MIN);
        assert!(super::super::now::signed_bytes(i64::MIN).starts_with('-'));
    }
}
