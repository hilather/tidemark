//! Owned by: W5-D
//! Compiled rule predicates over available core collector inputs.
use super::{AlertError, AlertInputs};
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
    window: f64,
    age: Option<f64>,
    path: GlobMatcher,
    vg: GlobMatcher,
    pool: GlobMatcher,
}
pub(super) struct Observation {
    pub subject: String,
    pub value: f64,
    pub condition: bool,
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
                .map(duration)
                .transpose()?
                .unwrap_or(1.0),
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
    pub fn available(&self, i: &AlertInputs<'_>) -> bool {
        use AlertKind::*;
        let mount = |m: &str| self.cfg.mount.as_deref() == Some(std::path::Path::new(m));
        let root = |r: &str| self.cfg.root.as_deref() == Some(std::path::Path::new(r));
        match self.cfg.kind {
            FsUsedPct => i.mounts.iter().any(|m| mount(&m.mount) && m.size > 0),
            TimeToFull => i.mounts.iter().any(|m| {
                mount(&m.mount)
                    && (if self.window == 900.0 {
                        m.rate_15m
                    } else if self.window == 3600.0 {
                        m.rate_1h
                    } else {
                        None
                    })
                    .is_some_and(f64::is_finite)
            }),
            HotDirGrowth => i
                .hot_dirs_window
                .iter()
                .any(|(m, e)| mount(m) && !e.iter().any(|e| e.lossy)),
            ThinPoolPct | VgFreeLow => !i.lvm.is_empty(),
            PhantomBytes => i.phantom.iter().any(|p| mount(&p.mount)),
            DirGrowth => i.dir_growth.iter().any(|d| root(&d.root)),
            PgWalBytes => i.pg.is_some_and(|p| {
                p.unreachable_since.is_none() && p.sampled_at.is_some() && p.wal_dir_bytes.is_some()
            }),
            PgSlotRetained => i.pg.is_some_and(|p| {
                p.unreachable_since.is_none()
                    && p.sampled_at.is_some()
                    && p.slots.iter().all(|s| s.retained_bytes.is_some())
            }),
            PgArchiverFailing => {
                i.pg.is_some_and(|p| p.unreachable_since.is_none() && p.sampled_at.is_some())
            }
            ScanStale => self.age.is_some() && i.scans.iter().any(|s| root(&s.root)),
            WatchLossy => {
                self.window == 900.0 && i.watch_overflows_15m.iter().any(|(m, _)| mount(m))
            }
        }
    }
    pub fn visit(&self, now: i64, i: &AlertInputs<'_>, mut add: impl FnMut(Observation)) {
        use AlertKind::*;
        let mount = self.cfg.mount.as_deref();
        let matches_mount = |s: &str| mount.is_some_and(|p| p == std::path::Path::new(s));
        let mut emit = |subject: String, value: f64, condition: bool| {
            if value.is_finite() {
                add(Observation {
                    subject,
                    value,
                    condition,
                });
            }
        };
        match self.cfg.kind {
            FsUsedPct | TimeToFull => {
                for m in i.mounts.iter().filter(|m| matches_mount(&m.mount)) {
                    if self.cfg.kind == FsUsedPct {
                        if m.size > 0 {
                            let v = 100.0 * m.used as f64 / m.size as f64;
                            emit(m.mount.clone(), v, v > self.above);
                        }
                    } else {
                        let rate = if self.window == 900.0 {
                            m.rate_15m
                        } else if self.window == 3600.0 {
                            m.rate_1h
                        } else {
                            None
                        };
                        if let Some(rate) = rate.filter(|r| r.is_finite()) {
                            let v = if rate > 0.0 {
                                m.avail as f64 / rate
                            } else {
                                f64::MAX
                            };
                            emit(m.mount.clone(), v, v < self.below);
                        }
                    }
                }
            }
            HotDirGrowth => {
                for (m, entries) in i.hot_dirs_window.iter().filter(|(m, _)| matches_mount(m)) {
                    let _ = m;
                    for e in entries {
                        if self
                            .path
                            .is_match(std::path::Path::new(std::ffi::OsStr::from_bytes(&e.key)))
                            && !e.lossy
                        {
                            let v = e.bytes_delta as f64 / self.window;
                            emit(display_path(&e.key), v, v > self.above);
                        }
                    }
                }
            }
            ThinPoolPct | VgFreeLow => {
                for (_, l) in i.lvm.iter().filter(|(_, l)| self.vg.is_match(&l.vg)) {
                    if self.cfg.kind == VgFreeLow {
                        emit(
                            l.vg.clone(),
                            l.vg_free as f64,
                            (l.vg_free as f64) < self.below,
                        );
                    } else if let Some(pool) =
                        l.thin_pool.as_ref().filter(|p| self.pool.is_match(p))
                    {
                        if let (Some(d), Some(m)) = (l.data_pct, l.meta_pct) {
                            emit(
                                format!("{}/{}", l.vg, pool),
                                d.max(m),
                                d > self.cfg.data_above.unwrap_or(100.0)
                                    || m > self.cfg.meta_above.unwrap_or(100.0),
                            );
                        }
                    }
                }
            }
            PhantomBytes => {
                for p in i.phantom.iter().filter(|p| matches_mount(&p.mount)) {
                    emit(
                        p.mount.clone(),
                        p.total_bytes as f64,
                        p.total_bytes as f64 > self.above,
                    );
                }
            }
            DirGrowth => {
                for d in i.dir_growth.iter().filter(|d| {
                    self.cfg.root.as_deref() == Some(std::path::Path::new(&d.root))
                        && d.depth <= self.cfg.depth.unwrap_or(0)
                }) {
                    emit(
                        d.path.display_lossy(),
                        d.delta as f64,
                        d.delta as f64 > self.above,
                    );
                }
            }
            PgWalBytes | PgSlotRetained | PgArchiverFailing => {
                if let Some(pg) =
                    i.pg.filter(|p| p.unreachable_since.is_none() && p.sampled_at.is_some())
                {
                    match self.cfg.kind {
                        PgWalBytes => {
                            if let Some(v) = pg.wal_dir_bytes {
                                emit("postgres".into(), v as f64, v as f64 > self.above);
                            }
                        }
                        PgSlotRetained => {
                            for slot in &pg.slots {
                                if let Some(v) = slot.retained_bytes {
                                    emit(slot.name.clone(), v as f64, v as f64 > self.above);
                                }
                            }
                        }
                        _ => {
                            let condition = pg.archiver_failing
                                && pg.archiver_last_failed.is_some_and(|ts| {
                                    ts <= now
                                        && (i128::from(now) - i128::from(ts)) as f64 <= self.window
                                });
                            emit(
                                "postgres".into(),
                                if condition { 1.0 } else { 0.0 },
                                condition,
                            );
                        }
                    }
                }
            }
            ScanStale => {
                for s in i
                    .scans
                    .iter()
                    .filter(|s| self.cfg.root.as_deref() == Some(std::path::Path::new(&s.root)))
                {
                    if let Some(age) = self.age {
                        let v = s
                            .last_complete
                            .as_ref()
                            .map(|s| (i128::from(now) - i128::from(s.finished_at)).max(0) as f64)
                            .unwrap_or(f64::MAX);
                        emit(s.root.clone(), v, v > age);
                    }
                }
            }
            WatchLossy => {
                if self.window == 900.0 {
                    for (m, v) in i
                        .watch_overflows_15m
                        .iter()
                        .filter(|(m, _)| matches_mount(m))
                    {
                        emit(m.clone(), *v as f64, *v as f64 > self.above);
                    }
                }
            }
        }
    }
}
use std::os::unix::ffi::OsStrExt;
pub(super) fn display_path(bytes: &[u8]) -> String {
    tidemark_core::path::RawPath::new(bytes.to_vec())
        .map(|p| p.display_lossy())
        .unwrap_or_else(|_| String::from_utf8_lossy(bytes).escape_default().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;
    use tidemark_core::{live::*, path::RawPath};
    #[test]
    fn all_rule_predicates_and_unavailable_collectors() {
        let mounts = [crate::rates::MountRates {
            mount: "/data".into(),
            size: 100,
            used: 95,
            avail: 5,
            rate_15m: Some(2.0),
            rate_1h: Some(1.0),
            ttf: None,
        }];
        let lvm = [(
            "/data".into(),
            LvmNow {
                vg: "vg".into(),
                lv: "lv".into(),
                lv_size: 100,
                vg_free: 0,
                thin_pool: Some("pool".into()),
                data_pct: Some(81.0),
                meta_pct: Some(10.0),
                sampled_at: 100,
            },
        )];
        let phantom = [PhantomSummary {
            mount: "/data".into(),
            total_bytes: 100,
            files: 1,
            holders: vec![],
        }];
        let hot = [(
            "/data".into(),
            vec![HotEntry {
                key: b"/data/logs".to_vec(),
                events: 1,
                files: 1,
                bytes_delta: 3000,
                top_unit: None,
                lossy: false,
            }],
        )];
        let pg = PgNow {
            sampled_at: Some(100),
            unreachable_since: None,
            server_version_num: Some(160000),
            databases: vec![],
            wal_dir_bytes: Some(100),
            slots: vec![PgSlot {
                name: "slot".into(),
                slot_type: "physical".into(),
                active: false,
                retained_bytes: Some(100),
                wal_status: None,
            }],
            archiver_failing: true,
            archiver_last_failed: Some(99),
            temp_bytes_rate: None,
            top_relations: vec![],
        };
        let scans = [ScanStatus {
            root: "/data".into(),
            running: None,
            last_complete: None,
            next_due: None,
        }];
        let growth = [crate::rates::DirGrowth {
            root: "/data".into(),
            path: RawPath::new(b"/data/logs".to_vec()).unwrap(),
            depth: 1,
            delta: 100,
            between: ("a".into(), "b".into()),
        }];
        let overflows = [("/data".into(), 11)];
        let i = AlertInputs {
            mounts: &mounts,
            lvm: &lvm,
            phantom: &phantom,
            hot_dirs_window: &hot,
            pg: Some(&pg),
            scans: &scans,
            dir_growth: &growth,
            watch_overflows_15m: &overflows,
            state_space_refused: false,
        };
        use AlertKind::*;
        for kind in [
            FsUsedPct,
            TimeToFull,
            HotDirGrowth,
            ThinPoolPct,
            VgFreeLow,
            DirGrowth,
            PhantomBytes,
            PgWalBytes,
            PgSlotRetained,
            PgArchiverFailing,
            ScanStale,
            WatchLossy,
        ] {
            let mut cfg = AlertRule {
                name: "rule".into(),
                kind,
                for_: Duration::ZERO,
                ..Default::default()
            };
            match kind {
                FsUsedPct => {
                    cfg.mount = Some("/data".into());
                    cfg.above = Some(AlertThreshold::Number(90.0));
                }
                TimeToFull => {
                    cfg.mount = Some("/data".into());
                    cfg.below = Some("12h".into());
                    cfg.window = Some("1h".into());
                }
                HotDirGrowth => {
                    cfg.mount = Some("/data".into());
                    cfg.path = Some("/data/*".into());
                    cfg.above = Some(AlertThreshold::Text("1B/s".into()));
                    cfg.window = Some("5m".into());
                }
                ThinPoolPct => {
                    cfg.data_above = Some(80.0);
                    cfg.meta_above = Some(80.0);
                }
                VgFreeLow => {
                    cfg.vg = Some("vg".into());
                    cfg.below = Some("1B".into());
                }
                DirGrowth => {
                    cfg.root = Some("/data".into());
                    cfg.depth = Some(1);
                    cfg.above = Some(AlertThreshold::Number(99.0));
                }
                PhantomBytes | PgWalBytes | PgSlotRetained => {
                    cfg.above = Some(AlertThreshold::Text("99B".into()));
                    if kind == PhantomBytes {
                        cfg.mount = Some("/data".into());
                    }
                }
                PgArchiverFailing => cfg.window = Some("30m".into()),
                ScanStale => {
                    cfg.root = Some("/data".into());
                    cfg.older_than = Some("1h".into());
                }
                WatchLossy => {
                    cfg.mount = Some("/data".into());
                    cfg.above = Some(AlertThreshold::Number(10.0));
                    cfg.window = Some("15m".into());
                }
            }
            let mut engine =
                super::super::AlertEngine::new(&[cfg], Duration::from_secs(1)).unwrap();
            assert_eq!(engine.evaluate(100, &i).len(), 1, "{kind:?}");
        }
        let cfg = AlertRule {
            name: "wal".into(),
            kind: PgWalBytes,
            above: Some(AlertThreshold::Number(1.0)),
            for_: Duration::ZERO,
            ..Default::default()
        };
        let mut engine = super::super::AlertEngine::new(&[cfg], Duration::from_secs(1)).unwrap();
        let mut unreachable = pg.clone();
        unreachable.unreachable_since = Some(90);
        let mut i = i;
        i.pg = Some(&unreachable);
        assert!(engine.evaluate(100, &i).is_empty());
        i.pg = None;
        assert!(engine.evaluate(101, &i).is_empty());
    }
}

#[cfg(test)]
mod missing_subject_tests {
    use super::*;
    use std::time::Duration;
    use tidemark_core::live::*;
    #[test]
    fn vanished_slot_resolves_but_unreachable_does_not() {
        let rule = AlertRule {
            name: "slot".into(),
            kind: AlertKind::PgSlotRetained,
            above: Some(AlertThreshold::Number(1.0)),
            for_: Duration::ZERO,
            resolve_for: Duration::from_secs(5),
            ..Default::default()
        };
        let mut engine = super::super::AlertEngine::new(&[rule], Duration::from_secs(20)).unwrap();
        let mut pg = PgNow {
            sampled_at: Some(0),
            unreachable_since: None,
            server_version_num: None,
            databases: vec![],
            wal_dir_bytes: None,
            slots: vec![PgSlot {
                name: "replica".into(),
                slot_type: "physical".into(),
                active: false,
                retained_bytes: Some(10),
                wal_status: None,
            }],
            archiver_failing: false,
            archiver_last_failed: None,
            temp_bytes_rate: None,
            top_relations: vec![],
        };
        fn input(pg: &PgNow) -> AlertInputs<'_> {
            AlertInputs {
                mounts: &[],
                lvm: &[],
                phantom: &[],
                hot_dirs_window: &[],
                pg: Some(pg),
                scans: &[],
                dir_growth: &[],
                watch_overflows_15m: &[],
                state_space_refused: false,
            }
        }
        assert_eq!(engine.evaluate(0, &input(&pg)).len(), 1);
        pg.slots.clear();
        pg.unreachable_since = Some(1);
        assert!(engine.evaluate(1, &input(&pg)).is_empty());
        assert!(engine.evaluate(100, &input(&pg)).is_empty());
        pg.unreachable_since = None;
        assert!(engine.evaluate(101, &input(&pg)).is_empty());
        assert_eq!(
            engine.evaluate(106, &input(&pg))[0].state,
            AlertState::Resolved
        );
    }
}

