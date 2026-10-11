//! Owned by: W5-C
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Include Postgres relations.
    #[arg(long)]
    pub relations: bool,
    /// Select the Postgres database.
    #[arg(long, value_name = "NAME")]
    pub db: Option<String>,
}

/// Collect and render PostgreSQL metadata once.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    use tidemark_core::live::PgNow;
    let mut cfg = context.config.postgres.clone();
    if let Some(db) = &args.db {
        if db.is_empty() {
            anyhow::bail!("database name must not be empty");
        }
    }
    cfg.enabled = true;
    let mut collector = tidemark_pg::PgCollector::new(cfg);
    let now = tidemark_core::time::unix_now();
    let mut samples = collector.collect_fast(now)?;
    if args.relations {
        samples.extend(match &args.db {
            Some(db) => collector.collect_relations_for_database(now, db)?,
            None => collector.collect_relations(now)?,
        });
    }
    let mut state: PgNow = collector
        .latest()
        .ok_or_else(|| anyhow::anyhow!("PostgreSQL returned no state"))?;
    if let Some(db) = &args.db {
        state.databases.retain(|(name, _)| name == db);
        state
            .top_relations
            .retain(|(name, _)| name.starts_with(&format!("{db}.")));
        samples.retain(|s| match s.kind.as_str() {
            "database" | "temp_bytes_total" => s.name == *db,
            "relation" | "dead_tuples" => s.name.starts_with(&format!("{db}.")),
            _ => true,
        });
    }
    render(
        &mut std::io::stdout().lock(),
        context.json,
        &state,
        &samples,
    )
}

fn render(
    out: &mut impl std::io::Write,
    json: bool,
    state: &tidemark_core::live::PgNow,
    samples: &[tidemark_core::live::PgSample],
) -> anyhow::Result<()> {
    use tidemark_core::units::fmt_bytes;
    if json {
        serde_json::to_writer(
            &mut *out,
            &tidemark_core::json::JsonDoc {
                schema: "tidemark.pg.v1".into(),
                data: serde_json::json!({"now":state,"samples":samples}),
            },
        )?;
        writeln!(out)?;
        return Ok(());
    }
    writeln!(out, "Databases:")?;
    for (name, size) in &state.databases {
        writeln!(out, "  {}  {}", fmt_bytes(*size), name)?;
    }
    writeln!(
        out,
        "WAL directory: {}",
        state
            .wal_dir_bytes
            .map(fmt_bytes)
            .unwrap_or_else(|| "unknown".into())
    )?;
    for slot in &state.slots {
        writeln!(
            out,
            "Slot {} ({}) active={} retained={} wal_status={}",
            slot.name,
            slot.slot_type,
            slot.active,
            slot.retained_bytes
                .map(fmt_bytes)
                .unwrap_or_else(|| "unknown".into()),
            slot.wal_status.as_deref().unwrap_or("unavailable")
        )?;
    }
    writeln!(
        out,
        "Archiver: {} (last failure {:?})",
        if state.archiver_failing {
            "failing"
        } else {
            "ok"
        },
        state.archiver_last_failed
    )?;
    for sample in samples.iter().filter(|s| {
        s.kind == "temp_bytes_total" || s.kind == "archiver_failed_total" || s.kind == "dead_tuples"
    }) {
        writeln!(out, "{} {}: {}", sample.kind, sample.name, sample.value)?;
    }
    for (name, size) in &state.top_relations {
        writeln!(out, "Relation {}: {}", name, fmt_bytes(*size))?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "../../tests/w5c/render.rs"]
mod tests;
