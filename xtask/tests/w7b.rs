//! Owned by: W7-B1
//! Small-scale fixture determinism, safety, mutation and benchmark oracle checks.
#![allow(clippy::unwrap_used, clippy::expect_used)]
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    os::unix::fs::{symlink, MetadataExt},
    path::Path,
    process::Command,
};
use tempfile::TempDir;
fn xtask() -> Command {
    Command::new(env!("CARGO_BIN_EXE_xtask"))
}
fn generate(root: &Path, n: u64, extra: &[&str]) -> std::process::Output {
    xtask()
        .args([
            "gen-fixture",
            "--profile",
            "reports",
            "--inodes",
            &n.to_string(),
            "--root",
        ])
        .arg(root)
        .args(extra)
        .output()
        .unwrap()
}
fn tree(root: &Path) -> BTreeMap<String, (u64, String)> {
    let mut result = BTreeMap::new();
    let mut todo = vec![root.to_path_buf()];
    let mut identities = BTreeMap::new();
    while let Some(p) = todo.pop() {
        let mut children = fs::read_dir(&p)
            .unwrap()
            .map(|e| e.unwrap().path())
            .collect::<Vec<_>>();
        children.sort();
        for child in children {
            let rel = child
                .strip_prefix(root)
                .unwrap()
                .to_string_lossy()
                .into_owned();
            let m = fs::symlink_metadata(&child).unwrap();
            if m.is_dir() {
                result.insert(rel, (0, "dir".into()));
                todo.push(child);
            } else if m.file_type().is_symlink() {
                result.insert(
                    rel,
                    (
                        m.len(),
                        format!("sym:{}", fs::read_link(child).unwrap().display()),
                    ),
                );
            } else if !rel.starts_with('.') {
                let identity = identities
                    .entry((m.dev(), m.ino()))
                    .or_insert_with(|| rel.clone())
                    .clone();
                result.insert(rel, (m.len(), identity));
            }
        }
    }
    result
}
#[test]
fn deterministic_and_proportions() {
    let a = TempDir::new().unwrap();
    let b = TempDir::new().unwrap();
    let out = generate(a.path(), 5000, &["--seed", "87"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(generate(b.path(), 5000, &["--seed", "87"]).status.success());
    assert_eq!(tree(a.path()), tree(b.path()));
    use std::hash::{Hash, Hasher};
    let digest = |root: &Path| {
        let mut h = std::collections::hash_map::DefaultHasher::new();
        tree(root).hash(&mut h);
        h.finish()
    };
    assert_eq!(digest(a.path()), digest(b.path()));
    let mut unique = BTreeSet::new();
    let mut linked = BTreeSet::new();
    let mut syms = 0;
    for d in fs::read_dir(a.path()).unwrap() {
        let d = d.unwrap();
        if !d.file_type().unwrap().is_dir() {
            continue;
        }
        for e in fs::read_dir(d.path()).unwrap() {
            let e = e.unwrap();
            let m = fs::symlink_metadata(e.path()).unwrap();
            if m.file_type().is_symlink() {
                syms += 1;
            } else {
                unique.insert(m.ino());
                if m.nlink() > 1 {
                    linked.insert(m.ino());
                    assert!((3..=5).contains(&m.nlink()));
                }
            }
        }
    }
    assert!((0.095..0.105).contains(&(syms as f64 / (unique.len() + syms) as f64)));
    assert!((0.195..0.205).contains(&(linked.len() as f64 / unique.len() as f64)));
    assert!(generate(a.path(), 5000, &["--seed", "87"]).status.success());
    assert_eq!(tree(a.path()), tree(b.path()));
}
#[test]
fn safety_refusals() {
    let t = TempDir::new().unwrap();
    let root = t.path().join("root");
    fs::create_dir(&root).unwrap();
    let sentinel = t.path().join("sentinel");
    fs::write(&sentinel, b"safe").unwrap();
    fs::write(root.join("foreign"), b"keep").unwrap();
    assert!(!generate(&root, 32, &[]).status.success());
    assert_eq!(fs::read(root.join("foreign")).unwrap(), b"keep");
    assert!(!generate(Path::new("/"), 32, &[]).status.success());
    assert!(!generate(&t.path().join("absent"), 32, &[]).status.success());
    let link = t.path().join("link");
    symlink(&root, &link).unwrap();
    assert!(!generate(&link, 32, &[]).status.success());
    assert!(!generate(&link.join("child"), 32, &[]).status.success());
    fs::remove_file(root.join("foreign")).unwrap();
    symlink(&sentinel, root.join(".tidemark-fixture")).unwrap();
    assert!(!generate(&root, 32, &[]).status.success());
    assert_eq!(fs::read(&sentinel).unwrap(), b"safe");
    fs::remove_file(root.join(".tidemark-fixture")).unwrap();
    fs::write(root.join(".tidemark-fixture"), b"bad marker").unwrap();
    assert!(!generate(&root, 32, &[]).status.success());
    assert!(!generate(&t.path().join(".."), 32, &[]).status.success());
}
#[test]
fn mutation_is_confined() {
    let t = TempDir::new().unwrap();
    let root = t.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::write(t.path().join("sentinel"), b"unchanged").unwrap();
    let out = generate(&root, 500, &["--mutate-rate", "101", "--duration", "2"]);
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let text = String::from_utf8(out.stdout).unwrap();
    let v: serde_json::Value = serde_json::from_str(text.lines().last().unwrap()).unwrap();
    assert!(v["mutation_operations"].as_u64().unwrap() >= 150);
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(root.join(".tidemark-fixture.json")).unwrap()).unwrap();
    assert_eq!(summary["state"], "before_mutation");
    assert!(root.join("mutations").is_dir());
    assert_eq!(fs::read(t.path().join("sentinel")).unwrap(), b"unchanged");
    assert_eq!(fs::read_dir(t.path()).unwrap().count(), 2);
}
#[test]
fn tiny_bench_matches_du() {
    let t = test_support::private_tempdir().unwrap();
    let root = t.path().join("fixture");
    let out = t.path().join("results");
    fs::create_dir(&root).unwrap();
    assert!(generate(&root, 500, &[]).status.success());
    // Measure the debug CLI of this test build: explicitly build the other
    // workspace binary beside this one (as tidemark-cli's surface test does)
    // so that a unit-test run needs no release build.
    let build = Command::new(env!("CARGO"))
        .current_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/.."))
        .args(["build", "--offline", "--bin", "tidemark"])
        .output()
        .unwrap();
    assert!(
        build.status.success(),
        "{}",
        String::from_utf8_lossy(&build.stderr)
    );
    let cli = Path::new(env!("CARGO_BIN_EXE_xtask")).with_file_name("tidemark");
    let result = xtask()
        .args(["bench", "--root"])
        .arg(&root)
        .args(["--modes", "du,walk", "--cache", "warm", "--tidemark"])
        .arg(&cli)
        .arg("--out")
        .arg(&out)
        .output()
        .unwrap();
    assert!(
        result.status.success(),
        "{}",
        String::from_utf8_lossy(&result.stderr)
    );
    let path = fs::read_dir(&out)
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.extension().is_some_and(|e| e == "json"))
        .unwrap();
    let v: serde_json::Value = serde_json::from_slice(&fs::read(path).unwrap()).unwrap();
    assert_eq!(
        v["rows"][0]["measurements"][0]["result"]["bytes"],
        v["rows"][1]["measurements"][0]["result"]["data"]["summary"]["t_disk"]
    );
}

