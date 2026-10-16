//! Owned by: W4-A
//! Direct-mode LVM report composition with optional read-only history.

use crate::render::lvm::{Report, TrackedLv, Trend};
use anyhow::Context as _;
use tidemark_fs::{lvm, mount};
/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Filter by mount path.
    #[arg(long, value_name = "M")]
    pub mount: Option<std::path::PathBuf>,
}
/// Collect metadata directly and render current LVM and optional 24-hour pool trends.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    // W6-A3: daemon-first forwarding; retain the direct collector below.
    match crate::client::Client::connect(
        &context.config.general.socket,
        std::time::Duration::from_secs(11),
    ) {
        Ok(mut client) => {
            let report: tidemark_core::api::LvmResult = super::now::mount_call(
                &mut client,
                "lvm",
                context,
                &tidemark_core::api::LvmParams {
                    mount: super::now::mount(
                        args.mount.as_deref(),
                        "lvm",
                        context.json,
                        &mut std::io::stdout().lock(),
                    )?,
                },
            )?;
            let db = context.config.general.state_dir.join("timeseries.db");
            let history = if db.try_exists()? {
                tidemark_store::timeseries::TimeSeries::open(&db, false)
                    .map_err(|e| tracing::warn!(%e, "LVM history unavailable"))
                    .ok()
            } else {
                None
            };
            let now = super::phantom::timestamp()?;
            let mut trends = Vec::new();
            for (mount, lv) in &report.mounts {
                if lv.thin_pool.is_some() {
                    if let Some(db) = &history {
                        match db.query_lvm(&lv.vg, &lv.lv, now.saturating_sub(86400), now) {
                            Ok(samples) => trends.push((
                                mount.clone(),
                                slope(&samples, |s| s.data_pct),
                                slope(&samples, |s| s.meta_pct),
                            )),
                            Err(e) => tracing::warn!(%e, "LVM trend unavailable"),
                        }
                    }
                }
            }
            return crate::render::now::lvm(
                &report,
                &trends,
                context.json,
                &mut std::io::stdout().lock(),
            );
        }
        Err(_) => crate::render::now::fallback("lvm")?,
    }
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs(),
    )?;
    let report = match lvm::collect(&context.config.lvm, now) {
        Ok(report) => report,
        Err(lvm::LvmError::NotInstalled | lvm::LvmError::Disabled) => {
            return crate::render::lvm::write(
                &Report {
                    status: "n/a".into(),
                    sampled_at: now,
                    tracked: Vec::new(),
                    vgs: Vec::new(),
                    other_lvs: Vec::new(),
                },
                context.json,
                &mut std::io::stdout().lock(),
            );
        }
        Err(e) => return Err(e.into()),
    };
    let mounts = mount::read_mountinfo()?;
    let mut selected = Vec::new();
    let paths: Vec<_> = if let Some(path) = &args.mount {
        vec![path]
    } else {
        context.config.roots.iter().map(|root| &root.path).collect()
    };
    for path in paths {
        let canonical = std::fs::canonicalize(path)
            .with_context(|| format!("resolve path: {}", super::display_path(path)))?;
        let raw = tidemark_core::path::RawPath::from_path(&canonical)?;
        if let Some(m) = mount::mount_for_path(&mounts, &raw) {
            if !selected
                .iter()
                .any(|old: &&mount::MountInfo| old.id == m.id)
            {
                selected.push(m);
            }
        }
    }
    // Map only the selected mount records, so an overmounted entry with the same
    // mountpoint cannot borrow the visible mount's selection.
    let mut tracked = Vec::new();
    for selected_mount in selected {
        tracked.extend(lvm::map_mounts(
            &report,
            std::slice::from_ref(selected_mount),
        ));
    }
    tracked.sort_by(|a, b| a.mount.as_bytes().cmp(b.mount.as_bytes()));
    let db = context.config.general.state_dir.join("timeseries.db");
    let history = if db.try_exists()? {
        match tidemark_store::timeseries::TimeSeries::open(&db, false) {
            Ok(history) => Some(history),
            Err(e) => {
                tracing::warn!(%e,"LVM history unavailable");
                None
            }
        }
    } else {
        None
    };
    let output = compose(&report, &tracked, history.as_ref(), now);
    crate::render::lvm::write(&output, context.json, &mut std::io::stdout().lock())
}
fn compose(
    report: &lvm::LvmReport,
    tracked: &[lvm::MountLv],
    history: Option<&tidemark_store::timeseries::TimeSeries>,
    now: i64,
) -> Report {
    let mut rows = Vec::new();
    for m in tracked {
        let current = lvm::lvm_now(report, m);
        let lv = report.lvs.iter().find(|lv| lv.vg == m.vg && lv.lv == m.lv);
        let trend = if m.thin_pool.is_some() {
            history.and_then(|db| {
                match db.query_lvm(&m.vg, &m.lv, now.saturating_sub(86400), now) {
                    Ok(samples) => Some(Trend {
                        data_pp_per_day: slope(&samples, |s| s.data_pct),
                        meta_pp_per_day: slope(&samples, |s| s.meta_pct),
                    }),
                    Err(e) => {
                        tracing::warn!(%e,"LVM trend unavailable");
                        None
                    }
                }
            })
        } else {
            None
        };
        rows.push(TrackedLv {
            mount: m.mount.to_json(),
            mount_display: m.mount.display_lossy(),
            current,
            segtype: lv.map_or_else(String::new, |lv| lv.segtype.clone()),
            trend,
        });
    }
    let vgs = report
        .vgs
        .iter()
        .filter(|vg| tracked.iter().any(|m| m.vg == vg.vg))
        .cloned()
        .collect();
    let other_lvs = report
        .lvs
        .iter()
        .filter(|lv| {
            tracked.iter().any(|m| m.vg == lv.vg)
                && !tracked.iter().any(|m| m.vg == lv.vg && m.lv == lv.lv)
        })
        .cloned()
        .collect();
    Report {
        status: if rows.is_empty() { "n/a" } else { "ok" }.into(),
        sampled_at: report.sampled_at,
        tracked: rows,
        vgs,
        other_lvs,
    }
}
fn slope(
    samples: &[tidemark_core::live::LvmSample],
    value: impl Fn(&tidemark_core::live::LvmSample) -> Option<f64>,
) -> Option<f64> {
    let base = samples.first()?.ts;
    let mut count = 0.0;
    let mut x = 0.0;
    let mut y = 0.0;
    let mut xx = 0.0;
    let mut xy = 0.0;
    for s in samples {
        if let Some(v) = value(s).filter(|v| v.is_finite()) {
            let t = (i128::from(s.ts) - i128::from(base)) as f64;
            count += 1.0;
            x += t;
            y += v;
            xx += t * t;
            xy += t * v;
        }
    }
    let denominator = count * xx - x * x;
    if count < 3.0 || denominator <= 0.0 {
        None
    } else {
        Some((count * xy - x * y) / denominator * 86400.0)
    }
}
#[cfg(test)]
#[path = "../../tests/w4a/lvm.rs"]
mod tests;
