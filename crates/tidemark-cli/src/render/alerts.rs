//! Owned by: W6-A3
//! Deterministic active and current-run historical alert tables.
use std::io::Write;
use tidemark_core::{
    live::{AlertEvent, AlertSummary},
    path::display_lossy_bytes as display,
};
/// Render active alerts in rule/subject order.
pub fn active(rows: &[AlertSummary], json: bool, out: &mut impl Write) -> anyhow::Result<()> {
    let mut rows: Vec<_> = rows.iter().collect();
    rows.sort_by(|a, b| (&a.rule, &a.subject).cmp(&(&b.rule, &b.subject)));
    if json {
        return crate::render::json("tidemark.alerts.v1", rows, out);
    }
    let mut table = vec![["RULE", "SUBJECT", "SINCE", "VALUE", "MESSAGE"]
        .map(str::to_owned)
        .to_vec()];
    for r in rows {
        table.push(vec![
            display(r.rule.as_bytes()),
            display(r.subject.as_bytes()),
            r.since.to_string(),
            r.value.map_or_else(|| "?".into(), |v| v.to_string()),
            display(r.message.as_bytes()),
        ]);
    }
    out.write_all(crate::render::table::aligned(&table).as_bytes())?;
    Ok(())
}
/// Render history, explicitly limited to the current daemon run.
pub fn history(rows: &[AlertEvent], json: bool, out: &mut impl Write) -> anyhow::Result<()> {
    let mut rows: Vec<_> = rows.iter().collect();
    rows.sort_by(|a, b| {
        b.ts.cmp(&a.ts)
            .then((&a.rule, &a.subject).cmp(&(&b.rule, &b.subject)))
    });
    if json {
        return crate::render::json("tidemark.alerts.v1", rows, out);
    }
    let mut table = vec![["TIME", "RULE", "SUBJECT", "STATE", "VALUE", "MESSAGE"]
        .map(str::to_owned)
        .to_vec()];
    for r in rows {
        table.push(vec![
            r.ts.to_string(),
            display(r.rule.as_bytes()),
            display(r.subject.as_bytes()),
            format!("{:?}", r.state).to_lowercase(),
            r.value.map_or_else(|| "?".into(), |v| v.to_string()),
            display(r.message.as_bytes()),
        ]);
    }
    out.write_all(crate::render::table::aligned(&table).as_bytes())?;
    writeln!(out, "History covers the current daemon run only.")?;
    Ok(())
}