#[test]
fn marker_cleanup_does_not_follow_links() {
    let t = TempDir::new().unwrap();
    let root = t.path().join("fixture");
    let outside = t.path().join("outside");
    fs::create_dir(&root).unwrap();
    fs::create_dir(&outside).unwrap();
    fs::write(outside.join("sentinel"), b"untouched").unwrap();
    assert!(generate(&root, 32, &[]).status.success());
    symlink(&outside, root.join("external-link")).unwrap();
    assert!(generate(&root, 32, &[]).status.success());
    assert_eq!(fs::read(outside.join("sentinel")).unwrap(), b"untouched");
    fs::create_dir_all(root.join("report-0000/nested")).unwrap();
    assert!(!generate(&root, 32, &[]).status.success());
    assert!(root.join("report-0000/nested").is_dir());
}

#[test]
fn summary_matches_du_and_inode_target() {
    let t = TempDir::new().unwrap();
    let result = generate(t.path(), 500, &[]);
    assert!(result.status.success());
    let summary: serde_json::Value =
        serde_json::from_slice(&fs::read(t.path().join(".tidemark-fixture.json")).unwrap())
            .unwrap();
    let stdout: serde_json::Value = serde_json::from_slice(&result.stdout).unwrap();
    assert_eq!(summary, stdout);
    let total = summary["directories"].as_u64().unwrap()
        + summary["regular"].as_u64().unwrap()
        + summary["symlinks"].as_u64().unwrap()
        + summary["control_files"].as_u64().unwrap();
    assert_eq!(total, 500);
    assert_eq!(summary["state"], "before_mutation");
    assert!(Command::new("sync")
        .arg("-f")
        .arg(t.path())
        .status()
        .unwrap()
        .success());
    assert_eq!(
        summary["apparent_bytes"],
        test_support::oracle::du_apparent_normalized(t.path()).unwrap()
    );
    let du = Command::new("du")
        .args(["-sx", "--block-size=1", "--"])
        .arg(t.path())
        .output()
        .unwrap();
    assert!(du.status.success());
    let bytes = String::from_utf8(du.stdout)
        .unwrap()
        .split_whitespace()
        .next()
        .unwrap()
        .parse::<u64>()
        .unwrap();
    assert_eq!(summary["allocated_bytes"], bytes);
}

/// Host-wide cache writes are opt-in and confined to explicitly marked disposable VMs.
#[cfg(feature = "privileged")]
#[test]
fn staging_cold_and_watcher() {
    if std::env::var("TIDEMARK_W7B_STAGING_VM").as_deref() != Ok("1") {
        eprintln!("pending VM: set TIDEMARK_W7B_STAGING_VM=1 on a disposable staging VM only");
        return;
    }
    let t = test_support::private_tempdir().unwrap();
    let root = t.path().join("fixture");
    fs::create_dir(&root).unwrap();
    assert!(generate(&root, 500, &[]).status.success());
    let cold = xtask()
        .args(["bench", "--root"])
        .arg(&root)
        .args([
            "--modes",
            "du,walk",
            "--cache",
            "cold",
            "--drop-caches-on-this-host",
            "--out",
        ])
        .arg(t.path().join("cold"))
        .output()
        .unwrap();
    assert!(
        cold.status.success(),
        "{}",
        String::from_utf8_lossy(&cold.stderr)
    );
    let watch = xtask()
        .args(["bench", "--root"])
        .arg(&root)
        .args([
            "--watcher-overhead",
            "--duration",
            "2",
            "--mutate-rate",
            "2000",
            "--out",
        ])
        .arg(t.path().join("watcher"))
        .output()
        .unwrap();
    assert!(
        watch.status.success(),
        "{}",
        String::from_utf8_lossy(&watch.stderr)
    );
}
