//! Owned by: WSNAP-bench
//! Deterministic synthetic snapshot generator and public-API latency baseline.
#![forbid(unsafe_code)]
use std::{
    error::Error,
    hint::black_box,
    path::{Path, PathBuf},
    time::Instant,
};
use tidemark_core::{
    config::LinkInstances,
    path::{RawName, RawPath},
    scan::{ScanMode, SnapshotStatus},
    snapshot::{DirRow, FileFilter, FileRow, LinkInstanceRow, LinkSetRow, SnapshotMeta, SortKey},
    time::ScanId,
};
use tidemark_store::{
    diff::{diff, DiffMetric},
    export_ncdu,
    writer::SnapshotWriter,
    writer_v2::SnapshotWriterV2,
    NcduOptions, Snapshot,
};
type Result<T> = std::result::Result<T, Box<dyn Error>>;
const NOW: i64 = 1_780_000_000;
const RUNS: usize = 5;
const QUERIES: &[&str] = &[
    "children_root",
    "children_wide",
    "top2",
    "top4",
    "files_root",
    "files_subtree",
    "path12",
    "diff",
    "ncdu",
];
struct Options {
    dirs: usize,
    files: usize,
    links: usize,
    seed: u64,
    out: PathBuf,
    layout: String,
    query: String,
    json: bool,
}
impl Options {
    fn parse() -> Result<Self> {
        let mut o = Self {
            dirs: 2_000_000,
            files: 200_000,
            links: 500_000,
            seed: 42,
            out: std::env::temp_dir().join("tidemark-snapbench"),
            layout: "v1".into(),
            query: "all".into(),
            json: false,
        };
        let mut args = std::env::args_os().skip(1);
        while let Some(a) = args.next() {
            if a == "--json" {
                o.json = true;
                continue;
            }
            let v = args.next().ok_or("missing option value")?;
            if a == "--out" {
                o.out = v.into();
                continue;
            }
            let a = a.to_str().ok_or("option name must be UTF-8")?;
            let v = v.into_string().map_err(|_| "option value must be UTF-8")?;
            match a {
                "--dirs" => o.dirs = v.parse()?,
                "--files" => o.files = v.parse()?,
                "--links" => o.links = v.parse()?,
                "--seed" => o.seed = v.parse()?,
                "--layout" => o.layout = v,
                "--query" => o.query = v,
                _ => return Err(format!("unknown option {a}").into()),
            }
        }
        if !o.out.is_absolute() {
            o.out = std::env::current_dir()?.join(&o.out);
        }
        if o.dirs < 32 || o.dirs > 5_000_000 || o.files > 5_000_000 || o.links > 5_000_000 {
            return Err("caps: dirs 32..=5M, files/links <=5M".into());
        }
        if o.query != "all" && !QUERIES.contains(&o.query.as_str()) {
            return Err("unknown query".into());
        }
        match o.layout.as_str() {
            "v1" | "v2" => (),
            _ => return Err("supported layout: v1, v2".into()),
        }
        Ok(o)
    }
}
struct Rng(u64);
impl Rng {
    fn next(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e3779b97f4a7c15);
        let mut z = self.0;
        z = (z ^ (z >> 30)).wrapping_mul(0xbf58476d1ce4e5b9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94d049bb133111eb);
        z ^ (z >> 31)
    }
    fn size(&mut self) -> u64 {
        let u = (self.next() as f64 + 1.0) / (u64::MAX as f64 + 2.0);
        let v = (self.next() as f64 + 1.0) / (u64::MAX as f64 + 2.0);
        let z = (-2.0 * u.ln()).sqrt() * (std::f64::consts::TAU * v).cos();
        ((14.0 + 2.0 * z).exp().clamp(512.0, 1e12) as u64).div_ceil(512) * 512
    }
    fn mtime(&mut self) -> i64 {
        NOW - (self.next() % (730 * 86400)) as i64
    }
}
fn name(i: usize) -> Result<RawName> {
    // Four base-36 digits preserve numeric sibling order through 1,679,616 entries.
    let mut bytes = Vec::with_capacity(40);
    for shift in [36usize.pow(3), 36usize.pow(2), 36, 1] {
        bytes.push(b"0123456789abcdefghijklmnopqrstuvwxyz"[(i / shift) % 36]);
    }
    let suffix: &[u8] = [
        b"".as_slice(),
        b"-src",
        b"-2024-archive",
        b"-results-long-repeated-component-data",
    ][i % 4];
    bytes.extend_from_slice(suffix);
    Ok(RawName::new(bytes)?)
}

