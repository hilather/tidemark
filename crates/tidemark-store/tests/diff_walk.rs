//! Owned by: SNAP-diff-walk
//! Tree-walk `diff`/`explain` against the old ATTACH+join SQL (v1/v1) and a model oracle (all pairs).
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[allow(dead_code)]
mod w3a {
    pub mod support;
}
use std::{collections::BTreeMap, path::PathBuf};
use tidemark_core::{
    path::RawPath,
    snapshot::{DirRow, SortKey},
};
use tidemark_store::{
    cold::compress_snapshot,
    diff::{diff, explain, DiffClass, DiffMetric, DiffRow},
    writer::SnapshotWriter,
    writer_v2::{SnapshotWriterV2, WriterOptions},
    Snapshot,
};
use w3a::support::{meta, row};

type Comps = Vec<Vec<u8>>;
type Tree = BTreeMap<Comps, u64>;
const ROOT: &[u8] = b"/da\xffta";
const CAP: usize = 1_000_000;

#[derive(Clone, Copy, PartialEq, Debug)]
enum Kind {
    V1,
    V2,
    ColdV1,
    ColdV2,
}

struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 ^= self.0 << 13;
        self.0 ^= self.0 >> 7;
        self.0 ^= self.0 << 17;
        self.0
    }
    fn below(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
}

const NAMES: &[&[u8]] = &[
    b"a", b"a-c", b"a0", b"a\xff", b"a.b", b"b", b"\xfe", b"ab", b"a b", b"c",
];

fn path_of(root: &[u8], comps: &Comps) -> RawPath {
    let mut p = root.to_vec();
    for c in comps {
        p.push(b'/');
        p.extend_from_slice(c);
    }
    RawPath::new(p).unwrap()
}

fn gen_tree(rng: &mut Rng, size: usize) -> Tree {
    let mut t = Tree::new();
    t.insert(vec![], 50 + rng.below(50) as u64);
    for _ in 0..size {
        let keys: Vec<_> = t.keys().cloned().collect();
        let mut c = keys[rng.below(keys.len())].clone();
        if c.len() >= 5 {
            continue;
        }
        c.push(NAMES[rng.below(NAMES.len())].to_vec());
        t.insert(c, rng.below(12) as u64);
    }
    t
}

fn mutate(rng: &mut Rng, old: &Tree) -> Tree {
    let mut t = old.clone();
    for _ in 0..1 + rng.below(8) {
        let keys: Vec<_> = t.keys().cloned().collect();
        let k = keys[rng.below(keys.len())].clone();
        match rng.below(5) {
            0 => *t.get_mut(&k).unwrap() += 1 + rng.below(5) as u64, // grow
            1 => {
                let v = t.get_mut(&k).unwrap();
                *v = v.saturating_sub(1 + rng.below(5) as u64); // shrink
            }
            2 if !k.is_empty() => t.retain(|x, _| !x.starts_with(&k)), // remove subtree
            3 if k.len() < 5 => {
                let mut c = k.clone();
                c.push(NAMES[rng.below(NAMES.len())].to_vec());
                t.entry(c).or_insert(rng.below(12) as u64); // add
            }
            4 if !k.is_empty() => {
                // rename or move: same values, new place
                let moved: Vec<_> = t
                    .iter()
                    .filter(|(x, _)| x.starts_with(&k))
                    .map(|(x, v)| (x.clone(), *v))
                    .collect();
                t.retain(|x, _| !x.starts_with(&k));
                let mut dest = if rng.below(2) == 0 {
                    k[..k.len() - 1].to_vec()
                } else {
                    vec![]
                };
                dest.push(NAMES[rng.below(NAMES.len())].to_vec());
                for (x, v) in moved {
                    let mut n = dest.clone();
                    n.extend_from_slice(&x[k.len()..]);
                    t.insert(n, v);
                }
                t.entry(vec![]).or_insert(1);
            }
            _ => {}
        }
        // Restore ancestors that removal/moves may have orphaned.
        let all: Vec<_> = t.keys().cloned().collect();
        for c in all {
            for l in 0..c.len() {
                t.entry(c[..l].to_vec()).or_insert(3);
            }
        }
    }
    t
}

