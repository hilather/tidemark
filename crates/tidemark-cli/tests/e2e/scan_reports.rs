//! Owned by: TEST-e2e-scan
//! One adversarial tree scanned through every single-snapshot operator report.
use super::support::{accessible, no_temporaries, Harness};
use serde_json::Value;
use std::{ffi::OsStr, os::unix::fs::PermissionsExt};
use tidemark_core::{path::RawPath, snapshot::DirRow};

fn rows(h: &Harness, root: &std::path::Path, sort: &str, depth: &str, limit: &str) -> Vec<DirRow> {
    let data = h.doc(
        &[
            "top".as_ref(),
            root.as_os_str(),
            "--sort".as_ref(),
            sort.as_ref(),
            "--depth".as_ref(),
            depth.as_ref(),
            "--limit".as_ref(),
            limit.as_ref(),
        ],
        0,
        "tidemark.top.v1",
    );
    serde_json::from_value(data).unwrap()
}
fn sum(v: &Value, key: &str) -> u64 {
    if let Some(a) = v.as_array() {
        a.iter().map(|v| sum(v, key)).sum()
    } else {
        v[key].as_u64().unwrap_or(0)
    }
}
#[test]
fn scan_to_every_report() {
    if super::identity::delegate("scan_reports::scan_to_every_report") {
        return;
    }
    assert!(test_support::oracle::gnu_available("du"));
    assert!(test_support::oracle::gnu_available("find"));
    let f = test_support::materialize(include_str!("../../../test-support/fixtures/e2e_scan.toml"));
    accessible(f.dir.path());
    let mut snapshots = Vec::new();
    // Independent states keep exactly one catalog entry while exercising both render modes.
    for json in [false, true] {
        let h = Harness::new();
        let scan_args = [
            OsStr::new("scan"),
            f.root.as_os_str(),
            OsStr::new("--mode"),
            OsStr::new("walk"),
            OsStr::new("--throttle"),
            OsStr::new("max"),
        ];
        let scan = h.run(json, &scan_args, 3);
        if json {
            let doc: Value = serde_json::from_slice(&scan.stdout).unwrap();
            assert_eq!(doc["schema"], "tidemark.scan.v1");
            assert_eq!(doc["data"]["status"], "complete");
            assert_eq!(doc["data"]["degraded"], true);
            assert_eq!(doc["data"]["summary"]["errors"], 1);
        } else {
            assert!(String::from_utf8_lossy(&scan.stdout).contains("Errors: 1"));
        }
        let catalog = tidemark_store::catalog::Catalog::open(&h.state, false).unwrap();
        let entries = catalog.list(None).unwrap();
        assert_eq!(entries.len(), 1);
        let e = &entries[0];
        assert_eq!(e.status, "complete");
        assert!(e.degraded);
        assert_eq!(catalog.latest(&e.root).unwrap().unwrap().scan_id, e.scan_id);
        let file = e.file.as_ref().unwrap();
        assert_eq!(
            std::fs::metadata(file).unwrap().permissions().mode() & 0o777,
            0o600
        );
        let snapshot = tidemark_store::Snapshot::open(file).unwrap();
        let errors = snapshot.errors_under(0, 100).unwrap();
        assert_eq!(errors.len(), 1);
        assert_eq!(
            errors[0].path,
            RawPath::from_path(&f.root.join("locked")).unwrap()
        );
        // The locked directory is empty: an oracle with permission restored sees
        // exactly the metadata the degraded scan can observe, without hidden entries.
        std::fs::set_permissions(
            f.root.join("locked"),
            std::fs::Permissions::from_mode(0o700),
        )
        .unwrap();
        let all = rows(&h, &f.root, "name", "100", "1000");
        let counts = test_support::oracle::find_counts(&f.root);
        assert_eq!(all.len() as u64, counts.dirs);
        for r in &all {
            let path = r.path.as_path();
            assert_eq!(
                r.t_disk,
                test_support::oracle::du_disk(path),
                "disk {:?}",
                path
            );
            assert_eq!(
                r.t_apparent,
                test_support::oracle::du_apparent_normalized(path).unwrap(),
                "apparent {:?}",
                path
            );
            let c = test_support::oracle::find_counts(path);
            assert_eq!(
                (r.t_files, r.t_inodes, r.t_dirs),
                (c.files, c.inodes, c.dirs),
                "counts {:?}",
                path
            );
        }
        std::fs::set_permissions(f.root.join("locked"), std::fs::Permissions::from_mode(0o0))
            .unwrap();
        // Full ordering includes the deterministic name-byte tie breaker. Unique
        // names in this fixture make the composite order strict despite metric ties.
        for (sort, field) in [
            ("disk", "t_disk"),
            ("apparent", "t_apparent"),
            ("reclaimable", "t_reclaimable"),
            ("charged", "t_charged"),
            ("files", "t_files"),
            ("inodes", "t_inodes"),
            ("newest", "newest_mtime"),
            ("name", ""),
        ] {
            let data = h.doc(
                &[
                    "top".as_ref(),
                    f.root.as_os_str(),
                    "--sort".as_ref(),
                    sort.as_ref(),
                    "--depth".as_ref(),
                    "100".as_ref(),
                    "--limit".as_ref(),
                    "1000".as_ref(),
                ],
                0,
                "tidemark.top.v1",
            );
            let a = data.as_array().unwrap();
            assert_eq!(a.len(), all.len());
            for pair in a.windows(2) {
                let left: DirRow = serde_json::from_value(pair[0].clone()).unwrap();
                let right: DirRow = serde_json::from_value(pair[1].clone()).unwrap();
                let metric = if field.is_empty() {
                    std::cmp::Ordering::Equal
                } else {
                    pair[1][field]
                        .as_i64()
                        .unwrap()
                        .cmp(&pair[0][field].as_i64().unwrap())
                };
                assert!(
                    metric.then_with(|| left.name.cmp(&right.name)).is_lt(),
                    "sort={sort}: {} then {}",
                    left.path.display_lossy(),
                    right.path.display_lossy()
                );
            }
            assert_eq!(
                rows(&h, &f.root, sort, "100", "3"),
                rows(&h, &f.root, sort, "100", "1000")[..3]
            );
            h.run(
                false,
                &[
                    "top".as_ref(),
                    f.root.as_os_str(),
                    "--sort".as_ref(),
                    sort.as_ref(),
                    "--wide".as_ref(),
                ],
                0,
            );
        }
        assert_eq!(rows(&h, &f.root, "name", "0", "1000").len(), 1);
        assert!(rows(&h, &f.root, "name", "1", "1000")
            .iter()
            .all(|r| r.depth <= 1));
        h.run(
            true,
            &[
                "top".as_ref(),
                f.root.as_os_str(),
                "--sort".as_ref(),
                "growth".as_ref(),
            ],
            5,
        );
        let files = h.doc(
            &[
                "files".as_ref(),
                f.root.as_os_str(),
                "--min-size".as_ref(),
                "4KiB".as_ref(),
                "--limit".as_ref(),
                "1000".as_ref(),
            ],
            0,
            "tidemark.files.v1",
        );
        assert!(!files.as_array().unwrap().is_empty());
        assert!(files
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["row"]["disk"].as_u64().unwrap() >= 4096));
        // A zero threshold includes the byte-name file; DTO deserialization must
        // round-trip the exact absolute path, never a replacement character.
        let files = h.doc(
            &[
                "files".as_ref(),
                f.root.as_os_str(),
                "--min-size".as_ref(),
                "0".as_ref(),
                "--limit".as_ref(),
                "1000".as_ref(),
            ],
            0,
            "tidemark.files.v1",
        );
        let hits: Vec<tidemark_core::snapshot::FileHit> =
            serde_json::from_value(files.clone()).unwrap();
        let raw = f
            .root
            .join(std::ffi::OsStr::from_bytes(b"weird/\xff\xfe-name"));
        assert!(hits
            .iter()
            .any(|hit| hit.path.as_ref().is_some_and(|p| p.as_path() == raw)));
        assert!(files
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v.get("path_b64").is_some() && v.get("path").is_none()));
        let text = h.run(
            false,
            &[
                "files".as_ref(),
                f.root.as_os_str(),
                "--limit".as_ref(),
                "1000".as_ref(),
            ],
            0,
        );
        assert!(String::from_utf8(text.stdout)
            .unwrap()
            .contains("\\xFF\\xFE-name"));
        let links = h.doc(
            &[
                "links".as_ref(),
                f.root.as_os_str(),
                "--boundary-depth".as_ref(),
                "1".as_ref(),
                "--show-paths".as_ref(),
                "--top".as_ref(),
                "100".as_ref(),
            ],
            0,
            "tidemark.links.v1",
        );
        assert_eq!(links["totals"]["sets"], 49);
        assert_eq!(links["totals"]["crossing_sets"], 49);
        assert_eq!(links["totals"]["incomplete_sets"], 1);
        assert_eq!(links["totals"]["instances"], (2..=50u64).sum::<u64>());
        assert_eq!(
            links["paths"].as_array().unwrap().len(),
            (2..=50usize).sum::<usize>()
        );
        assert!(links["paths"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["status"] == "snapshot"));
        use std::os::unix::fs::MetadataExt;
        let link_disk: u64 = (2..=50)
            .map(|n| {
                std::fs::symlink_metadata(f.root.join(format!("d00/s{n}")))
                    .unwrap()
                    .blocks()
                    * 512
            })
            .sum();
        assert_eq!(links["totals"]["disk"], link_disk);
        assert_eq!(links["totals"]["crossing_disk"], link_disk);
        assert_eq!(links["region_pairs"][0]["sets"], 49);
        let incomplete = h.doc(
            &[
                "links".as_ref(),
                f.root.as_os_str(),
                "--incomplete".as_ref(),
            ],
            0,
            "tidemark.links.v1",
        );
        assert_eq!(incomplete["largest_sets"].as_array().unwrap().len(), 1);
        assert_eq!(incomplete["largest_sets"][0]["complete"], false);
        assert_eq!(incomplete["largest_sets"][0]["found"], 2);
        assert_eq!(incomplete["largest_sets"][0]["nlink"], 3);
        let text = h.run(
            false,
            &[
                "links".as_ref(),
                f.root.as_os_str(),
                "--boundary-depth".as_ref(),
                "1".as_ref(),
            ],
            0,
        );
        for section in [
            "Link sets:",
            "Incomplete:",
            "Crossing:",
            "Pinned directories:",
            "Largest link sets:",
            "Most shared directories:",
        ] {
            assert!(String::from_utf8_lossy(&text.stdout).contains(section));
        }
        // Check targets while permission is available so this live report measures
        // symlink classes rather than introducing another permission failure.
        std::fs::set_permissions(
            f.root.join("locked"),
            std::fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        let symlinks = h.doc(
            &[
                "symlinks".as_ref(),
                f.root.as_os_str(),
                "--check-targets".as_ref(),
            ],
            0,
            "tidemark.symlinks.v1",
        );
        assert_eq!(symlinks["total"], 5);
        let filtered = h.doc(
            &[
                "symlinks".as_ref(),
                f.root.as_os_str(),
                "--check-targets".as_ref(),
                "--dangling".as_ref(),
                "--limit".as_ref(),
                "1".as_ref(),
            ],
            0,
            "tidemark.symlinks.v1",
        );
        assert_eq!(filtered["total"], symlinks["total"]);
        assert_eq!(filtered["examples"].as_array().unwrap().len(), 1);
        assert_eq!(filtered["examples"][0]["class"], "dangling");
        assert_eq!(filtered["examples"][0]["target"]["path"], "absent");
        let escaping = h.doc(
            &[
                "symlinks".as_ref(),
                f.root.as_os_str(),
                "--check-targets".as_ref(),
                "--escaping".as_ref(),
            ],
            0,
            "tidemark.symlinks.v1",
        );
        assert_eq!(escaping["total"], symlinks["total"]);
        assert_eq!(escaping["examples"].as_array().unwrap().len(), 1);
        let classes = symlinks["by_class"].as_array().unwrap();
        assert!(classes.iter().any(|v| v[0] == "loop" && v[1] == 2));
        h.run(
            false,
            &[
                "symlinks".as_ref(),
                f.root.as_os_str(),
                "--check-targets".as_ref(),
            ],
            0,
        );
        h.run(
            false,
            &[
                "symlinks".as_ref(),
                f.root.as_os_str(),
                "--dangling".as_ref(),
            ],
            2,
        );
        std::fs::set_permissions(f.root.join("locked"), std::fs::Permissions::from_mode(0o0))
            .unwrap();
        let list = h.doc(
            &["snapshots".as_ref(), "list".as_ref()],
            0,
            "tidemark.snapshots.v1",
        );
        assert_eq!(list.as_array().unwrap().len(), 1);
        let show = h.doc(
            &["snapshots".as_ref(), "show".as_ref(), e.scan_id.as_ref()],
            0,
            "tidemark.snapshots.v1",
        );
        assert_eq!(show["scan_id"], e.scan_id);
        for args in [
            vec!["snapshots".as_ref(), "list".as_ref()],
            vec!["snapshots".as_ref(), "show".as_ref(), e.scan_id.as_ref()],
        ] {
            h.run(false, &args, 0);
        }
        let live = h.doc(
            &["top".as_ref(), f.root.as_os_str(), "--live".as_ref()],
            0,
            "tidemark.top.v1",
        );
        assert!(live
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["live_disk"].is_number()));
        assert_eq!(catalog.list(None).unwrap().len(), 1);
        let output = h.temp.path().join("export.json");
        let args = [
            "export".as_ref(),
            "ncdu".as_ref(),
            f.root.as_os_str(),
            "--utf8-lossy".as_ref(),
            "-o".as_ref(),
            output.as_os_str(),
        ];
        h.run(json, &args, 0);
        let bytes = std::fs::read(&output).unwrap();
        let ncdu: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(sum(&ncdu[3], "dsize"), snapshot.root().unwrap().t_charged);
        assert!(bytes.windows(6).any(|w| w == "��".as_bytes()));
        assert_eq!(
            std::fs::metadata(&output).unwrap().permissions().mode() & 0o777,
            0o600
        );
        h.run(json, &args, 1);
        assert_eq!(std::fs::read(&output).unwrap(), bytes);
        let inside = h.state.join("forbidden.json");
        let mut forbidden = args;
        forbidden[5] = inside.as_os_str();
        h.run(json, &forbidden, 5);
        assert!(!inside.exists());
        let ncdu_raw = h.run(
            json,
            &["export".as_ref(), "ncdu".as_ref(), f.root.as_os_str()],
            0,
        );
        assert!(ncdu_raw.stdout.windows(2).any(|w| w == [255, 254]));
        for escaped in [b"\\u000a".as_slice(), b"\\\\", b"\\\""] {
            assert!(ncdu_raw.stdout.windows(escaped.len()).any(|w| w == escaped));
        }
        no_temporaries(&h.state);
        snapshots.push(all);
        if json {
            let mut old = catalog.get(&e.scan_id).unwrap().unwrap();
            std::fs::write(f.root.join("d00/added"), vec![0; 16384]).unwrap();
            let new = h.doc(&scan_args, 3, "tidemark.scan.v1");
            let new_id = new["scan_id"].as_str().unwrap();
            old.started_at = tidemark_core::time::unix_now() - 86400;
            let writable = tidemark_store::catalog::Catalog::open(&h.state, true).unwrap();
            writable.record_finish(&old).unwrap();
            let growth = h.doc(
                &[
                    "top".as_ref(),
                    f.root.as_os_str(),
                    "--snapshot".as_ref(),
                    new_id.as_ref(),
                    "--sort".as_ref(),
                    "growth".as_ref(),
                    "--depth".as_ref(),
                    "100".as_ref(),
                    "--limit".as_ref(),
                    "1000".as_ref(),
                ],
                0,
                "tidemark.top.v1",
            );
            let a = growth.as_array().unwrap();
            assert!(a.iter().any(|v| v["delta"].as_i64().unwrap() > 0));
            for pair in a.windows(2) {
                let left: DirRow = serde_json::from_value(pair[0].clone()).unwrap();
                let right: DirRow = serde_json::from_value(pair[1].clone()).unwrap();
                assert!(pair[1]["delta"]
                    .as_i64()
                    .unwrap()
                    .cmp(&pair[0]["delta"].as_i64().unwrap())
                    .then_with(|| left.path.cmp(&right.path))
                    .is_lt());
            }
        }
    }
    assert_eq!(snapshots[0], snapshots[1]);
}
use std::os::unix::ffi::OsStrExt;
