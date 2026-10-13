//! Owned by: WTEST-rules
//! Binary regressions for metadata-only reads and namespace mount boundaries.
#![forbid(unsafe_code)]
#![allow(clippy::unwrap_used, clippy::expect_used)]
#[path = "support/config.rs"]
mod config_support;
use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    process::Command,
};

fn command(temp: &Path) -> Command {
    let config = config_support::direct_config(
        temp,
        "[scan]\nthreads=1\n[throttle]\nmin_ops_per_sec=50000\nmax_ops_per_sec=50000\n",
    );
    fs::set_permissions(&config, fs::Permissions::from_mode(0o644)).unwrap();
    let mut c = Command::new(env!("CARGO_BIN_EXE_tidemark"));
    c.arg("--config")
        .arg(config)
        .arg("--state-dir")
        .arg(temp.join("state"))
        .arg("--json");
    c
}
fn copied_binary(temp: &Path) -> std::path::PathBuf {
    let binary = temp.join("tidemark");
    fs::copy(env!("CARGO_BIN_EXE_tidemark"), &binary).unwrap();
    fs::set_permissions(&binary, fs::Permissions::from_mode(0o755)).unwrap();
    binary
}
// A real-root CI runner must not bypass the fixture's mode-000 permissions.
fn metadata_command(temp: &Path) -> Command {
    use std::os::unix::process::CommandExt;
    let probe = temp.join("permission-probe");
    fs::write(&probe, b"probe").unwrap();
    fs::set_permissions(&probe, fs::Permissions::from_mode(0o0)).unwrap();
    let bypass = fs::File::open(&probe).is_ok();
    fs::remove_file(&probe).unwrap();
    let c = command(temp);
    if rustix::process::geteuid().is_root() && bypass {
        fs::set_permissions(temp, fs::Permissions::from_mode(0o1777)).unwrap();
        let binary = copied_binary(temp);
        let mut dropped = Command::new(binary);
        dropped.args(c.get_args()).gid(65534).uid(65534);
        dropped
    } else {
        assert!(
            !bypass,
            "mode-000 must reject contents opens for the scanner uid"
        );
        c
    }
}
fn run(c: &mut Command) -> Value {
    let out = c.output().unwrap();
    assert!(
        out.status.success(),
        "{c:?}\nstdout={}\nstderr={}",
        String::from_utf8_lossy(&out.stdout),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice::<Value>(&out.stdout).unwrap()["data"].clone()
}
fn atimes(paths: &[std::path::PathBuf]) -> Vec<(i64, i64)> {
    paths
        .iter()
        .map(|p| {
            let m = fs::metadata(p).unwrap();
            (m.atime(), m.atime_nsec())
        })
        .collect()
}
#[test]
fn regular_contents_are_never_read() {
    let t = ::test_support::private_tempdir().unwrap();
    let root = t.path().join("root");
    fs::create_dir(&root).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o755)).unwrap();
    let locked = root.join("locked");
    let readable = root.join("readable");
    for p in [&locked, &readable] {
        fs::write(p, b"metadata only").unwrap();
        let old = std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(1_000_000);
        let times = || {
            fs::FileTimes::new()
                .set_accessed(old)
                .set_modified(old + std::time::Duration::from_secs(1000))
        };
        fs::File::open(p).unwrap().set_times(times()).unwrap();
        // Establish that this fixture filesystem exposes contents reads via
        // atime; a noatime mount must fail the precondition, never silently pass.
        fs::read(p).unwrap();
        assert_ne!(
            fs::metadata(p).unwrap().atime(),
            1_000_000,
            "fixture filesystem does not expose reads through atime"
        );
        fs::File::open(p).unwrap().set_times(times()).unwrap();
    }
    fs::set_permissions(&readable, fs::Permissions::from_mode(0o644)).unwrap();
    fs::set_permissions(&locked, fs::Permissions::from_mode(0o0)).unwrap();
    fs::hard_link(&locked, root.join("alias")).unwrap();
    std::os::unix::fs::symlink("locked", root.join("link")).unwrap();
    let paths = vec![locked, readable, root.join("alias")];
    let before = atimes(&paths);
    let apparent: u64 = fs::read_dir(&root)
        .unwrap()
        .map(|e| fs::symlink_metadata(e.unwrap().path()).unwrap().len())
        .sum();
    let scan = run(metadata_command(t.path()).arg("scan").arg(&root).args([
        "--mode",
        "walk",
        "--throttle",
        "max",
    ]));
    assert_eq!(scan["summary"]["errors"], 0);
    let catalog = tidemark_store::catalog::Catalog::open(&t.path().join("state"), false).unwrap();
    let entries = catalog.list(None).unwrap();
    let snapshot = tidemark_store::Snapshot::open(entries[0].file.as_ref().unwrap()).unwrap();
    let row = snapshot.root().unwrap();
    assert_eq!(row.t_files, 4);
    let expected_disk = [&paths[0], &paths[1], &root.join("link"), &root]
        .iter()
        .map(|p| fs::symlink_metadata(p).unwrap().blocks() * 512)
        .sum::<u64>();
    assert_eq!(row.t_disk, expected_disk);
    // Apparent totals count hard-linked inode once and include directory metadata.
    assert_eq!(
        row.t_apparent,
        apparent - fs::metadata(&paths[0]).unwrap().len() + fs::metadata(&root).unwrap().len()
    );
    assert_eq!(atimes(&paths), before, "scan read contents");
    let symlinks = run(metadata_command(t.path())
        .arg("symlinks")
        .arg(&root)
        .arg("--check-targets"));
    assert_eq!(symlinks["total"], 1);
    assert_eq!(symlinks["examples"][0]["class"], "inside_root");
    assert_eq!(atimes(&paths), before, "symlink analysis read contents");
    let links = run(metadata_command(t.path())
        .arg("links")
        .arg(&root)
        .arg("--show-paths"));
    assert_eq!(links["paths"].as_array().unwrap().len(), 2);
    assert_eq!(atimes(&paths), before, "links path recovery read contents");
}