fn rows_of(root: &[u8], t: &Tree) -> Vec<DirRow> {
    let keys: Vec<&Comps> = t.keys().collect();
    let ids: BTreeMap<&Comps, u32> = keys
        .iter()
        .enumerate()
        .map(|(i, k)| (*k, i as u32))
        .collect();
    let mut end: Vec<u32> = (0..keys.len() as u32).collect();
    for i in (1..keys.len()).rev() {
        let parent = ids[&keys[i][..keys[i].len() - 1].to_vec()] as usize;
        end[parent] = end[parent].max(end[i]);
    }
    keys.iter()
        .enumerate()
        .map(|(i, k)| {
            let v = t[*k];
            let p = path_of(root, k);
            let parent = (!k.is_empty()).then(|| ids[&k[..k.len() - 1].to_vec()]);
            let mut r = row(i as u32, parent, p.as_bytes(), v);
            r.depth = k.len() as u32;
            r.subtree_end = end[i];
            r.name = k.last().cloned().unwrap_or_else(|| p.as_bytes().to_vec());
            r.t_reclaimable = v / 2;
            r.t_files = v % 5;
            r.own_charged = v / 3;
            r
        })
        .collect()
}

fn write(dir: &std::path::Path, root: &[u8], t: &Tree, now: i64, kind: Kind, opt: u64) -> PathBuf {
    let rows = rows_of(root, t);
    let mut m = meta(RawPath::new(root.to_vec()).unwrap(), now);
    m.dirs = rows.len() as u64;
    let v2 = matches!(kind, Kind::V2 | Kind::ColdV2);
    m.schema_version = if v2 { 2 } else { 1 };
    let hot = if v2 {
        let options = WriterOptions {
            plain_depth: (opt % 3) as u32,
            hash_width: if opt.is_multiple_of(2) { 64 } else { 32 },
            chunk_dirs: 1 + (opt % 4) as usize,
            ..WriterOptions::default()
        };
        let mut w = SnapshotWriterV2::create_with_options(dir, &m, options).unwrap();
        for c in rows.chunks(5) {
            w.put_dirs(c).unwrap();
        }
        w.finish(&m).unwrap()
    } else {
        let mut w = SnapshotWriter::create(dir, &m).unwrap();
        for c in rows.chunks(5) {
            w.put_dirs(c).unwrap();
        }
        w.finish(&m).unwrap()
    };
    if matches!(kind, Kind::ColdV1 | Kind::ColdV2) {
        compress_snapshot(&hot, 1, &|| false).unwrap()
    } else {
        hot
    }
}

fn open(p: &std::path::Path) -> Snapshot {
    Snapshot::open_with_space_guard(p, |_| Ok(())).unwrap()
}

fn val(r: &DirRow, m: DiffMetric) -> u64 {
    match m {
        DiffMetric::Disk => r.t_disk,
        DiffMetric::Reclaimable => r.t_reclaimable,
        DiffMetric::Files => r.t_files,
    }
}

type Flat = (Vec<u8>, u32, Option<u64>, Option<u64>, i64, DiffClass);
fn flat(r: &DiffRow) -> Flat {
    (
        r.path.as_bytes().to_vec(),
        r.depth_rel,
        r.new,
        r.old,
        r.delta,
        r.class,
    )
}

fn class(n: Option<u64>, o: Option<u64>) -> DiffClass {
    let d = n.unwrap_or(0) as i64 - o.unwrap_or(0) as i64;
    match (n, o) {
        (Some(_), None) => DiffClass::New,
        (None, Some(_)) => DiffClass::Removed,
        _ if d > 0 => DiffClass::Grown,
        _ if d < 0 => DiffClass::Shrunk,
        _ => DiffClass::Same,
    }
}

