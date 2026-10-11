//! Owned by: W0-C
//! Unprivileged fixture, deterministic-model and oracle acceptance tests.
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::{
        ffi::OsStrExt,
        fs::{FileTypeExt, MetadataExt},
    },
    path::Path,
};
use test_support::{fixture::*, golden::*, oracle::*, random::*, reference::compute};
use tidemark_core::{
    path::RawName,
    scan::{DirId, ErrOp, FileKind, ScanErrorRec, Skip, SkipReason},
    time::AGE_BOUNDS_SECS,
};
fn fixture(name: &str) -> Fixture {
    materialize_file(
        Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("fixtures")
            .join(format!("{name}.toml")),
    )
}
#[test]
fn materialize_link_semantics_basic_matches_du_with_measured_directories() {
    let f = fixture("link_semantics_basic");
    let mut model = link_semantics_basic();
    let paths = model.paths();
    for (d, p) in model.dirs.iter_mut().zip(&paths) {
        let rel = p.as_path().strip_prefix("/r").unwrap();
        let m = fs::symlink_metadata(f.root.join(rel)).unwrap();
        d.disk = m.blocks() * 512;
        d.apparent = m.size();
    }
    let mut differing_allocations = Vec::new();
    for (ino, path) in [
        (100, "a/x"),
        (101, "a/big"),
        (102, "b/z"),
        (103, "c/d/w"),
        (104, "c/s"),
    ] {
        let m = fs::symlink_metadata(f.root.join(path)).unwrap();
        let inode = model.inodes.get_mut(&ino).unwrap();
        assert_eq!(m.nlink(), inode.nlink as u64);
        let measured = m.blocks() * 512;
        if measured != inode.disk {
            differing_allocations.push(format!(
                "inode {ino} ({path}): measured {measured}, golden {}",
                inode.disk
            ));
        }
        inode.disk = measured;
        assert_eq!(m.size(), inode.apparent);
    }
    assert_eq!(
        fs::symlink_metadata(f.root.join("a/x")).unwrap().ino(),
        fs::symlink_metadata(f.root.join("b/y")).unwrap().ino()
    );
    assert_eq!(
        fs::read_link(f.root.join("c/s")).unwrap(),
        Path::new("../a")
    );
    let rows = compute(&model);
    if differing_allocations.is_empty() {
        for (id, golden) in LINK_SEMANTICS_BASIC.iter().enumerate() {
            let row = &rows[&DirId(id as u32)];
            let directory_disk: u64 = model
                .dirs
                .iter()
                .zip(&paths)
                .filter(|(_, p)| p.starts_with(&paths[id]))
                .map(|(d, _)| d.disk)
                .sum();
            assert_eq!(paths[id].display_lossy(), golden.path);
            assert_eq!(
                [
                    row.t_disk - directory_disk,
                    row.t_charged - directory_disk,
                    row.t_reclaimable - directory_disk,
                    row.shared,
                    row.t_files,
                    row.t_inodes,
                    row.t_dirs,
                ],
                golden.values
            );
        }
    } else {
        eprintln!(
            "skipping only 03 §6 golden-table assertion: filesystem allocations differ: {}",
            differing_allocations.join("; ")
        );
    }
    if gnu_available("du") {
        for (id, path) in paths.iter().enumerate() {
            let path = f.root.join(path.as_path().strip_prefix("/r").unwrap());
            let row = &rows[&DirId(id as u32)];
            assert_eq!(du_disk(&path), row.t_disk);
            let counts_dirs = du_apparent_counts_directories().unwrap();
            let dir_apparent: u64 = model
                .dirs
                .iter()
                .zip(&paths)
                .filter(|(_, p)| p.starts_with(&paths[id]))
                .map(|(d, _)| d.apparent)
                .sum();
            assert_eq!(
                du_apparent(&path),
                row.t_apparent - if counts_dirs { 0 } else { dir_apparent }
            );
        }
    }
    if gnu_available("find") {
        for (id, path) in paths.iter().enumerate() {
            let path = f.root.join(path.as_path().strip_prefix("/r").unwrap());
            let m = &rows[&DirId(id as u32)];
            assert_eq!(
                find_counts(path),
                FindCounts {
                    files: m.t_files,
                    dirs: m.t_dirs,
                    inodes: m.t_inodes
                }
            );
        }
    }
    let root = f.root.clone();
    let outside = f.outside.clone();
    drop(f);
    assert!(!root.exists() && !outside.exists());
}
#[test]
fn dsl_bytes_sparse_specials_and_modes() {
    let f = fixture("nonutf8");
    let names: BTreeSet<_> = fs::read_dir(f.root.join("weird"))
        .unwrap()
        .map(|e| e.unwrap().file_name().as_bytes().to_vec())
        .collect();
    assert!(names.contains(b"\xff\xfe-name".as_slice()));
    assert!(names.contains(b"new\nline".as_slice()));
    assert!(names.contains(b"slash\\quote\"".as_slice()));
    if gnu_available("find") {
        assert_eq!(
            find_counts(&f.root),
            FindCounts {
                files: 3,
                dirs: 2,
                inodes: 3
            }
        );
    }
    let sparse = fixture("sparse");
    let m = fs::metadata(sparse.root.join("sparse")).unwrap();
    assert_eq!(m.len(), 1 << 30);
    if gnu_available("du") {
        assert_eq!(du_disk(sparse.root.join("sparse")), m.blocks() * 512);
    }
    let fifo = materialize("[[entry]]\npath='pipe'\nkind='fifo'");
    assert!(fs::symlink_metadata(fifo.root.join("pipe"))
        .unwrap()
        .file_type()
        .is_fifo());
    match try_materialize(include_str!("../fixtures/mixed_special.toml")) {
        Ok(special) => assert!(fs::symlink_metadata(special.root.join("socket"))
            .unwrap()
            .file_type()
            .is_socket()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            eprintln!("skipping Unix socket fixture: sandbox denies socket creation ({e})")
        }
        Err(e) => panic!("special fixture: {e}"),
    }
    let locked=materialize("[[entry]]\npath='locked'\nkind='dir'\nmode=0o000\n[[entry]]\npath='locked/file'\nkind='file'\nsize=1\n");
    assert_eq!(
        fs::metadata(locked.root.join("locked")).unwrap().mode() & 0o777,
        0
    );
    let root = locked.root.clone();
    drop(locked);
    assert!(!root.exists());
}
#[test]
fn dsl_rejects_escapes_duplicates_symlink_parents_and_cycles() {
    for spec in ["[[entry]]\npath='../escape'\nkind='file'", "[[entry]]\npath='/escape'\nkind='file'", "[[entry]]\npath='bad\\x00'\nkind='file'", "[[entry]]\npath='bad\\xQQ'\nkind='file'", "[[entry]]\npath='x'\nkind='file'\n[[entry]]\npath='x'\nkind='file'", "[[entry]]\npath='a'\nkind='symlink'\ntarget='..'\n[[entry]]\npath='a/file'\nkind='file'", "[[entry]]\npath='a'\nkind='hardlink'\ntarget='b'\n[[entry]]\npath='b'\nkind='hardlink'\ntarget='a'", "[[entry]]\npath='a'\nkind='symlink'\ntarget='..'\n[[entry]]\npath='b'\nkind='hardlink'\ntarget='a/file'", "[[entry]]\npath='x'\nkind='file'\nsize=1\nsparse=2"] {assert!(try_materialize(spec).is_err(),"{spec}");}
    let f=materialize("[[entry]]\npath='b'\nkind='hardlink'\ntarget='a'\n[[entry]]\npath='a'\nkind='file'\nsize=1");
    assert_eq!(
        fs::metadata(f.root.join("a")).unwrap().ino(),
        fs::metadata(f.root.join("b")).unwrap().ino()
    );
}
#[test]
fn generated_deep_and_link_web_and_symlink_cycles() {
    let f = fixture("deep");
    let mut cursor = f.root.clone();
    let mut depth = 0;
    loop {
        let mut children = fs::read_dir(&cursor).unwrap();
        match children.next() {
            Some(e) => {
                assert!(children.next().is_none());
                cursor = e.unwrap().path();
                depth += 1;
            }
            None => break,
        }
    }
    assert_eq!(depth, 200);
    let web = fixture("hardlink_web");
    assert_eq!(fs::metadata(web.root.join("d00/s50")).unwrap().nlink(), 51);
    assert_eq!(fs::metadata(web.root.join("d00/s2")).unwrap().nlink(), 3);
    let loops = fixture("symlink_loops");
    assert_eq!(fs::read_link(loops.root.join("a")).unwrap(), Path::new("b"));
    if gnu_available("find") {
        assert_eq!(
            find_counts(loops.root.clone()),
            FindCounts {
                files: 4,
                dirs: 2,
                inodes: 4
            }
        );
    }
}
#[test]
#[ignore = "scale"]
fn generated_wide_has_120000_entries() {
    let f = fixture("wide_dir");
    assert_eq!(fs::read_dir(f.root.join("wide")).unwrap().count(), 120_000);
}
// Canonical byte listing replaces actual inode numbers with first entry paths.
type ListingRow = (Vec<u8>, u64, u64, u32, Vec<u8>);
#[allow(clippy::unwrap_used)] // Test-only traversal helper; failures must abort the test.
fn listing(root: &Path) -> Vec<ListingRow> {
    let mut todo = vec![root.to_path_buf()];
    let mut entries = vec![];
    while let Some(p) = todo.pop() {
        for e in fs::read_dir(p).unwrap() {
            let p = e.unwrap().path();
            let m = fs::symlink_metadata(&p).unwrap();
            if m.is_dir() {
                todo.push(p.clone());
            }
            entries.push((p, m));
        }
    }
    entries.sort_by(|a, b| a.0.cmp(&b.0));
    let mut ino_names = BTreeMap::new();
    entries
        .into_iter()
        .map(|(p, m)| {
            let bytes = p
                .strip_prefix(root)
                .unwrap()
                .as_os_str()
                .as_bytes()
                .to_vec();
            let identity = ino_names
                .entry(m.ino())
                .or_insert_with(|| bytes.clone())
                .clone();
            (bytes, m.len(), m.nlink(), m.mode() & 0o170000, identity)
        })
        .collect()
}
#[test]
fn random_model_and_disk_structure_are_seed_deterministic() {
    let p = RandomTreeParams {
        dirs: 12,
        files: 30,
        size_dist: SizeDist::Fixed(32),
        ..RandomTreeParams::default()
    };
    let a = model_tree(42, p.clone());
    let b = model_tree(42, p.clone());
    assert_eq!(a, b);
    assert_eq!(a.to_dir_records(), b.to_dir_records());
    assert_ne!(a, model_tree(43, p.clone()));
    let f = random_tree(42, p.clone());
    let g = random_tree(42, p);
    assert_eq!(listing(&f.root), listing(&g.root));
    assert_eq!(listing(&f.outside), listing(&g.outside));
    let paths = a.paths();
    for e in &a.entries {
        let rel = paths[e.dir].join(&e.name);
        let disk = f.root.join(rel.as_path().strip_prefix("/r").unwrap());
        let stat = fs::symlink_metadata(disk).unwrap();
        let i = &a.inodes[&e.ino];
        assert_eq!(stat.nlink(), i.nlink as u64);
        assert_eq!(stat.len(), i.apparent);
        assert_eq!(stat.file_type().is_symlink(), i.kind == FileKind::Symlink);
    }
}
#[test]
fn reference_handles_same_directory_links_races_negative_mtimes_and_errors() {
    let mut t = link_semantics_basic();
    t.now = 0;
    for d in &mut t.dirs {
        d.mtime = -100;
        d.disk = 512;
        d.apparent = 64;
    }
    t.entries[2].dir = 1;
    t.inodes.get_mut(&100).unwrap().nlink = 2; // found >= nlink race tolerance
    let mut copy = t.entries[0].clone();
    copy.name = RawName::new(b"another".to_vec()).unwrap();
    t.entries.push(copy);
    t.inodes.get_mut(&100).unwrap().mtime = 1;
    t.dirs[1].errors.push(ScanErrorRec {
        path: t.root.clone(),
        op: ErrOp::Statx,
        errno: 13,
    });
    t.dirs[1].skipped.push(Skip {
        name: RawName::new(b"excluded".to_vec()).unwrap(),
        reason: SkipReason::Excluded,
    });
    let rows = compute(&t);
    let a = &rows[&DirId(1)];
    assert_eq!(a.t_files, 4);
    assert_eq!(a.t_inodes, 2);
    assert_eq!(a.t_reclaimable, a.t_charged);
    assert_eq!(a.newest_mtime, 1);
    assert_eq!(a.max_entries, 5);
    assert_eq!(a.t_errors, 1);
    let root = &rows[&DirId(0)];
    assert_eq!(root.t_errors, 1);
    assert_eq!(root.t_disk, root.t_charged);
    let records = t.to_dir_records();
    assert_eq!(records[1].entries, 5);
    assert_eq!(records[3].own.newest_mtime, 0);
}
#[test]
fn reference_age_boundaries_and_directory_bytes() {
    let mut t = link_semantics_basic();
    for i in t.inodes.values_mut() {
        i.disk = 0;
    }
    for (d, age) in t
        .dirs
        .iter_mut()
        .zip(std::iter::once(-1).chain(AGE_BOUNDS_SECS))
    {
        d.disk = 512;
        d.mtime = -age;
    }
    let root = &compute(&t)[&DirId(0)];
    assert_eq!(root.age, [512; 6]);
    assert_eq!(root.newest_mtime, 1);
    assert_eq!(root.t_reclaimable, 3072);
}
#[test]
fn random_reference_invariants_across_seeds() {
    for seed in 0..100 {
        let t = model_tree(seed, RandomTreeParams::default());
        let rows = compute(&t);
        let root = &rows[&DirId(0)];
        assert_eq!(root.t_disk, root.t_charged);
        for (id, m) in &rows {
            assert!(m.t_reclaimable <= m.t_charged && m.t_reclaimable <= m.t_disk);
            assert_eq!(m.age.iter().sum::<u64>(), m.t_charged);
            let children: Vec<_> = t
                .dirs
                .iter()
                .enumerate()
                .filter(|(_, d)| d.parent == Some(id.0 as usize))
                .map(|(c, _)| &rows[&DirId(c as u32)])
                .collect();
            assert!(children.iter().all(|c| m.t_disk >= c.t_disk));
            assert!(children.iter().map(|c| c.t_charged).sum::<u64>() <= m.t_charged);
            assert_eq!(1 + children.iter().map(|c| c.t_dirs).sum::<u64>(), m.t_dirs);
        }
    }
}