#[test]
fn namespace_mounts_are_skipped() {
    let t = ::test_support::private_tempdir().unwrap();
    let root = t.path().join("root");
    fs::create_dir(&root).unwrap();
    for name in ["tmpfs", "bind"] {
        fs::create_dir(root.join(name)).unwrap();
    }
    let source = t.path().join("source");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("hidden"), b"bind payload").unwrap();
    let marker = t.path().join("entered");
    fs::set_permissions(t.path(), fs::Permissions::from_mode(0o755)).unwrap();
    let original = command(t.path());
    let mut scan = Command::new(copied_binary(t.path()));
    scan.args(original.get_args());
    scan.arg("scan")
        .arg(&root)
        .args(["--mode", "walk", "--throttle", "max"]);
    // Positional arguments preserve byte paths; no shell interpolation. unshare
    // defaults to private propagation; request it explicitly to isolate all mounts.
    let script = r#"set -eux
: > "$1"
mount -n -t tmpfs tmpfs "$2/tmpfs"
printf 'hidden tmpfs payload' > "$2/tmpfs/hidden"
mount -n --bind "$3" "$2/bind"
shift 3
exec "$@"
"#;
    let out = Command::new("unshare")
        .args([
            "--user",
            "--map-root-user",
            "--mount",
            "--propagation",
            "private",
            "sh",
            "-c",
            script,
            "rules",
        ])
        .arg(&marker)
        .arg(&root)
        .arg(&source)
        .arg(scan.get_program())
        .args(scan.get_args())
        .output()
        .unwrap();
    let stderr = String::from_utf8_lossy(&out.stderr);
    if !marker.exists()
        && !out.status.success()
        && (stderr.contains("unshare failed")
            || stderr.contains("write failed /proc/self/uid_map")
            || stderr.contains("write failed /proc/self/gid_map"))
    {
        eprintln!("namespace cannot be created: {}", stderr.trim());
        return;
    }
    assert!(
        out.status.success(),
        "namespace child failed: stdout={}\nstderr={stderr}",
        String::from_utf8_lossy(&out.stdout)
    );
    let catalog = tidemark_store::catalog::Catalog::open(&t.path().join("state"), false).unwrap();
    let entries = catalog.list(None).unwrap();
    let snapshot = tidemark_store::Snapshot::open(entries[0].file.as_ref().unwrap()).unwrap();
    let row = snapshot.root().unwrap();
    assert_eq!(row.t_files, 0, "{stderr}");
    assert_eq!(
        row.t_apparent,
        fs::metadata(&root).unwrap().len(),
        "{stderr}"
    );
    assert_eq!(
        row.t_disk,
        fs::metadata(&root).unwrap().blocks() * 512,
        "{stderr}"
    );
    let skips = snapshot.skipped_under(row.id).unwrap();
    assert_eq!(skips.len(), 2, "{stderr}");
    for (skip, name) in skips.iter().zip([b"bind".as_slice(), b"tmpfs".as_slice()]) {
        assert_eq!(skip.name.as_bytes(), name, "{stderr}");
        assert_eq!(
            skip.reason,
            tidemark_core::scan::SkipReason::Mount,
            "{stderr}"
        );
    }
}