/// `direction = None`: subtree diff ordered by |delta|; `Some(d)`: depth-1 children by delta*sign(d).
fn model(
    new: &Snapshot,
    old: &Snapshot,
    target: &RawPath,
    m: DiffMetric,
    depth: u32,
    limit: usize,
    direction: Option<i64>,
) -> (Vec<Flat>, u32) {
    let load = |s: &Snapshot| -> BTreeMap<Vec<u8>, (u32, u64)> {
        let Some(t) = s.dir_by_path(target).unwrap() else {
            return BTreeMap::new();
        };
        s.subtree_dirs(t.id, depth, SortKey::Name, CAP)
            .unwrap()
            .iter()
            .map(|r| (r.path.as_bytes().to_vec(), (r.depth - t.depth, val(r, m))))
            .collect()
    };
    let (n, o) = (load(new), load(old));
    let mut keys: Vec<&Vec<u8>> = n
        .keys()
        .chain(o.keys().filter(|k| !n.contains_key(*k)))
        .collect();
    keys.sort();
    let mut rows: Vec<(i128, Flat)> = keys
        .into_iter()
        .filter_map(|k| {
            let (nv, ov) = (n.get(k).map(|x| x.1), o.get(k).map(|x| x.1));
            let d = (n.get(k).or(o.get(k)).unwrap()).0;
            if direction.is_some() && d != 1 {
                return None;
            }
            let delta = nv.unwrap_or(0) as i64 - ov.unwrap_or(0) as i64;
            let sign = if direction.is_some_and(|x| x < 0) {
                -1
            } else {
                1
            };
            let key = if direction.is_some() {
                delta as i128 * sign
            } else {
                (delta as i128).abs()
            };
            Some((key, (k.clone(), d, nv, ov, delta, class(nv, ov))))
        })
        .collect();
    let count = match direction {
        Some(d) if d != 0 => rows
            .iter()
            .filter(|(_, r)| r.4 as i128 * d.signum() as i128 > 0)
            .count() as u32,
        _ => 0,
    };
    rows.sort_by(|a, b| b.0.cmp(&a.0).then(a.1 .0.cmp(&b.1 .0)));
    rows.truncate(limit);
    (rows.into_iter().map(|r| r.1).collect(), count)
}

