//! Owned by: W5-D
//! Compiled rule predicates over available core collector inputs.
use super::{AlertError, AlertSource};
use globset::{Glob, GlobMatcher};
use tidemark_core::{
    config::{AlertKind, AlertRule, AlertThreshold},
    time::parse_duration,
    units::{parse_rate, ByteSize},
};

pub(super) struct Rule {
    pub cfg: AlertRule,
    above: f64,
    below: f64,
    window: std::time::Duration,
    age: Option<f64>,
    path: GlobMatcher,
    vg: GlobMatcher,
    pool: GlobMatcher,
}
pub(super) struct Observation {
    pub subject: String,
    pub value: f64,
    pub condition: Option<bool>,
}
impl Rule {
    pub fn compile(cfg: &AlertRule) -> Result<Self, AlertError> {
        let glob = |s: &str| {
            Glob::new(s)
                .map(|g| g.compile_matcher())
                .map_err(|e| AlertError::Invalid(e.to_string()))
        };
        let duration = |s: &str| {
            parse_duration(s)
                .map(|d| d.as_secs_f64())
                .map_err(|e| AlertError::Invalid(e.to_string()))
        };
        let size = |s: &str| {
            s.parse::<ByteSize>()
                .map(|b| b.0 as f64)
                .map_err(|e| AlertError::Invalid(e.to_string()))
        };
        let above = match &cfg.above {
            None => 0.0,
            Some(AlertThreshold::Number(n)) => *n,
            Some(AlertThreshold::Text(s)) if cfg.kind == AlertKind::HotDirGrowth => {
                parse_rate(s).map_err(|e| AlertError::Invalid(e.to_string()))?
            }
            Some(AlertThreshold::Text(s)) => size(s)?,
        };
        Ok(Self {
            cfg: cfg.clone(),
            above,
            below: match cfg.below.as_deref() {
                None => 0.0,
                Some(s) if cfg.kind == AlertKind::TimeToFull => duration(s)?,
                Some(s) => size(s)?,
            },
            window: cfg
                .window
                .as_deref()
                .map(|s| parse_duration(s).map_err(|e| AlertError::Invalid(e.to_string())))
                .transpose()?
                .unwrap_or(std::time::Duration::from_secs(1)),
            age: cfg
                .older_than
                .as_deref()
                .filter(|s| *s != "auto")
                .map(duration)
                .transpose()?,
            path: glob(cfg.path.as_deref().unwrap_or("*"))?,
            vg: glob(cfg.vg.as_deref().unwrap_or("*"))?,
            pool: glob(cfg.pool.as_deref().unwrap_or("*"))?,
        })
    }
    pub fn visit(
        &self,
        now: i64,
        source: &dyn AlertSource,
        mut add: impl FnMut(Observation),
    ) -> Result<(), &'static str> {
        use AlertKind::*;
        let mount = self.cfg.mount.as_ref().map(|p| p.as_os_str().as_bytes());
        let root = self.cfg.root.as_ref().map(|p| p.as_os_str().as_bytes());
        let mount = std::str::from_utf8(mount.unwrap_or(b"")).map_err(|_| "mount is not UTF-8")?;
        let root = std::str::from_utf8(root.unwrap_or(b"")).map_err(|_| "root is not UTF-8")?;
        let window = self.window;
        let mut emit = |subject: String, value: f64, condition: Option<bool>| {
            add(Observation {
                subject,
                value,
                condition,
            })
        };
        match self.cfg.kind {
            FsUsedPct | TimeToFull => {
                let m = source
                    .fs_sample(mount)
                    .ok_or("filesystem sample unavailable")?;
                let v = if self.cfg.kind == FsUsedPct {
                    if m.size == 0 {
                        return Err("filesystem size is zero");
                    }
                    100.0 * m.used as f64 / m.size as f64
                } else {
                    let r = source
                        .fs_rate(mount, window)
                        .filter(|r| r.is_finite())
                        .ok_or("window rate unavailable or nonfinite")?;
                    if r > 0.0 {
                        m.avail as f64 / r
                    } else {
                        f64::MAX
                    }
                };
                emit(
                    display_path(mount.as_bytes()),
                    v,
                    Some(if self.cfg.kind == FsUsedPct {
                        v > self.above
                    } else {
                        v < self.below
                    }),
                );
            }
            HotDirGrowth => {
                let mut entries = source
                    .hot_dirs(mount, window)
                    .ok_or("hot directory window unavailable")?;
                // A lossy result is incomplete: it cannot confirm disappearance.
                if entries.iter().any(|e| e.lossy) {
                    return Err("hot directory window is lossy");
                }
                entries.sort_by(|a, b| a.key.cmp(&b.key));
                for e in entries {
                    if self
                        .path
                        .is_match(std::path::Path::new(std::ffi::OsStr::from_bytes(&e.key)))
                    {
                        let v = e.bytes_delta as f64 / self.window.as_secs_f64();
                        emit(display_path(&e.key), v, Some(v > self.above));
                    }
                }
            }
            ThinPoolPct | VgFreeLow => {
                let mut rows = source.lvm().ok_or("LVM inventory unavailable")?;
                rows.sort_by(|a, b| {
                    (&a.vg, &a.thin_pool, &a.lv).cmp(&(&b.vg, &b.thin_pool, &b.lv))
                });
                for l in rows.into_iter().filter(|l| self.vg.is_match(&l.vg)) {
                    if self.cfg.kind == VgFreeLow {
                        emit(
                            display_path(l.vg.as_bytes()),
                            l.vg_free as f64,
                            Some((l.vg_free as f64) < self.below),
                        );
                    } else if let Some(pool) =
                        l.thin_pool.as_ref().filter(|p| self.pool.is_match(p))
                    {
                        let data = l.data_pct.filter(|p| p.is_finite());
                        let meta = l.meta_pct.filter(|p| p.is_finite());
                        let condition = match (data, meta) {
                            (Some(d), Some(m)) => Some(
                                d > self.cfg.data_above.unwrap_or(100.0)
                                    || m > self.cfg.meta_above.unwrap_or(100.0),
                            ),
                            _ => None,
                        };
                        emit(
                            display_path(format!("{}/{}", l.vg, pool).as_bytes()),
                            data.unwrap_or(0.0).max(meta.unwrap_or(0.0)),
                            condition,
                        );
                    }
                }
            }
            PhantomBytes => {
                let v = source
                    .phantom_bytes(mount)
                    .ok_or("phantom measurement unavailable")? as f64;
                emit(display_path(mount.as_bytes()), v, Some(v > self.above));
            }
            DirGrowth => {
                let mut growth = source
                    .dir_growth(root, self.cfg.depth.unwrap_or(0))
                    .ok_or("snapshot comparison unavailable")?;
                growth.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
                for (path, delta) in growth {
                    emit(
                        path.display_lossy(),
                        delta as f64,
                        Some(delta as f64 > self.above),
                    );
                }
            }
            ScanStale => {
                let age = source.scan_age(root).ok_or("scan timing unavailable")?;
                let threshold = match self.age {
                    Some(v) => v,
                    None => {
                        2.0 * age
                            .effective_cadence
                            .ok_or("effective scan cadence unavailable")?
                            .as_secs_f64()
                    }
                };
                let v = age
                    .last_complete_at
                    .map(|t| (i128::from(now) - i128::from(t)).max(0) as f64)
                    .unwrap_or(f64::MAX);
                emit(display_path(root.as_bytes()), v, Some(v > threshold));
            }
            WatchLossy => {
                let v = source
                    .watch_overflows(mount, window)
                    .ok_or("overflow window unavailable")? as f64;
                emit(display_path(mount.as_bytes()), v, Some(v > self.above));
            }
        }
        Ok(())
    }
}
use std::os::unix::ffi::OsStrExt;
pub(super) fn display_path(bytes: &[u8]) -> String {
    tidemark_core::path::display_lossy_bytes(bytes)
}