fn row(id: usize, parent: Option<&DirRow>, component: RawName, mtime: i64) -> Result<DirRow> {
    let path = match parent {
        Some(p) => p.path.join(&component),
        None => RawPath::new(b"/synthetic".to_vec())?,
    };
    Ok(DirRow {
        id: id as u32,
        parent_id: parent.map(|p| p.id),
        subtree_end: id as u32,
        depth: parent.map_or(0, |p| p.depth + 1),
        name: if parent.is_none() {
            path.as_bytes().to_vec()
        } else {
            component.as_bytes().to_vec()
        },
        path_hash: path.hash64(),
        path,
        ino: id as u64 + 1,
        mtime,
        dir_disk: 4096,
        dir_apparent: 4096,
        projid: None,
        entries: 0,
        errors: 0,
        own_files: 0,
        own_symlinks: 0,
        own_charged: 4096,
        own_apparent: 4096,
        t_dirs: 1,
        t_files: 0,
        t_inodes: 0,
        t_disk: 4096,
        t_apparent: 4096,
        t_charged: 4096,
        t_reclaimable: 4096,
        t_errors: 0,
        age0: 0,
        age1: 0,
        age2: 0,
        age3: 0,
        age4: 0,
        age5: 0,
        newest_mtime: mtime,
        max_entries: 0,
    })
}
fn age(r: &mut DirRow, bytes: u64, mtime: i64) {
    let days = (NOW - mtime) / 86400;
    let bucket = match days {
        ..1 => &mut r.age0,
        1..7 => &mut r.age1,
        7..30 => &mut r.age2,
        30..90 => &mut r.age3,
        90..365 => &mut r.age4,
        _ => &mut r.age5,
    };
    *bucket += bytes;
}
struct Data {
    dirs: Vec<DirRow>,
    files: Vec<FileRow>,
    sets: Vec<LinkSetRow>,
    links: Vec<LinkInstanceRow>,
    wide: u32,
    small: u32,
    deep: RawPath,
}
fn generate(o: &Options, changed: bool) -> Result<Data> {
    let mut rng = Rng(o.seed);
    let mut dirs = Vec::with_capacity(o.dirs);
    dirs.push(row(0, None, name(0)?, rng.mtime())?);
    // Explicit work stack: root groups, then six-deep stems and broad leaf fans.
    let mut groups = Vec::new();
    let mut left = o.dirs - 1;
    for size in [
        100_002.min((o.dirs - 13) / 4),
        100_002.min((o.dirs - 13) / 4),
        1000.min((o.dirs - 13) / 2),
        12,
    ] {
        groups.push(size);
        left -= size;
    }
    while left > 0 {
        let size = left.min(1000);
        groups.push(size);
        left -= size;
    }
    let mut pending = Vec::new();
    for (i, size) in groups.into_iter().enumerate().rev() {
        pending.push((
            0usize,
            size,
            if i < 2 {
                1
            } else if i == 3 {
                12
            } else {
                6
            },
            i,
        ));
    }
    let mut wide = 1;
    let mut small = 0;
    while let Some((parent, size, stem, index)) = pending.pop() {
        let id = dirs.len();
        let mut r = row(id, Some(&dirs[parent]), name(index)?, rng.mtime())?;
        r.subtree_end = (id + size - 1) as u32;
        dirs[parent].entries += 1;
        if size == 1000 && small == 0 {
            small = id as u32;
        }
        if size > 100_000 {
            wide = id as u32;
        }
        dirs.push(r);
        if size > 1 {
            if stem > 1 {
                pending.push((id, size - 1, stem - 1, 0));
            } else {
                for j in (0..size - 1).rev() {
                    pending.push((id, 1, 0, j));
                }
            }
        }
    }
    let deep = dirs
        .iter()
        .find(|r| r.depth == 12)
        .ok_or("missing depth12")?
        .path
        .clone();
    let mut files = Vec::with_capacity(o.files);
    let mut sets = Vec::with_capacity(o.links.div_ceil(2));
    let mut links = Vec::with_capacity(o.links);
    for i in 0..o.files {
        let d = (rng.next() as usize) % dirs.len();
        let disk = rng.size();
        let mtime = rng.mtime();
        let r = &mut dirs[d];
        r.own_files += 1;
        r.entries += 1;
        r.own_charged += disk;
        r.own_apparent += disk;
        r.t_inodes += 1;
        age(r, disk, mtime);
        r.newest_mtime = r.newest_mtime.max(mtime);
        files.push(FileRow {
            id: i as u32,
            dir_id: d as u32,
            name: Some(RawName::new(format!("file-{i:08}").into_bytes())?),
            ino: o.dirs as u64 + i as u64 + 1,
            disk,
            apparent: disk,
            mtime,
            nlink: 1,
        });
    }
    // Same-directory hard-link pairs model complete sets without cross-branch union ambiguity.
    for i in (0..o.links).step_by(2) {
        let d = (rng.next() as usize) % dirs.len();
        let found = (o.links - i).min(2) as u32;
        let disk = rng.size();
        let mtime = rng.mtime();
        let ino = (o.dirs + o.files + i + 1) as u64;
        let r = &mut dirs[d];
        r.entries += found;
        r.own_files += u64::from(found);
        r.own_charged += disk;
        r.own_apparent += disk;
        r.t_inodes += 1;
        age(r, disk, mtime);
        r.newest_mtime = r.newest_mtime.max(mtime);
        sets.push(LinkSetRow {
            ino,
            nlink: 2,
            found,
            complete: found == 2,
            disk,
            apparent: disk,
            mtime,
            lca_dir_id: d as u32,
        });
        for j in 0..found {
            links.push(LinkInstanceRow {
                ino,
                dir_id: d as u32,
                name: Some(RawName::new(format!("link-{i:08}-{j}").into_bytes())?),
            });
        }
    }
    // Start local totals then roll up in reverse preorder. All pairs are local.
    for r in &mut dirs {
        age(r, 4096, r.mtime);
        r.t_disk = r.own_charged;
        r.t_apparent = r.own_apparent;
        r.t_charged = r.own_charged;
        r.t_reclaimable = r.own_charged;
        r.t_files = r.own_files;
        r.max_entries = r.entries;
        r.t_errors = 0;
        if changed && r.id % 100 == 0 {
            r.dir_disk += 512;
            r.dir_apparent += 512;
            r.own_charged += 512;
            r.own_apparent += 512;
            r.t_disk += 512;
            r.t_apparent += 512;
            r.t_charged += 512;
            r.t_reclaimable += 512;
            age(r, 512, r.mtime);
        }
    }
    for set in &sets {
        if !set.complete {
            dirs[set.lca_dir_id as usize].t_reclaimable -= set.disk;
        }
    }
    // Rename two leaf subtrees in the comparison: paths disappear and appear, IDs stay valid.
    if changed {
        let leaves: Vec<usize> = dirs
            .iter()
            .enumerate()
            .filter(|(_, r)| r.subtree_end == r.id && r.depth == 7)
            .map(|(i, _)| i)
            .take(2)
            .collect();
        for i in leaves {
            let p = dirs[i].parent_id.ok_or("leaf parent")? as usize;
            let mut n = dirs[i].name.clone();
            n.extend_from_slice(b"-new");
            let n = RawName::new(n)?;
            dirs[i].path = dirs[p].path.join(&n);
            dirs[i].name = n.as_bytes().to_vec();
            dirs[i].path_hash = dirs[i].path.hash64();
        }
    }
    for i in (1..dirs.len()).rev() {
        let p = dirs[i].parent_id.ok_or("parent")? as usize;
        let (head, tail) = dirs.split_at_mut(i);
        let c = &tail[0];
        let r = &mut head[p];
        r.t_dirs += c.t_dirs;
        r.t_files += c.t_files;
        r.t_inodes += c.t_inodes;
        r.t_disk += c.t_disk;
        r.t_apparent += c.t_apparent;
        r.t_charged += c.t_charged;
        r.t_reclaimable += c.t_reclaimable;
        r.age0 += c.age0;
        r.age1 += c.age1;
        r.age2 += c.age2;
        r.age3 += c.age3;
        r.age4 += c.age4;
        r.age5 += c.age5;
        r.newest_mtime = r.newest_mtime.max(c.newest_mtime);
        r.max_entries = r.max_entries.max(c.max_entries);
    }
    dirs[0].subtree_end = (dirs.len() - 1) as u32;
    Ok(Data {
        dirs,
        files,
        sets,
        links,
        wide,
        small,
        deep,
    })
}
fn meta(o: &Options, d: &Data, changed: bool) -> Result<SnapshotMeta> {
    Ok(SnapshotMeta {
        schema_version: if o.layout == "v2" { 2 } else { 1 },
        tool_version: "snapbench-v1".into(),
        scan_id: ScanId::new(NOW + i64::from(changed))?,
        root: d.dirs[0].path.clone(),
        root_display: d.dirs[0].path.display_lossy(),
        mode: ScanMode::Walk,
        mode_reason: "synthetic".into(),
        status: SnapshotStatus::Complete,
        degraded: false,
        exclude: vec![],
        link_instances: LinkInstances::All,
        started_at: NOW,
        finished_at: NOW,
        host: "synthetic".into(),
        kernel: "synthetic".into(),
        os_release: "synthetic".into(),
        fs_uuid: String::new(),
        fs_dev: 0,
        fs_blocksize: 4096,
        threads: 1,
        throttle_profile: "max".into(),
        dirs: d.dirs.len() as u64,
        files: d.dirs[0].t_files,
        inodes: d.dirs[0].t_inodes,
        errors: 0,
        races: 0,
        skipped: 0,
        noatime_fallbacks: 0,
        phase_ms: Default::default(),
        gate_stats: Default::default(),
    })
}
enum W {
    V1(SnapshotWriter),
    V2(SnapshotWriterV2),
}
macro_rules! put {
    ($w:expr, $f:ident, $rows:expr) => {
        match $w {
            W::V1(w) => w.$f($rows)?,
            W::V2(w) => w.$f($rows)?,
        }
    };
}
fn write(o: &Options, d: &Data, changed: bool) -> Result<PathBuf> {
    let m = meta(o, d, changed)?;
    let mut w = match o.layout.as_str() {
        "v1" => W::V1(SnapshotWriter::create(&o.out, &m)?),
        "v2" => W::V2(SnapshotWriterV2::create(&o.out, &m)?),
        _ => return Err("unsupported layout".into()),
    };
    for batch in d.dirs.chunks(4096) {
        put!(&mut w, put_dirs, batch);
    }
    for batch in d.files.chunks(4096) {
        put!(&mut w, put_files, batch);
    }
    for batch in d.sets.chunks(4096) {
        put!(&mut w, put_link_sets, batch);
    }
    for batch in d.links.chunks(4096) {
        put!(&mut w, put_link_instances, batch);
    }
    Ok(match w {
        W::V1(w) => w.finish(&m)?,
        W::V2(w) => w.finish(&m)?,
    })
}
struct Targets {
    wide: u32,
    small: u32,
    deep: RawPath,
    root: RawPath,
}
fn query(s: &Snapshot, old: &Path, t: &Targets, q: &str) -> Result<()> {
    match q {
        "children_root" | "children_wide" => {
            black_box(s.children_page(
                if q == "children_root" { 0 } else { t.wide },
                SortKey::Disk,
                true,
                None,
                false,
                50,
                b"",
            )?);
        }
        "top2" | "top4" => {
            black_box(s.subtree_dirs(
                0,
                if q == "top2" { 2 } else { 4 },
                SortKey::Disk,
                1_000_001,
            )?);
        }
        "files_root" | "files_subtree" => {
            black_box(s.subtree_files(
                if q == "files_root" { 0 } else { t.small },
                &FileFilter::default(),
                50,
            )?);
        }
        "path12" => {
            black_box(s.dir_by_path(&t.deep)?.ok_or("depth12 lookup absent")?);
        }
        "diff" => {
            black_box(diff(s, old, &t.root, DiffMetric::Disk, u32::MAX, 50)?);
        }
        "ncdu" => {
            black_box(export_ncdu(
                s,
                &t.root,
                &mut std::io::sink(),
                &NcduOptions {
                    utf8_lossy: false,
                    max_depth: None,
                },
            )?);
        }
        _ => return Err("unknown query".into()),
    }
    Ok(())
}
// Add the v2/cold-file reader path here without changing query bodies.
fn open(layout: &str, path: &Path) -> Result<Snapshot> {
    match layout {
        "v1" | "v2" => Ok(Snapshot::open(path)?),
        _ => Err("unsupported layout".into()),
    }
}
fn rss() -> Result<u64> {
    let status = std::fs::read_to_string("/proc/self/status")?;
    Ok(status
        .lines()
        .find(|l| l.starts_with("VmHWM:"))
        .ok_or("VmHWM absent")?
        .split_whitespace()
        .nth(1)
        .ok_or("VmHWM value")?
        .parse()?)
}
fn main() -> Result<()> {
    let o = Options::parse()?;
    eprintln!("Generating {} directories", o.dirs);
    let d = generate(&o, false)?;
    let t = Targets {
        wide: d.wide,
        small: d.small,
        deep: d.deep.clone(),
        root: d.dirs[0].path.clone(),
    };
    let wide_children = d.dirs[t.wide as usize].entries as u64 - d.dirs[t.wide as usize].own_files;
    let small_dirs = d.dirs[t.small as usize].t_dirs;
    let max_depth = d.dirs.iter().map(|r| r.depth).max().ok_or("empty tree")?;
    let mut depths: Vec<u32> = d.dirs.iter().map(|r| r.depth).collect();
    depths.sort_unstable();
    let median_depth = depths[depths.len() / 2];
    drop(depths);
    let old = write(&o, &d, false)?;
    drop(d);
    eprintln!("Generating comparison");
    let d = generate(&o, true)?;
    let new = write(&o, &d, true)?;
    drop(d);
    let mut measurements = Vec::new();
    eprintln!("query                 mode      median_ms       p95_ms");
    for &q in QUERIES {
        if o.query != "all" && o.query != q {
            continue;
        }
        let source = if q == "diff" { &new } else { &old };
        for cold in [true, false] {
            let warm = if cold {
                None
            } else {
                Some(open(&o.layout, source)?)
            };
            if let Some(s) = &warm {
                query(s, &old, &t, q)?;
            }
            let mut times = Vec::new();
            for _ in 0..RUNS {
                let start = Instant::now();
                if let Some(s) = &warm {
                    query(s, &old, &t, q)?;
                } else {
                    let s = open(&o.layout, source)?;
                    query(&s, &old, &t, q)?;
                }
                times.push(start.elapsed().as_secs_f64() * 1000.0);
            }
            times.sort_by(f64::total_cmp);
            let mode = if cold { "fresh_open" } else { "warm" };
            eprintln!("{q:22} {mode:10} {:12.3} {:12.3}", times[2], times[4]);
            measurements.push(serde_json::json!({"query":q,"mode":mode,"median_ms":times[2],"p95_ms":times[4],"runs":RUNS,"samples_ms":times}));
        }
    }
    let report = serde_json::json!({"layout":o.layout,"seed":o.seed,"dirs":o.dirs,"notable_files":o.files,"link_instances":o.links,"median_depth":median_depth,"max_depth":max_depth,"wide_children":wide_children,"subtree_dirs":small_dirs,"old_bytes":std::fs::metadata(&old)?.len(),"new_bytes":std::fs::metadata(&new)?.len(),"peak_rss_kib":rss()?,"cold_definition":"fresh Snapshot::open included; OS page cache not dropped","measurements":measurements});
    eprintln!(
        "Snapshot bytes: {} + {}; process peak RSS: {} KiB",
        std::fs::metadata(&old)?.len(),
        std::fs::metadata(&new)?.len(),
        rss()?
    );
    std::fs::write(
        o.out.join("report.json"),
        serde_json::to_vec_pretty(&report)?,
    )?;
    if o.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        println!("Report: {}", o.out.join("report.json").display());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    fn options() -> Options {
        Options {
            dirs: 4096,
            files: 300,
            links: 501,
            seed: 42,
            out: PathBuf::new(),
            layout: "v1".into(),
            query: "all".into(),
            json: false,
        }
    }
    #[test]
    fn deterministic_tree_and_link_accounting() -> Result<()> {
        let o = options();
        let a = generate(&o, false)?;
        let b = generate(&o, false)?;
        assert_eq!(a.dirs, b.dirs);
        assert_eq!(a.files, b.files);
        assert_eq!(a.links, b.links);
        assert_eq!(a.dirs.len(), o.dirs);
        assert_eq!(a.dirs[0].t_files, (o.files + o.links) as u64);
        assert_eq!(a.dirs[0].t_inodes, (o.files + o.links.div_ceil(2)) as u64);
        let incomplete = a
            .sets
            .iter()
            .filter(|s| !s.complete)
            .map(|s| s.disk)
            .sum::<u64>();
        assert_eq!(a.dirs[0].t_disk - a.dirs[0].t_reclaimable, incomplete);
        for r in &a.dirs {
            assert_eq!(r.path_hash, r.path.hash64());
            assert_eq!(r.t_dirs, u64::from(r.subtree_end - r.id + 1));
            assert_eq!(
                r.t_charged,
                r.age0 + r.age1 + r.age2 + r.age3 + r.age4 + r.age5
            );
            assert!(r.t_reclaimable <= r.t_charged);
            if let Some(p) = r.parent_id {
                let parent = &a.dirs[p as usize];
                assert!(r.id <= parent.subtree_end);
                assert_eq!(r.depth, parent.depth + 1);
                assert_eq!(parent.path.join(&RawName::new(r.name.clone())?), r.path);
            }
        }
        let mut last = vec![None::<Vec<u8>>; o.dirs];
        for r in &a.dirs {
            if let Some(p) = r.parent_id {
                if let Some(n) = &last[p as usize] {
                    assert!(n < &r.name);
                }
                last[p as usize] = Some(r.name.clone());
            }
        }
        Ok(())
    }
    #[test]
    fn writer_roundtrip_and_every_query() -> Result<()> {
        let tmp = tempfile::tempdir()?;
        let mut o = options();
        o.out = tmp.path().join("snapshots");
        let a = generate(&o, false)?;
        let old = write(&o, &a, false)?;
        let b = generate(&o, true)?;
        let new = write(&o, &b, true)?;
        let t = Targets {
            wide: a.wide,
            small: a.small,
            deep: a.deep.clone(),
            root: a.dirs[0].path.clone(),
        };
        let s = Snapshot::open(&new)?;
        for &q in QUERIES {
            query(&s, &old, &t, q)?;
        }
        let changes = diff(&s, &old, &t.root, DiffMetric::Disk, 20, 10000)?;
        assert!(changes
            .iter()
            .any(|r| r.class == tidemark_store::diff::DiffClass::New));
        assert!(changes
            .iter()
            .any(|r| r.class == tidemark_store::diff::DiffClass::Removed));
        assert_eq!(s.root()?, b.dirs[0]);
        Ok(())
    }
}