#[allow(clippy::too_many_arguments)]
// ---- The previous implementation: ATTACH + join, run directly via rusqlite (v1/v1 only). ----
fn sql_reference(
    new_file: &std::path::Path,
    old_file: &std::path::Path,
    n: Option<&DirRow>,
    o: Option<&DirRow>,
    m: DiffMetric,
    depth: u32,
    limit: usize,
    direction: Option<i64>,
) -> (Vec<Flat>, u32) {
    let column = match m {
        DiffMetric::Disk => "t_disk",
        DiffMetric::Reclaimable => "t_reclaimable",
        DiffMetric::Files => "t_files",
    };
    let children = direction.is_some();
    let (n_filter, o_filter, order) = if children {
        (
            "n.parent_id = :n_lo",
            "o2.parent_id = :o_lo",
            "delta * :sign DESC, path",
        )
    } else {
        (
            "n.id BETWEEN :n_lo AND :n_hi AND n.depth <= :n_depth",
            "o2.id BETWEEN :o_lo AND :o_hi AND o2.depth <= :o_depth",
            "ABS(delta) DESC, path",
        )
    };
    let union = format!(
        "SELECT n.path, n.depth - :n_base AS depth_rel,
                n.{column} AS new_value, o2.{column} AS old_value,
                n.{column} - COALESCE(o2.{column}, 0) AS delta
         FROM main.dirs n LEFT JOIN o.dirs o2
           ON o2.path_hash = n.path_hash AND o2.path = n.path
         WHERE {n_filter}
         UNION ALL
         SELECT o2.path, o2.depth - :o_base, NULL, o2.{column}, -o2.{column}
         FROM o.dirs o2 WHERE {o_filter}
           AND NOT EXISTS (SELECT 1 FROM main.dirs n
                           WHERE n.path_hash = o2.path_hash AND n.path = o2.path)"
    );
    let limited = format!(
        "SELECT path, depth_rel, new_value, old_value FROM ({union}) ORDER BY {order} LIMIT :limit"
    );
    let aggregate = format!("SELECT COUNT(*) FROM ({union}) WHERE delta * :count_sign > 0");
    let conn = rusqlite::Connection::open_with_flags(
        new_file,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_URI,
    )
    .unwrap();
    conn.execute(
        "ATTACH DATABASE ?1 AS o",
        [format!("file:{}?mode=ro&immutable=1", old_file.display())],
    )
    .unwrap();
    for schema in ["main", "o"] {
        conn.pragma_update(Some(schema), "mmap_size", 1i64 << 30)
            .unwrap();
    }
    let bind = |st: &mut rusqlite::Statement<'_>| {
        let mut b = |name: &str, v: i64| {
            if let Some(i) = st.parameter_index(name).unwrap() {
                st.raw_bind_parameter(i, v).unwrap();
            }
        };
        for (p, r) in [("n", n), ("o", o)] {
            b(&format!(":{p}_lo"), r.map_or(-1, |r| i64::from(r.id)));
            b(
                &format!(":{p}_hi"),
                r.map_or(-1, |r| i64::from(r.subtree_end)),
            );
            let base = r.map_or(0, |r| i64::from(r.depth));
            b(&format!(":{p}_base"), base);
            b(&format!(":{p}_depth"), base + i64::from(depth));
        }
        b(":limit", limit as i64);
        b(
            ":sign",
            if direction.is_some_and(|d| d < 0) {
                -1
            } else {
                1
            },
        );
        b(":count_sign", direction.map_or(0, i64::signum));
    };
    let count = if children {
        let mut st = conn.prepare(&aggregate).unwrap();
        bind(&mut st);
        let mut q = st.raw_query();
        let count = q.next().unwrap().unwrap().get::<_, u32>(0).unwrap();
        count
    } else {
        0
    };
    let mut st = conn.prepare(&limited).unwrap();
    bind(&mut st);
    let mut q = st.raw_query();
    let mut rows = vec![];
    while let Some(r) = q.next().unwrap() {
        let nv: Option<i64> = r.get(2).unwrap();
        let ov: Option<i64> = r.get(3).unwrap();
        let (nv, ov) = (nv.map(|x| x as u64), ov.map(|x| x as u64));
        let delta = nv.unwrap_or(0) as i64 - ov.unwrap_or(0) as i64;
        rows.push((
            r.get(0).unwrap(),
            r.get(1).unwrap(),
            nv,
            ov,
            delta,
            class(nv, ov),
        ));
    }
    (rows, count)
}

struct Case {
    _dir: tempfile::TempDir,
    new_file: PathBuf,
    old_file: PathBuf,
    new: Snapshot,
    old: Snapshot,
    old_tree: Tree,
    new_tree: Tree,
}

fn case(seed: u64, nk: Kind, ok: Kind, size: usize) -> Case {
    let mut rng = Rng(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) | 1);
    let old_tree = gen_tree(&mut rng, size);
    let new_tree = mutate(&mut rng, &old_tree);
    let dir = test_support::private_tempdir().unwrap();
    let old_file = write(dir.path(), ROOT, &old_tree, 1000, ok, seed);
    let new_file = write(dir.path(), ROOT, &new_tree, 2000, nk, seed + 1);
    Case {
        new: open(&new_file),
        old: open(&old_file),
        _dir: dir,
        new_file,
        old_file,
        old_tree,
        new_tree,
    }
}

fn targets(c: &Case, rng: &mut Rng) -> Vec<RawPath> {
    let mut v = vec![path_of(ROOT, &vec![])];
    let o: Vec<_> = c.old_tree.keys().cloned().collect();
    let n: Vec<_> = c.new_tree.keys().cloned().collect();
    v.push(path_of(ROOT, &o[rng.below(o.len())]));
    v.push(path_of(ROOT, &n[rng.below(n.len())]));
    // Present on one side only, if any.
    if let Some(k) = n.iter().find(|k| !c.old_tree.contains_key(*k)) {
        v.push(path_of(ROOT, k));
    }
    if let Some(k) = o.iter().find(|k| !c.new_tree.contains_key(*k)) {
        v.push(path_of(ROOT, k));
    }
    v
}

