//! Owned by: W3-A
//! Command surface and shared CLI support.

use anyhow::Context as _;

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory path to query (canonicalised before lookup).
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Compare with a snapshot this duration ago.
    #[arg(long, value_name = "DUR", value_parser = humantime::parse_duration, conflicts_with = "from")]
    pub since: Option<std::time::Duration>,
    /// Starting snapshot ID.
    #[arg(long, value_name = "ID")]
    pub from: Option<String>,
    /// Ending snapshot ID.
    #[arg(long, value_name = "ID", requires = "from")]
    pub to: Option<String>,
    /// Directory depth to display.
    #[arg(long, value_name = "N", default_value_t = 3)]
    pub depth: usize,
    /// Metric to compare between snapshots.
    #[arg(long, value_parser = ["disk", "reclaimable", "files"])]
    pub metric: Option<String>,
    /// Explain the sources of changes.
    #[arg(long)]
    pub explain: bool,
    /// Minimum fraction of change to explain.
    #[arg(long, value_name = "F", default_value_t = 0.5)]
    pub explain_ratio: f64,
    /// Minimum size of change to explain.
    #[arg(long, value_name = "SIZE", default_value = "1GiB")]
    pub explain_min: tidemark_core::units::ByteSize,
    /// Maximum number of results.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