#[test]
fn caller_models_reject_invalid_structure_and_byte_overflow() {
    let mut tree = link_semantics_basic();
    tree.dirs[1].parent = Some(1);
    assert!(std::panic::catch_unwind(|| tree.to_dir_records()).is_err());
    let mut tree = link_semantics_basic();
    tree.inodes.get_mut(&101).unwrap().disk = u64::MAX;
    assert!(std::panic::catch_unwind(|| compute(&tree)).is_err());
    let mut tree = link_semantics_basic();
    tree.entries[0].ino = 999;
    assert!(std::panic::catch_unwind(|| tree.to_dir_records()).is_err());
}

#[test]
fn walk_records_preserve_negative_mtimes_names_and_skipped_width() {
    let mut tree = link_semantics_basic();
    for inode in tree.inodes.values_mut() {
        inode.mtime = -7;
    }
    let records = tree.to_dir_records();
    assert_eq!(records[1].own.newest_mtime, -7);
    assert_eq!(records[3].own.symlinks, 1);
    assert_eq!(records[3].own.files, 1);
    assert_eq!(records[3].own.apparent, 4);
    assert_eq!(records[2].links[1].name.as_ref().unwrap().as_bytes(), b"z");
    assert!(records.iter().all(|r| r.projid.is_none() && r.races == 0));
}
