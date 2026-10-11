//! Owned by: W0-C
//! Deterministic bounded abstract trees and walk-record conversion.
use crate::fixture::{materialize, Fixture};
use std::collections::BTreeMap;
use tidemark_core::{
    path::{RawName, RawPath},
    scan::*,
    time::age_bucket,
};

/// Maximum combined directories, inodes, link instances, skips and errors.
pub const MAX_MODEL_ITEMS: usize = 1_000_000;
/// Model directory, with metadata independently editable by tests.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelDir {
    /// Parent index, absent for root.
    pub parent: Option<usize>,
    /// Entry name, absent for root.
    pub name: Option<RawName>,
    /// Directory inode.
    pub ino: u64,
    /// Allocated directory bytes.
    pub disk: u64,
    /// Apparent directory bytes.
    pub apparent: u64,
    /// Directory modification time.
    pub mtime: i64,
    /// Scanner errors attributed to this directory.
    pub errors: Vec<ScanErrorRec>,
    /// Skipped entries counted in direct width.
    pub skipped: Vec<Skip>,
}
/// Metadata of one non-directory inode.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelInode {
    /// Kernel link count (may exceed discovered entries).
    pub nlink: u32,
    /// Entry type.
    pub kind: FileKind,
    /// Allocated bytes.
    pub disk: u64,
    /// Apparent bytes.
    pub apparent: u64,
    /// Modification time.
    pub mtime: i64,
}
/// One non-directory entry; duplicate inode ids represent hard links.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelEntry {
    /// Containing directory index.
    pub dir: usize,
    /// Raw entry name.
    pub name: RawName,
    /// Key in ModelTree::inodes.
    pub ino: u64,
}
/// In-memory tree. Directory indices are discovery ids; parents precede children.
/// Caller-created models must have one root, valid indices and unique entry names.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelTree {
    /// Absolute scan root.
    pub root: RawPath,
    /// Injected scan-start timestamp.
    pub now: i64,
    /// Directory metadata.
    pub dirs: Vec<ModelDir>,
    /// Distinct non-directory inode metadata.
    pub inodes: BTreeMap<u64, ModelInode>,
    /// Link instances.
    pub entries: Vec<ModelEntry>,
}
impl ModelTree {
    /// Directory paths in discovery order, without filesystem access.
    pub fn paths(&self) -> Vec<RawPath> {
        self.validate();
        let mut paths = vec![self.root.clone()];
        for d in self.dirs.iter().skip(1) {
            let (Some(parent), Some(name)) = (d.parent, d.name.as_ref()) else {
                panic!("invalid model directory")
            };
            paths.push(paths[parent].join(name));
        }
        paths
    }
    /// Validate boundedness and structural preconditions of a caller-built model.
    pub fn validate(&self) {
        assert!(!self.dirs.is_empty(), "model needs a root");
        let count = self.dirs.iter().fold(
            self.entries.len().checked_add(self.inodes.len()),
            |count, d| {
                count
                    .and_then(|n| n.checked_add(1))
                    .and_then(|n| n.checked_add(d.errors.len()))
                    .and_then(|n| n.checked_add(d.skipped.len()))
            },
        );
        assert!(
            count.is_some_and(|n| n <= MAX_MODEL_ITEMS),
            "model item cap exceeded"
        );
        for apparent in [false, true] {
            let total = self
                .dirs
                .iter()
                .map(|d| if apparent { d.apparent } else { d.disk })
                .chain(
                    self.inodes
                        .values()
                        .map(|i| if apparent { i.apparent } else { i.disk }),
                )
                .try_fold(0u64, u64::checked_add);
            assert!(total.is_some(), "model byte totals overflow u64");
        }
        assert!(
            self.dirs[0].parent.is_none() && self.dirs[0].name.is_none(),
            "invalid root"
        );
        let mut names = std::collections::BTreeSet::new();
        for (id, d) in self.dirs.iter().enumerate().skip(1) {
            let (Some(p), Some(n)) = (d.parent, d.name.as_ref()) else {
                panic!("nonroot needs parent and name")
            };
            assert!(p < id, "parents must precede children");
            assert!(names.insert((p, n)), "duplicate name");
        }
        for e in &self.entries {
            assert!(
                e.dir < self.dirs.len() && self.inodes.contains_key(&e.ino),
                "invalid entry"
            );
            assert!(names.insert((e.dir, &e.name)), "duplicate name");
            assert!(self.inodes[&e.ino].nlink > 0, "zero nlink");
            assert!(
                self.inodes[&e.ino].kind != FileKind::Dir,
                "directory in non-directory map"
            );
        }
    }
    /// Produce real core walk-mode records; every single-link inode is a notable candidate.
    pub fn to_dir_records(&self) -> Vec<DirRecord> {
        self.validate();
        let mut records: Vec<_> = self
            .dirs
            .iter()
            .enumerate()
            .map(|(id, d)| DirRecord {
                id: DirId(id as u32),
                parent: d.parent.map(|p| DirId(p as u32)),
                name: d.name.clone(),
                ino: d.ino,
                dev: 1,
                mtime: d.mtime,
                dir_disk: d.disk,
                dir_apparent: d.apparent,
                projid: None,
                entries: d.skipped.len() as u32,
                own: OwnStats {
                    newest_mtime: i64::MIN,
                    ..OwnStats::default()
                },
                links: vec![],
                notable: vec![],
                skipped: d.skipped.clone(),
                errors: d.errors.clone(),
                races: 0,
            })
            .collect();
        for d in &self.dirs {
            if let Some(p) = d.parent {
                records[p].entries += 1;
            }
        }
        let mut entries: Vec<_> = self.entries.iter().collect();
        entries.sort_by(|a, b| (a.dir, &a.name).cmp(&(b.dir, &b.name)));
        for e in entries {
            let i = &self.inodes[&e.ino];
            let r = &mut records[e.dir];
            r.entries += 1;
            if i.nlink > 1 {
                r.links.push(LinkRef {
                    ino: e.ino,
                    nlink: i.nlink,
                    kind: i.kind,
                    disk: i.disk,
                    apparent: i.apparent,
                    mtime: i.mtime,
                    name: Some(e.name.clone()),
                });
            } else {
                r.own.files += 1;
                r.own.symlinks += u64::from(i.kind == FileKind::Symlink);
                r.own.disk += i.disk;
                r.own.apparent += i.apparent;
                r.own.age.0[age_bucket(self.now, i.mtime)] += i.disk;
                r.own.newest_mtime = r.own.newest_mtime.max(i.mtime);
                r.notable.push(NotableFile {
                    ino: e.ino,
                    name: Some(e.name.clone()),
                    disk: i.disk,
                    apparent: i.apparent,
                    mtime: i.mtime,
                });
            }
        }
        records
    }
}
/// Distribution for regular inode sizes; endpoints are inclusive.
#[derive(Clone, Debug, PartialEq)]
pub enum SizeDist {
    /// Constant bytes.
    Fixed(u64),
    /// Uniform integer bytes.
    Uniform { min: u64, max: u64 },
}
/// Bounded deterministic generation parameters. `dirs` includes root; `files`
/// counts distinct inodes and `link_sets` selects the first such inodes.
#[derive(Clone, Debug, PartialEq)]
pub struct RandomTreeParams {
    /// Number of directories including root.
    pub dirs: usize,
    /// Number of distinct non-directory inodes.
    pub files: usize,
    /// Maximum directory depth (root depth zero).
    pub max_depth: usize,
    /// Number of hard-linked regular inode sets.
    pub link_sets: usize,
    /// Maximum discovered instances per linked set, minimum two.
    pub max_links_per_set: usize,
    /// Probability a linked inode has one additional outside link.
    pub incomplete_ratio: f64,
    /// Probability an unlinked inode is a symlink.
    pub symlink_ratio: f64,
    /// Regular-file apparent size distribution.
    pub size_dist: SizeDist,
}
impl Default for RandomTreeParams {
    fn default() -> Self {
        Self {
            dirs: 20,
            files: 100,
            max_depth: 5,
            link_sets: 10,
            max_links_per_set: 4,
            incomplete_ratio: 0.2,
            symlink_ratio: 0.1,
            size_dist: SizeDist::Uniform { min: 0, max: 65536 },
        }
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
    fn index(&mut self, n: usize) -> usize {
        (self.next() % n as u64) as usize
    }
    fn chance(&mut self, p: f64) -> bool {
        (self.next() >> 11) as f64 / ((1u64 << 53) as f64) < p
    }
}
fn name(s: String) -> RawName {
    RawName::new(s.into_bytes()).unwrap_or_else(|e| panic!("generated name: {e}"))
}
/// Generate a deterministic abstract tree without touching disk. Sizes in the
/// model are synthetic (512-byte allocation); real fixtures must measure metadata.
pub fn model_tree(seed: u64, p: RandomTreeParams) -> ModelTree {
    assert!(
        p.dirs > 0 && (p.dirs == 1 || p.max_depth > 0),
        "invalid directory/depth count"
    );
    assert!(
        p.link_sets <= p.files && p.max_links_per_set >= 2,
        "invalid link parameters"
    );
    assert!(
        (0.0..=1.0).contains(&p.incomplete_ratio) && (0.0..=1.0).contains(&p.symlink_ratio),
        "invalid ratio"
    );
    let cap = p
        .files
        .checked_mul(p.max_links_per_set)
        .and_then(|n| n.checked_add(p.files))
        .and_then(|n| n.checked_add(p.dirs));
    assert!(
        cap.is_some_and(|n| n <= MAX_MODEL_ITEMS),
        "model item cap exceeded"
    );
    let mut rng = Rng(seed);
    let mut tree = ModelTree {
        root: RawPath::new(b"/r".to_vec()).unwrap_or_else(|e| panic!("root: {e}")),
        now: 40_000_000,
        dirs: vec![ModelDir {
            parent: None,
            name: None,
            ino: 1,
            disk: 0,
            apparent: 0,
            mtime: 0,
            errors: vec![],
            skipped: vec![],
        }],
        inodes: BTreeMap::new(),
        entries: vec![],
    };
    let mut depth = vec![0];
    let mut parents = vec![0];
    for id in 1..p.dirs {
        let parent = parents[rng.index(parents.len())];
        let dep = depth[parent] + 1;
        tree.dirs.push(ModelDir {
            parent: Some(parent),
            name: Some(name(format!("d{id:06}"))),
            ino: id as u64 + 1,
            disk: 0,
            apparent: 0,
            mtime: tree.now - rng.index(40_000_001) as i64,
            errors: vec![],
            skipped: vec![],
        });
        depth.push(dep);
        if dep < p.max_depth {
            parents.push(id);
        }
    }
    for id in 0..p.files {
        let linked = id < p.link_sets;
        let symlink = !linked && rng.chance(p.symlink_ratio);
        let size = if symlink {
            2
        } else {
            match p.size_dist {
                SizeDist::Fixed(n) => n,
                SizeDist::Uniform { min, max } => {
                    assert!(min <= max && max < u64::MAX, "invalid size distribution");
                    min + rng.next() % (max - min + 1)
                }
            }
        };
        assert!(size <= u64::MAX - 511, "size overflow");
        let count = if linked {
            2 + rng.index(p.max_links_per_set - 1)
        } else {
            1
        };
        let ino = p.dirs as u64 + 1 + id as u64;
        tree.inodes.insert(
            ino,
            ModelInode {
                nlink: (count + usize::from(linked && rng.chance(p.incomplete_ratio))) as u32,
                kind: if symlink {
                    FileKind::Symlink
                } else {
                    FileKind::Regular
                },
                disk: if symlink { 0 } else { size.div_ceil(512) * 512 },
                apparent: size,
                mtime: tree.now - rng.index(40_000_001) as i64,
            },
        );
        for link in 0..count {
            tree.entries.push(ModelEntry {
                dir: rng.index(p.dirs),
                name: name(format!("f{id:06}_{link}")),
                ino,
            });
        }
    }
    tree
}
/// Materialize exactly the structure generated by `model_tree` for the same seed.
/// On-disk allocation and timestamps are filesystem-dependent.
pub fn random_tree(seed: u64, p: RandomTreeParams) -> Fixture {
    let tree = model_tree(seed, p);
    let paths = tree.paths();
    let mut spec = String::new();
    for path in paths.iter().skip(1) {
        spec.push_str(&format!(
            "[[entry]]\npath = {:?}\nkind = \"dir\"\n",
            &path.display_lossy()[3..]
        ));
    }
    let mut first = BTreeMap::new();
    for e in &tree.entries {
        let path = format!(
            "{}/{}",
            &paths[e.dir].display_lossy()[2..],
            e.name.display_lossy()
        );
        let path = path.trim_start_matches('/');
        let i = &tree.inodes[&e.ino];
        spec.push_str(&format!("[[entry]]\npath = {path:?}\n"));
        if let Some(target) = first.get(&e.ino) {
            spec.push_str(&format!("kind = \"hardlink\"\ntarget = {target:?}\n"));
        } else {
            first.insert(e.ino, path.to_owned());
            if i.kind == FileKind::Symlink {
                spec.push_str("kind = \"symlink\"\ntarget = \"..\"\n");
            } else {
                spec.push_str(&format!("kind = \"file\"\nsize = {}\n", i.apparent));
            }
        }
    }
    for (ino, target) in first {
        let found = tree.entries.iter().filter(|e| e.ino == ino).count();
        if found < tree.inodes[&ino].nlink as usize {
            spec.push_str(&format!(
                "[[outside]]\npath = \"outside{ino}\"\nkind = \"hardlink\"\ntarget = {target:?}\n"
            ));
        }
    }
    materialize(&spec)
}