/// Compare catalog snapshots and render the selected metric.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    use tidemark_store::{
        catalog::Catalog,
        diff::{diff_with_retention, explain_with_retention, DiffMetric},
    };
    if args.from.is_none() && args.since.is_none() {
        return Err(
            crate::CliError::Configuration("diff requires --since or --from".into()).into(),
        );
    }
    let metric = match args.metric.as_deref().unwrap_or("disk") {
        "disk" => DiffMetric::Disk,
        "reclaimable" => DiffMetric::Reclaimable,
        "files" => DiffMetric::Files,
        _ => return Err(crate::CliError::Configuration("invalid diff metric".into()).into()),
    };
    if args.limit.is_some_and(|n| n > 1_000_000) {
        return Err(crate::CliError::Configuration("limit exceeds 1,000,000".into()).into());
    }
    if args.explain
        && (!args.explain_ratio.is_finite()
            || args.explain_ratio <= 0.0
            || args.explain_ratio > 1.0)
    {
        return Err(crate::CliError::Configuration("explain ratio must be in (0,1]".into()).into());
    }
    let catalog = Catalog::open(&context.config.general.state_dir, false).with_context(|| {
        format!(
            "open catalog: {}",
            super::display_path(&context.config.general.state_dir)
        )
    })?;
    // Removed targets cannot be canonicalized live. Canonicalize the nearest
    // existing ancestor, then append the missing byte names after rejecting unresolved parents.
    let target = query_path(
        args.path
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new("/data")),
    )?;
    let entry = if let Some(id) = &args.to {
        catalog.get(id)?
    } else {
        catalog
            .list(None)?
            .into_iter()
            .filter(|e| e.status == "complete" && target.starts_with(&e.root))
            .max_by(|a, b| {
                a.root
                    .as_bytes()
                    .len()
                    .cmp(&b.root.as_bytes().len())
                    .then_with(|| a.started_at.cmp(&b.started_at))
                    .then_with(|| a.scan_id.cmp(&b.scan_id))
            })
    }
    .ok_or_else(|| crate::CliError::Precondition("no matching new snapshot".into()))?;
    let now = tidemark_core::time::unix_now();
    let old = if let Some(id) = &args.from {
        catalog.get(id)?
    } else {
        let duration = args
            .since
            .ok_or_else(|| crate::CliError::Configuration("missing comparison duration".into()))?;
        let seconds = i64::try_from(duration.as_secs())
            .map_err(|_| crate::CliError::Configuration("duration too large".into()))?;
        if seconds == 0 {
            return Err(crate::CliError::Configuration("since must be positive".into()).into());
        }
        catalog.nearest_before(&entry.root, now.saturating_sub(seconds))?
    }
    .ok_or_else(|| crate::CliError::Precondition("no matching old snapshot".into()))?;
    let age_notice = args.since.and_then(|d| {
        let age = now.abs_diff(old.started_at);
        (age.abs_diff(d.as_secs()) as f64 > d.as_secs_f64() * 0.25).then(|| {
            format!(
                "comparison snapshot actual age: {}",
                humantime::format_duration(std::time::Duration::from_secs(age))
            )
        })
    });
    if !target.starts_with(&entry.root) || old.root != entry.root {
        return Err(
            crate::CliError::Precondition("snapshot roots do not match query".into()).into(),
        );
    }
    let old_file = old
        .file
        .as_ref()
        .ok_or_else(|| crate::CliError::Precondition("old snapshot has no file".into()))?;
    let new_file = entry
        .file
        .as_ref()
        .ok_or_else(|| crate::CliError::Precondition("new snapshot has no file".into()))?;
    let policy = &context.config.retention;
    let new = tidemark_store::Snapshot::open_with_retention(new_file, policy)
        .with_context(|| format!("open snapshot: {}", super::display_path(new_file)))?;
    let depth = u32::try_from(args.depth)
        .map_err(|_| crate::CliError::Configuration("depth exceeds u32".into()))?;
    let rows = diff_with_retention(
        &new,
        old_file,
        &target,
        metric,
        depth,
        args.limit.unwrap_or(25),
        policy,
    )?;
    let explanation = args
        .explain
        .then(|| {
            explain_with_retention(
                &new,
                old_file,
                &target,
                metric,
                args.explain_ratio,
                args.explain_min.0,
                5,
                policy,
            )
        })
        .transpose()?;
    let warning = crate::render::diff::coverage_notice(&old, &entry);
    let notice = match (warning, age_notice) {
        (Some(warning), Some(age)) => Some(format!("{warning}\nNotice: {age}")),
        (warning, age) => warning.or(age),
    };
    let mut out = std::io::stdout().lock();
    if !context.json {
        crate::render::diff::inputs_header(&old, &entry, now, &mut out)?;
    }
    crate::render::diff::report(
        &target,
        &rows,
        explanation.as_ref(),
        matches!(metric, DiffMetric::Files),
        notice.as_deref(),
        context.json,
        &mut out,
    )
}
fn query_path(path: &std::path::Path) -> anyhow::Result<tidemark_core::path::RawPath> {
    use std::os::unix::ffi::OsStrExt;
    match tidemark_fs::canon::canonicalize(path) {
        Ok(p) => Ok(p),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
            let mut ancestor = if path.is_absolute() {
                path.to_path_buf()
            } else {
                std::env::current_dir()?.join(path)
            };
            let mut names = Vec::new();
            loop {
                match tidemark_fs::canon::canonicalize(&ancestor) {
                    Ok(mut base) => {
                        for name in names.iter().rev() {
                            base = base.join(name);
                        }
                        return Ok(base);
                    }
                    Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                        let name = ancestor.file_name().ok_or_else(|| {
                            crate::CliError::Precondition(
                                "cannot resolve missing path with trailing parent components"
                                    .into(),
                            )
                        })?;
                        names.push(tidemark_core::path::RawName::new(name.as_bytes().to_vec())?);
                        ancestor = ancestor
                            .parent()
                            .ok_or_else(|| anyhow::anyhow!("missing root"))?
                            .to_path_buf();
                    }
                    Err(e) => return Err(e.into()),
                }
            }
        }
        Err(e) => Err(e.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn removed_target_resolves_existing_symlink_ancestor_before_normalization() {
        let dir = ::test_support::private_tempdir().unwrap();
        std::fs::create_dir_all(dir.path().join("real/deep")).unwrap();
        std::os::unix::fs::symlink("real/deep", dir.path().join("link")).unwrap();
        let target = query_path(&dir.path().join("link/../gone")).unwrap();
        let expected = tidemark_fs::canon::canonicalize(&dir.path().join("real"))
            .unwrap()
            .join(&tidemark_core::path::RawName::new(b"gone".to_vec()).unwrap());
        assert_eq!(target, expected);
        assert!(query_path(&dir.path().join("link/missing/../gone")).is_err());
    }
    #[test]
    fn missing_target_keeps_bytes_and_rejects_unresolved_parents() {
        use std::os::unix::ffi::OsStringExt;
        let dir = ::test_support::private_tempdir().unwrap();
        let missing = std::ffi::OsString::from_vec(b"missing\xff".to_vec());
        let target = query_path(&dir.path().join(&missing).join("child")).unwrap();
        let expected = tidemark_fs::canon::canonicalize(dir.path())
            .unwrap()
            .join(&tidemark_core::path::RawName::new(b"missing\xff".to_vec()).unwrap())
            .join(&tidemark_core::path::RawName::new(b"child".to_vec()).unwrap());
        assert_eq!(target, expected);
        assert_eq!(
            query_path(&dir.path().join(&missing).join("./child/")).unwrap(),
            expected
        );
        for suffix in ["missing/..", "missing/../child", "missing/child/../../gone"] {
            assert!(query_path(&dir.path().join(suffix)).is_err());
        }
    }
}