const KINDS: [(Kind, Kind); 6] = [
    (Kind::V1, Kind::V1),
    (Kind::V2, Kind::V1),
    (Kind::V1, Kind::V2),
    (Kind::V2, Kind::V2),
    (Kind::ColdV1, Kind::V2),
    (Kind::V2, Kind::ColdV2),
];
const METRICS: [DiffMetric; 3] = [DiffMetric::Disk, DiffMetric::Reclaimable, DiffMetric::Files];

#[test]
fn diff_matches_model_and_old_sql_for_every_pair_kind() {
    let mut seen = [0usize; 5];
    for seed in 1..=3u64 {
        for (nk, ok) in KINDS {
            let c = case(seed, nk, ok, 40);
            let mut rng = Rng(seed + 77);
            for target in targets(&c, &mut rng) {
                let n = c.new.dir_by_path(&target).unwrap();
                let o = c.old.dir_by_path(&target).unwrap();
                for m in METRICS {
                    for depth in [0, 1, 2, 100, u32::MAX] {
                        for limit in [0, 1, 3, 1000, CAP] {
                            let got: Vec<_> = diff(&c.new, &c.old_file, &target, m, depth, limit)
                                .unwrap()
                                .iter()
                                .map(flat)
                                .collect();
                            let (want, _) = model(&c.new, &c.old, &target, m, depth, limit, None);
                            for r in &got {
                                seen[r.5 as usize] += 1;
                            }
                            assert_eq!(
                                got, want,
                                "{seed} {nk:?}/{ok:?} {target:?} {m:?} d{depth} l{limit}"
                            );
                            if nk == Kind::V1 && ok == Kind::V1 {
                                let sql = sql_reference(
                                    &c.new_file,
                                    &c.old_file,
                                    n.as_ref(),
                                    o.as_ref(),
                                    m,
                                    depth,
                                    limit,
                                    None,
                                );
                                assert_eq!(
                                    got, sql.0,
                                    "sql {seed} {target:?} {m:?} d{depth} l{limit}"
                                );
                            }
                        }
                    }
                }
            }
        }
    }
    assert!(
        seen.iter().all(|n| *n > 0),
        "every class must be exercised: {seen:?}"
    );
}