#[cfg(test)]
mod window_tests {
    use super::*;
    use tidemark_core::live::HotEntry;
    #[test]
    fn numeric_and_text_rates_match_and_lossy_is_suppressed() {
        let cfg = AlertRule {
            name: "hot".into(),
            kind: AlertKind::HotDirGrowth,
            mount: Some("/data".into()),
            path: Some("/data/*".into()),
            above: Some(AlertThreshold::Number(2.0)),
            window: Some("5m".into()),
            ..Default::default()
        };
        let numeric = Rule::compile(&cfg).unwrap();
        let mut cfg = cfg;
        cfg.above = Some(AlertThreshold::Text("2B/s".into()));
        let text = Rule::compile(&cfg).unwrap();
        let mut hot = [(
            "/data".into(),
            vec![HotEntry {
                key: b"/data/log".to_vec(),
                events: 1,
                files: 1,
                bytes_delta: 601,
                top_unit: None,
                lossy: false,
            }],
        )];
        fn input(hot: &[(String, Vec<HotEntry>)]) -> AlertInputs<'_> {
            AlertInputs {
                mounts: &[],
                lvm: &[],
                phantom: &[],
                hot_dirs_window: hot,
                pg: None,
                scans: &[],
                dir_growth: &[],
                watch_overflows_15m: &[],
                state_space_refused: false,
            }
        }
        let mut a = vec![];
        numeric.visit(0, &input(&hot), |o| a.push((o.value, o.condition)));
        let mut b = vec![];
        text.visit(0, &input(&hot), |o| b.push((o.value, o.condition)));
        assert_eq!(a, b);
        assert!(a[0].1);
        hot[0].1[0].bytes_delta = 600;
        numeric.visit(0, &input(&hot), |o| assert!(!o.condition));
        hot[0].1[0].lossy = true;
        numeric.visit(0, &input(&hot), |_| panic!("lossy observation fired"));
        assert!(!numeric.available(&input(&hot)));
        cfg.path = Some("[".into());
        assert!(Rule::compile(&cfg).is_err());
    }
}