#[test]
fn explain_matches_reference_loop_for_every_pair_kind() {
    for seed in 1..=2u64 {
        for (nk, ok) in KINDS {
            let c = case(seed, nk, ok, 40);
            let mut rng = Rng(seed + 5);
            for target in targets(&c, &mut rng) {
                for m in METRICS {
                    for (ratio, min, top) in [(0.3, 0, 0), (0.5, 1, 1), (0.8, 0, 3), (1.0, 2, 10)] {
                        let got =
                            explain(&c.new, &c.old_file, &target, m, ratio, min, top).unwrap();
                        // Reference loop on the model / old SQL.
                        let sql_ok = nk == Kind::V1 && ok == Kind::V1;
                        let mut path = target.clone();
                        let mut steps: Vec<(Vec<u8>, i64)> = vec![];
                        let value = |s: &Snapshot, p: &RawPath| s.dir_by_path(p).unwrap();
                        let d = |a: Option<u64>, b: Option<u64>| {
                            a.unwrap_or(0) as i64 - b.unwrap_or(0) as i64
                        };
                        let (mut nn, mut oo) = (value(&c.new, &path), value(&c.old, &path));
                        let target_delta = d(
                            nn.as_ref().map(|r| val(r, m)),
                            oo.as_ref().map(|r| val(r, m)),
                        );
                        let mut cur = target_delta;
                        let (spread, spread_children, own_delta) = loop {
                            let (mut rows, count) =
                                model(&c.new, &c.old, &path, m, 1, top.max(1), Some(cur));
                            if sql_ok {
                                let s = sql_reference(
                                    &c.new_file,
                                    &c.old_file,
                                    nn.as_ref(),
                                    oo.as_ref(),
                                    m,
                                    1,
                                    top.max(1),
                                    Some(cur),
                                );
                                assert_eq!((&rows, count), (&s.0, s.1));
                            }
                            let dom = rows.first().filter(|r| {
                                cur != 0
                                    && r.4.signum() == cur.signum()
                                    && r.4.unsigned_abs() >= min
                                    && r.4.unsigned_abs() as f64
                                        >= ratio * cur.unsigned_abs() as f64
                            });
                            if let Some(r) = dom {
                                steps.push((r.0.clone(), r.4));
                                path = RawPath::new(r.0.clone()).unwrap();
                                cur = r.4;
                                nn = value(&c.new, &path);
                                oo = value(&c.old, &path);
                            } else {
                                rows.truncate(top);
                                break (
                                    rows,
                                    count,
                                    d(
                                        nn.as_ref().map(|r| r.own_charged),
                                        oo.as_ref().map(|r| r.own_charged),
                                    ),
                                );
                            }
                        };
                        let ctx =
                            format!("{seed} {nk:?}/{ok:?} {target:?} {m:?} {ratio} {min} {top}");
                        assert_eq!(got.target_delta, target_delta, "{ctx}");
                        assert_eq!(
                            got.steps
                                .iter()
                                .map(|s| (s.path.as_bytes().to_vec(), s.delta))
                                .collect::<Vec<_>>(),
                            steps,
                            "{ctx}"
                        );
                        assert_eq!(got.own_delta, own_delta, "{ctx}");
                        assert_eq!(got.spread_children, spread_children, "{ctx}");
                        assert_eq!(
                            got.spread.iter().map(flat).collect::<Vec<_>>(),
                            spread,
                            "{ctx}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn errors_and_missing_targets() {
    let c = case(9, Kind::V2, Kind::V1, 20);
    let absent = path_of(ROOT, &vec![b"nope".to_vec()]);
    let root = path_of(ROOT, &vec![]);
    assert!(diff(&c.new, &c.old_file, &absent, DiffMetric::Disk, 3, 10).is_err());
    assert!(explain(&c.new, &c.old_file, &absent, DiffMetric::Disk, 0.5, 0, 3).is_err());
    assert!(diff(&c.new, &c.old_file, &root, DiffMetric::Disk, 3, CAP + 1).is_err());
    assert!(explain(
        &c.new,
        &c.old_file,
        &root,
        DiffMetric::Disk,
        0.5,
        0,
        CAP + 1
    )
    .is_err());
    for ratio in [0.0, -1.0, 1.5, f64::NAN] {
        assert!(explain(&c.new, &c.old_file, &root, DiffMetric::Disk, ratio, 0, 3).is_err());
    }
    // Different roots, any layout.
    let dir = test_support::private_tempdir().unwrap();
    for kind in [Kind::V1, Kind::V2] {
        let other = write(
            dir.path(),
            b"/other",
            &c.new_tree,
            3000 + kind as i64,
            kind,
            0,
        );
        let err = diff(&open(&other), &c.old_file, &root, DiffMetric::Disk, 3, 10)
            .err()
            .unwrap();
        assert!(err.to_string().contains("roots differ"), "{err}");
    }
}

#[test]
fn ties_order_by_raw_path_bytes_not_preorder() {
    // a/x sorts after a-c by path bytes ('/' > '-') but before it in preorder.
    let mut old = Tree::new();
    let mut new = Tree::new();
    for t in [&mut old, &mut new] {
        t.insert(vec![], 10);
        t.insert(vec![b"a".to_vec()], 5);
        t.insert(vec![b"a".to_vec(), b"x".to_vec()], 2);
        t.insert(vec![b"a-c".to_vec()], 2);
    }
    for k in [
        vec![],
        vec![b"a".to_vec()],
        vec![b"a".to_vec(), b"x".to_vec()],
        vec![b"a-c".to_vec()],
    ] {
        *new.get_mut(&k).unwrap() += 1;
    }
    let dir = test_support::private_tempdir().unwrap();
    let o = write(dir.path(), ROOT, &old, 1, Kind::V2, 1);
    let n = write(dir.path(), ROOT, &new, 2, Kind::V1, 0);
    let rows = diff(
        &open(&n),
        &o,
        &path_of(ROOT, &vec![]),
        DiffMetric::Disk,
        9,
        10,
    )
    .unwrap();
    let paths: Vec<_> = rows.iter().map(|r| r.path.as_bytes().to_vec()).collect();
    let mut sorted = paths.clone();
    sorted.sort();
    assert_eq!(paths, sorted);
    assert!(
        paths.iter().position(|p| p.ends_with(b"a-c"))
            < paths.iter().position(|p| p.ends_with(b"a/x"))
    );
}

#[test]
fn wide_directory_crosses_page_boundaries() {
    // More children than the first pages hold, with interleaved add/remove/change.
    let mut old = Tree::new();
    let mut new = Tree::new();
    old.insert(vec![], 1);
    new.insert(vec![], 1);
    for i in 0..1500u32 {
        let name = format!("d{i:05}").into_bytes();
        if i % 3 != 0 {
            old.insert(vec![name.clone()], u64::from(i % 7));
        }
        if i % 5 != 0 {
            new.insert(vec![name.clone()], u64::from(i % 11));
        }
        if i % 100 == 0 {
            old.insert(vec![name.clone(), b"k".to_vec()], 3);
            new.insert(vec![name, b"k".to_vec()], 4);
            // parents must exist
        }
    }
    for t in [&mut old, &mut new] {
        let all: Vec<_> = t.keys().cloned().collect();
        for c in all {
            for l in 0..c.len() {
                t.entry(c[..l].to_vec()).or_insert(1);
            }
        }
    }
    let dir = test_support::private_tempdir().unwrap();
    for (nk, ok) in [
        (Kind::V1, Kind::V1),
        (Kind::V2, Kind::V2),
        (Kind::V2, Kind::V1),
    ] {
        let o = write(
            dir.path(),
            ROOT,
            &old,
            10 + nk as i64 * 10 + ok as i64,
            ok,
            3,
        );
        let n = write(
            dir.path(),
            ROOT,
            &new,
            100 + nk as i64 * 10 + ok as i64,
            nk,
            2,
        );
        let (ns, os) = (open(&n), open(&o));
        let t = path_of(ROOT, &vec![]);
        let got: Vec<_> = diff(&ns, &o, &t, DiffMetric::Disk, 5, 100_000)
            .unwrap()
            .iter()
            .map(flat)
            .collect();
        let (want, _) = model(&ns, &os, &t, DiffMetric::Disk, 5, 100_000, None);
        assert_eq!(got, want);
        if nk == Kind::V1 && ok == Kind::V1 {
            let (nr, or) = (ns.dir_by_path(&t).unwrap(), os.dir_by_path(&t).unwrap());
            assert_eq!(
                got,
                sql_reference(
                    &n,
                    &o,
                    nr.as_ref(),
                    or.as_ref(),
                    DiffMetric::Disk,
                    5,
                    100_000,
                    None
                )
                .0
            );
        }
        let e = explain(&ns, &o, &t, DiffMetric::Disk, 1.0, 0, 5).unwrap();
        let (rows, count) = model(&ns, &os, &t, DiffMetric::Disk, 1, 5, Some(e.target_delta));
        if e.steps.is_empty() {
            assert_eq!(e.spread.iter().map(flat).collect::<Vec<_>>(), rows);
            assert_eq!(e.spread_children, count);
        }
    }
}

/// `cargo test --release -p tidemark-store --test diff_walk -- --ignored --nocapture scale`
/// Streams a 2,000,101-directory tree (100 x 20 x 999) and a copy with 1% of directories changed,
/// then times the old ATTACH+join SQL (v1/v1) against the walk (v1/v1, v2/v2).
#[test]
#[ignore = "scale"]
fn scale_diff_old_sql_vs_walk() {
    const A: u32 = 100;
    const B: u32 = 20;
    const C: u32 = 999;
    let per_a = 1 + B * (1 + C);
    let build = |dir: &std::path::Path, kind: Kind, changed: bool, now: i64| -> PathBuf {
        let mut m = meta(RawPath::new(ROOT.to_vec()).unwrap(), now);
        m.schema_version = if kind == Kind::V2 { 2 } else { 1 };
        m.dirs = u64::from(1 + A * per_a);
        enum W {
            V1(SnapshotWriter),
            V2(SnapshotWriterV2),
        }
        let mut w = if kind == Kind::V2 {
            W::V2(SnapshotWriterV2::create(dir, &m).unwrap())
        } else {
            W::V1(SnapshotWriter::create(dir, &m).unwrap())
        };
        let mut batch: Vec<DirRow> = Vec::with_capacity(10_000);
        let flush = |batch: &mut Vec<DirRow>, w: &mut W| {
            match w {
                W::V1(w) => w.put_dirs(batch).unwrap(),
                W::V2(w) => w.put_dirs(batch).unwrap(),
            }
            batch.clear();
        };
        let mk = |id: u32, parent: Option<u32>, comps: &[u32], end: u32| {
            let mut c: Comps = vec![];
            for (i, x) in comps.iter().enumerate() {
                c.push(format!("{}{x:05}", ["d", "e", "f"][i]).into_bytes());
            }
            let p = path_of(ROOT, &c);
            let mut r = row(id, parent, p.as_bytes(), 1000 + u64::from(id % 97));
            if changed && id % 100 == 7 {
                r.t_disk += 1 + u64::from(id % 13);
            }
            r.depth = comps.len() as u32;
            r.subtree_end = end;
            r.name = c.last().cloned().unwrap_or_else(|| p.as_bytes().to_vec());
            r
        };
        batch.push(mk(0, None, &[], A * per_a));
        let mut id = 1;
        for a in 0..A {
            let a_id = id;
            batch.push(mk(id, Some(0), &[a], a_id + per_a - 1));
            id += 1;
            for b in 0..B {
                let b_id = id;
                batch.push(mk(id, Some(a_id), &[a, b], b_id + C));
                id += 1;
                for c in 0..C {
                    batch.push(mk(id, Some(b_id), &[a, b, c], id));
                    id += 1;
                    if batch.len() >= 10_000 {
                        flush(&mut batch, &mut w);
                    }
                }
            }
        }
        flush(&mut batch, &mut w);
        match w {
            W::V1(w) => w.finish(&m).unwrap(),
            W::V2(w) => w.finish(&m).unwrap(),
        }
    };
    let dir = test_support::private_tempdir().unwrap();
    let root = path_of(ROOT, &vec![]);
    let time = |label: &str, f: &mut dyn FnMut() -> usize| {
        let mut ms = vec![];
        let mut rows = 0;
        for _ in 0..5 {
            let t = std::time::Instant::now();
            rows = f();
            ms.push(t.elapsed().as_secs_f64() * 1000.0);
        }
        ms.sort_by(|a, b| a.partial_cmp(b).unwrap());
        eprintln!(
            "{label}: rows={rows} median={:.1}ms min={:.1}ms max={:.1}ms",
            ms[2], ms[0], ms[4]
        );
    };
    for (kind, label) in [(Kind::V1, "v1/v1"), (Kind::V2, "v2/v2")] {
        let o = build(dir.path(), kind, false, 1000 + kind as i64);
        let n = build(dir.path(), kind, true, 2000 + kind as i64);
        let ns = open(&n);
        if kind == Kind::V1 {
            let os = open(&o);
            let (nr, or) = (
                ns.dir_by_path(&root).unwrap(),
                os.dir_by_path(&root).unwrap(),
            );
            let mut old_rows = vec![];
            time("v1/v1 old SQL (ATTACH join)", &mut || {
                old_rows = sql_reference(
                    &n,
                    &o,
                    nr.as_ref(),
                    or.as_ref(),
                    DiffMetric::Disk,
                    u32::MAX,
                    50,
                    None,
                )
                .0;
                old_rows.len()
            });
            let got: Vec<_> = diff(&ns, &o, &root, DiffMetric::Disk, u32::MAX, 50)
                .unwrap()
                .iter()
                .map(flat)
                .collect();
            assert_eq!(got, old_rows);
        }
        time(&format!("{label} walk (warm)"), &mut || {
            diff(&ns, &o, &root, DiffMetric::Disk, u32::MAX, 50)
                .unwrap()
                .len()
        });
        time(
            &format!("{label} walk (fresh Snapshot::open each run)"),
            &mut || {
                diff(&open(&n), &o, &root, DiffMetric::Disk, u32::MAX, 50)
                    .unwrap()
                    .len()
            },
        );
    }
}
