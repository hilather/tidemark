//! Owned by: W0-B1-fix
//! Regression evidence for the accepted contract review decisions.

use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use std::{collections::BTreeMap, path::PathBuf, time::Duration};
use tidemark_core::{
    api::{Request, ScanCancelParams, ScanStartParams, ScanStatusParams, WritersParams},
    config::{
        default_rules, AlertKind, AlertRule, AlertThreshold, Config, RootConfig, ThrottleProfile,
    },
    json::JsonDoc,
    live::{
        HotEntry, HotMinute, OpenWriter, PhantomHolder, PhantomSample, PhantomSummary, WriterSample,
    },
    path::RawPath,
    snapshot::{DirRow, ErrorRow, FileHit, LinkSetHit, SnapshotMeta},
    time::ScanId,
    units::{fmt_bytes, fmt_rate, parse_rate},
};

#[allow(clippy::unwrap_used)] // Assertions in this test-only helper report serialization failures.
fn roundtrip<T: Serialize + DeserializeOwned + PartialEq + std::fmt::Debug>(value: T) {
    let wire = serde_json::to_value(&value).unwrap();
    assert_eq!(serde_json::from_value::<T>(wire).unwrap(), value);
}

#[test]
fn removed_http_keys_have_actionable_errors() {
    for (table, key) in [("metrics", "listen"), ("alerts", "webhook")] {
        for value in ["\"\"", "\"http://localhost\"", "42"] {
            let input = format!("[{table}]\n{key} = {value}\n");
            let error = Config::from_toml_bytes(input.as_bytes())
                .unwrap_err()
                .to_string();
            assert!(error.contains(&format!("{table}.{key}")), "{error}");
            assert!(error.contains("removed in v1 (ADR-020)"), "{error}");
            let tmp = tempfile::NamedTempFile::new().unwrap();
            std::fs::write(tmp.path(), input).unwrap();
            assert!(Config::load(tmp.path())
                .unwrap_err()
                .to_string()
                .contains("ADR-020"));
        }
    }
}

#[test]
fn root_wire_envelopes_roundtrip_and_validate() {
    for bytes in [b"/data".as_slice(), "/données".as_bytes(), b"/data/\xff"] {
        let root = RawPath::new(bytes.to_vec()).unwrap();
        let expected = if let Ok(text) = std::str::from_utf8(bytes) {
            json!({"root": text})
        } else {
            json!({"root_b64": "L2RhdGEv/w=="})
        };
        let cancel = ScanCancelParams { root: root.clone() };
        assert_eq!(serde_json::to_value(&cancel).unwrap(), expected);
        roundtrip(cancel);
        roundtrip(ScanStatusParams {
            root: Some(root.clone()),
        });
        let start = ScanStartParams {
            root,
            mode: None,
            throttle: None,
        };
        let wire = serde_json::to_value(&start).unwrap();
        assert_eq!(wire, expected);
        let request = Request {
            v: 1,
            id: "c1".into(),
            method: "scan.start".into(),
            params: wire,
        };
        let decoded: Request =
            serde_json::from_str(&serde_json::to_string(&request).unwrap()).unwrap();
        assert_eq!(
            serde_json::from_value::<ScanStartParams>(decoded.params).unwrap(),
            start
        );
    }
    roundtrip(ScanStatusParams { root: None });
    assert_eq!(
        serde_json::to_value(ScanStatusParams { root: None }).unwrap(),
        json!({})
    );
    for value in [
        json!({}),
        json!({"root":"/data","root_b64":"L2RhdGE="}),
        json!({"root":"relative"}),
        json!({"root":"/a\u{0000}"}),
        json!({"root_b64":"%%%"}),
        json!({"root_b64":"L2RhdGE"}),
        json!({"root":[47]}),
        json!({"root":null,"root_b64":"L2RhdGE="}),
        json!({"root":"/data","root_b64":null}),
        json!({"root":"/data","other":1}),
    ] {
        assert!(
            serde_json::from_value::<ScanStartParams>(value.clone()).is_err(),
            "{value}"
        );
        assert!(serde_json::from_value::<ScanCancelParams>(value).is_err());
    }
    assert!(serde_json::from_value::<ScanStatusParams>(
        json!({"root":"/data","root_b64":"L2RhdGE="})
    )
    .is_err());
    assert_eq!(
        serde_json::from_value::<ScanCancelParams>(json!({"root":"//data/./a/.."}))
            .unwrap()
            .root
            .as_bytes(),
        b"/data"
    );
}

#[test]
fn writer_windows_are_positive_durations_and_schema_is_owned() {
    for (text, seconds) in [("2m", 120), ("1h", 3600), ("60m", 3600), ("1ms", 0)] {
        let value: WritersParams = serde_json::from_value(json!({"window":text})).unwrap();
        assert_eq!(value.window.as_secs(), seconds);
        roundtrip(value);
    }
    assert!(serde_json::from_value::<WritersParams>(json!({"window":"0s"})).is_err());
    assert!(serde_json::to_value(WritersParams {
        mount: None,
        window: Duration::ZERO
    })
    .is_err());
    let input = String::from(r#"{"schema":"tidemark.test.v1","data":42}"#);
    let doc: JsonDoc<u32> = serde_json::from_str(&input).unwrap();
    drop(input);
    assert_eq!(doc.schema, "tidemark.test.v1");
    roundtrip(doc);
}

#[test]
fn byte_and_rate_rounding_boundaries() {
    for (bytes, expected) in [
        (0, "0 B"),
        (1023, "1023 B"),
        (1024, "1.00 KiB"),
        (10239, "10.0 KiB"),
        (102399, "100 KiB"),
        (1048575, "1.00 MiB"),
        (1048576, "1.00 MiB"),
        (1073741823, "1.00 GiB"),
    ] {
        assert_eq!(fmt_bytes(bytes), expected);
        assert_eq!(fmt_rate(bytes as f64), format!("{expected}/s"));
        if bytes > 0 {
            assert_eq!(fmt_rate(-(bytes as f64)), format!("-{expected}/s"));
        }
    }
    assert_eq!(fmt_rate(0.125), "0.125 B/s");
    assert_eq!(fmt_rate(f64::NAN), "unknown");
}

#[test]
fn scan_id_supported_year_boundaries() {
    for (now, prefix) in [
        (-62167219200, "00000101T000000Z-"),
        (253402300799, "99991231T235959Z-"),
        (1709164800, "20240229T000000Z-"),
    ] {
        assert!(ScanId::new(now).unwrap().as_str().starts_with(prefix));
    }
    for now in [-62167219201, 253402300800, i64::MIN, i64::MAX] {
        assert!(ScanId::new(now).is_err());
    }
}

#[test]
fn stale_roots_numeric_rates_and_resource_caps() {
    let mut cfg = Config::default();
    let mut stale = AlertRule {
        name: "manual".into(),
        kind: AlertKind::ScanStale,
        root: Some(PathBuf::from("/manual")),
        older_than: Some("auto".into()),
        ..AlertRule::default()
    };
    cfg.alerts.rule = vec![stale.clone()];
    assert!(cfg.validate().is_empty());
    cfg.roots[0].schedule = "off".into();
    assert!(cfg.validate().is_empty()); // Explicit rules may monitor manual scans.
    stale.root = None;
    cfg.alerts.rule = vec![stale];
    assert!(cfg.validate().iter().any(|i| i.field.ends_with(".root")));
    let mut hot = AlertRule {
        name: "growth".into(),
        kind: AlertKind::HotDirGrowth,
        mount: Some("/data".into()),
        path: Some("/data/**".into()),
        window: Some("5m".into()),
        ..AlertRule::default()
    };
    for (n, valid) in [
        (0.0, true),
        (1024.5, true),
        (-1.0, false),
        (f64::NAN, false),
        (f64::INFINITY, false),
        (f64::NEG_INFINITY, false),
    ] {
        hot.above = Some(AlertThreshold::Number(n));
        cfg.alerts.rule = vec![hot.clone()];
        assert_eq!(cfg.validate().is_empty(), valid);
    }
    for above in [
        AlertThreshold::Number(parse_rate("1KiB/s").unwrap()),
        AlertThreshold::Text("1KiB/s".into()),
    ] {
        hot.above = Some(above);
        cfg.alerts.rule = vec![hot.clone()];
        assert!(cfg.validate().is_empty());
    }
    cfg = Config::default();
    cfg.scan.max_notable = u32::MAX as usize;
    assert!(cfg.validate().is_empty());
    if let Some(too_many) = (u32::MAX as usize).checked_add(1) {
        cfg.scan.max_notable = too_many;
        assert!(cfg.validate().iter().any(|i| i.field == "scan.max_notable"));
    }
    cfg.scan.max_notable = 0;
    assert!(cfg.validate().iter().any(|i| i.field == "scan.max_notable"));
    cfg = Config::default();
    cfg.roots[0].throttle = "max".into();
    assert!(cfg.validate().is_empty());
    cfg.throttle
        .profiles
        .insert("max".into(), ThrottleProfile::default());
    assert!(cfg
        .validate()
        .iter()
        .any(|i| i.field == "throttle.profiles.max"));
}

fn subjects(cfg: &Config) -> BTreeMap<String, PathBuf> {
    default_rules(cfg)
        .into_iter()
        .filter_map(|r| r.mount.or(r.root).map(|subject| (r.name, subject)))
        .collect()
}

#[test]
fn stable_default_names_and_scheduled_roots() {
    use std::os::unix::ffi::OsStringExt;
    let mut cfg = Config::default();
    assert_eq!(subjects(&cfg)["scan_stale"], PathBuf::from("/data"));
    assert_eq!(subjects(&cfg)["scan_stale:/"], PathBuf::from("/"));
    cfg.sampler.mounts = vec!["/srv".into(), "/backup".into()];
    cfg.watch.mounts = cfg.sampler.mounts.clone();
    cfg.roots = vec![
        RootConfig {
            path: "/srv".into(),
            schedule: "every 6h".into(),
            ..RootConfig::default()
        },
        RootConfig {
            path: "/backup".into(),
            schedule: "off".into(),
            ..RootConfig::default()
        },
    ];
    let original = subjects(&cfg);
    assert!(original.contains_key("fs_full_soon:/srv"));
    assert!(!original.contains_key("fs_full_soon"));
    assert!(original.contains_key("scan_stale:/srv"));
    assert!(!original.contains_key("scan_stale:/backup"));
    cfg.sampler.mounts.reverse();
    cfg.watch.mounts.reverse();
    cfg.roots.reverse();
    assert_eq!(subjects(&cfg), original);
    cfg.sampler.mounts.push("/data".into());
    assert_eq!(subjects(&cfg)["fs_full_soon"], PathBuf::from("/data"));
    for (name, path) in &original {
        assert_eq!(&subjects(&cfg)[name], path);
    }
    cfg.sampler
        .mounts
        .retain(|m| m != &PathBuf::from("/backup"));
    assert_eq!(subjects(&cfg)["fs_full_soon:/srv"], PathBuf::from("/srv"));
    cfg.alerts.rule = vec![AlertRule {
        name: "fs_full_soon:/srv".into(),
        kind: AlertKind::TimeToFull,
        mount: Some("/srv".into()),
        below: Some("24h".into()),
        window: Some("1h".into()),
        ..AlertRule::default()
    }];
    cfg.merge_default_rules();
    assert_eq!(
        cfg.alerts
            .rule
            .iter()
            .filter(|r| r.name == "fs_full_soon:/srv")
            .count(),
        1
    );
    assert_eq!(
        cfg.alerts
            .rule
            .iter()
            .find(|r| r.name == "fs_full_soon:/srv")
            .unwrap()
            .below
            .as_deref(),
        Some("24h")
    );
    cfg.sampler.mounts = vec![
        PathBuf::from(std::ffi::OsString::from_vec(b"/x\xff".to_vec())),
        "/x\\xFF".into(),
    ];
    let names = subjects(&cfg);
    assert!(names.contains_key("fs_full_soon:/x%FF"));
    assert!(names.contains_key("fs_full_soon:/x%5CxFF"));
}

#[test]
fn percent_encoded_default_names_and_disabled_overrides() {
    for (subject, name) in [
        ("/srv:blue", "fs_full_soon:/srv%3Ablue"),
        ("/x%y", "fs_full_soon:/x%25y"),
        ("/x y", "fs_full_soon:/x%20y"),
        ("/x\\y", "fs_full_soon:/x%5Cy"),
        ("/données", "fs_full_soon:/donn%C3%A9es"),
        ("/AZaz09._-", "fs_full_soon:/AZaz09._-"),
    ] {
        let mut cfg = Config::default();
        cfg.sampler.mounts = vec![subject.into()];
        assert_eq!(subjects(&cfg)[name], PathBuf::from(subject));
        cfg.alerts.rule = vec![AlertRule {
            name: name.into(),
            kind: AlertKind::TimeToFull,
            enabled: false,
            mount: Some(subject.into()),
            below: Some("12h".into()),
            window: Some("1h".into()),
            ..AlertRule::default()
        }];
        cfg.merge_default_rules();
        cfg.merge_default_rules();
        assert!(cfg.validate().is_empty(), "{:?}", cfg.validate());
        let rules: Vec<_> = cfg.alerts.rule.iter().filter(|r| r.name == name).collect();
        assert_eq!(rules.len(), 1);
        assert!(!rules[0].enabled);
    }
}

#[test]
fn complete_live_report_path_outputs() {
    for (path_fields, bytes) in [
        (json!({"path":"/data/é"}), "/data/é".as_bytes().to_vec()),
        (json!({"path_b64":"L2RhdGEv/w=="}), b"/data/\xff".to_vec()),
    ] {
        let mut writer = json!({"mount":"/data","pid":1,"comm":"writer","unit":null,"dev":2,"ino":3,
            "size":4,"disk":5,"growth":6,"window_secs":120});
        writer
            .as_object_mut()
            .unwrap()
            .extend(path_fields.as_object().unwrap().clone());
        let value: OpenWriter = serde_json::from_value(writer.clone()).unwrap();
        assert_eq!(value.path, bytes);
        let doc = JsonDoc {
            schema: "tidemark.writers.v1".into(),
            data: vec![value],
        };
        assert_eq!(
            serde_json::to_value(&doc).unwrap(),
            json!({"schema":"tidemark.writers.v1","data":[writer]})
        );
        roundtrip(doc);
        let mut holder = json!({"pid":1,"comm":"writer","unit":null,"dev":2,"ino":3,"bytes":4});
        holder
            .as_object_mut()
            .unwrap()
            .extend(path_fields.as_object().unwrap().clone());
        let summary = json!({"mount":"/data","total_bytes":4,"files":1,"holders":[holder]});
        let value: PhantomSummary = serde_json::from_value(summary.clone()).unwrap();
        assert_eq!(value.holders[0].path, bytes);
        assert_eq!(serde_json::to_value(&value).unwrap(), summary);
        roundtrip(value);
        let mut hot = json!({"events":1,"files":2,"bytes_delta":3,"top_unit":null,"lossy":false});
        hot.as_object_mut()
            .unwrap()
            .extend(path_fields.as_object().unwrap().clone());
        let value: HotEntry = serde_json::from_value(hot.clone()).unwrap();
        assert_eq!(value.key, bytes);
        assert_eq!(serde_json::to_value(&value).unwrap(), hot);
        roundtrip(value);
        let mut minute = hot.clone();
        minute.as_object_mut().unwrap().remove("top_unit");
        minute.as_object_mut().unwrap().extend(
            json!({"mount":"/data","scope":"dir","ts":0})
                .as_object()
                .unwrap()
                .clone(),
        );
        roundtrip(serde_json::from_value::<HotMinute>(minute).unwrap());
        let mut sample = holder.clone();
        sample
            .as_object_mut()
            .unwrap()
            .extend(json!({"mount":"/data","ts":0}).as_object().unwrap().clone());
        roundtrip(serde_json::from_value::<PhantomSample>(sample).unwrap());
        roundtrip(serde_json::from_value::<PhantomHolder>(holder).unwrap());
        let mut sample = writer.clone();
        sample.as_object_mut().unwrap().remove("growth");
        sample.as_object_mut().unwrap().remove("window_secs");
        sample["ts"] = json!(0);
        roundtrip(serde_json::from_value::<WriterSample>(sample).unwrap());
    }
}

#[test]
fn snapshot_hit_paths_and_errors_roundtrip() {
    for fields in [
        json!({"path":"/data/é"}),
        json!({"path_b64":"L2RhdGEv/w=="}),
    ] {
        let row = json!({"id":0,"dir_id":0,"name":[120],"ino":1,"disk":2,"apparent":3,"mtime":4,"nlink":1});
        let mut hit = json!({"row":row});
        hit.as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let value: FileHit = serde_json::from_value(hit.clone()).unwrap();
        assert_eq!(serde_json::to_value(&value).unwrap(), hit);
        roundtrip(value);
        let mut links = json!({"row":{"ino":1,"nlink":2,"found":2,"complete":true,"disk":2,"apparent":3,"mtime":4,"lca_dir_id":0},"instance_count":2});
        links
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        let value: LinkSetHit = serde_json::from_value(links.clone()).unwrap();
        assert_eq!(serde_json::to_value(&value).unwrap(), links);
        roundtrip(value);
        let mut error = json!({"dir_id":null,"op":"statx","errno":2});
        error
            .as_object_mut()
            .unwrap()
            .extend(fields.as_object().unwrap().clone());
        roundtrip(serde_json::from_value::<ErrorRow>(error).unwrap());
    }
    let unresolved = json!({"row":{"id":0,"dir_id":0,"name":null,"ino":1,"disk":2,"apparent":3,"mtime":4,"nlink":1}});
    let value: FileHit = serde_json::from_value(unresolved.clone()).unwrap();
    assert!(value.path.is_none());
    assert_eq!(serde_json::to_value(value).unwrap(), unresolved);
    assert!(serde_json::from_value::<FileHit>(
        json!({"row":unresolved["row"],"path":"/data","path_b64":"L2RhdGE="})
    )
    .is_err());
}

#[test]
fn complete_snapshot_and_now_envelopes() {
    use tidemark_core::{live::NowReport, scan::ScanErrorRec};
    for (path_fields, root_fields) in [
        (json!({"path":"/data/é"}), json!({"root":"/data/é"})),
        (
            json!({"path_b64":"L2RhdGEv/w=="}),
            json!({"root_b64":"L2RhdGEv/w=="}),
        ),
    ] {
        let mut wire = json!({"schema_version": 0, "tool_version": "", "scan_id": "19700101T000000Z-00000000", "root_display": "", "mode": "walk", "mode_reason": "", "status": "complete", "degraded": false, "exclude": [], "link_instances": "all", "started_at": 0, "finished_at": 0, "host": "", "kernel": "", "os_release": "", "fs_uuid": "", "fs_dev": 0, "fs_blocksize": 0, "threads": 0, "throttle_profile": "", "dirs": 0, "files": 0, "inodes": 0, "errors": 0, "races": 0, "skipped": 0, "noatime_fallbacks": 0, "phase_ms": {}, "gate_stats": {"mean_rate": 0.0, "min_rate": 0.0, "throttled_ms": 0, "decreases": 0, "increases": 0}});
        wire.as_object_mut()
            .unwrap()
            .extend(root_fields.as_object().unwrap().clone());
        let value: SnapshotMeta = serde_json::from_value(wire.clone()).unwrap();
        let doc = JsonDoc {
            schema: "tidemark.snapshots.v1".into(),
            data: value,
        };
        assert_eq!(
            serde_json::to_value(&doc).unwrap(),
            json!({"schema":"tidemark.snapshots.v1","data":wire})
        );
        roundtrip(doc);
        let mut wire = json!({"id": 0, "parent_id": null, "subtree_end": 0, "depth": 0, "name": [], "path_hash": 0, "ino": 0, "mtime": 0, "dir_disk": 0, "dir_apparent": 0, "projid": null, "entries": 0, "errors": 0, "own_files": 0, "own_symlinks": 0, "own_charged": 0, "own_apparent": 0, "t_dirs": 0, "t_files": 0, "t_inodes": 0, "t_disk": 0, "t_apparent": 0, "t_charged": 0, "t_reclaimable": 0, "t_errors": 0, "age0": 0, "age1": 0, "age2": 0, "age3": 0, "age4": 0, "age5": 0, "newest_mtime": 0, "max_entries": 0});
        wire.as_object_mut()
            .unwrap()
            .extend(path_fields.as_object().unwrap().clone());
        let value: DirRow = serde_json::from_value(wire.clone()).unwrap();
        let doc = JsonDoc {
            schema: "tidemark.snapshots.v1".into(),
            data: value,
        };
        assert_eq!(
            serde_json::to_value(&doc).unwrap(),
            json!({"schema":"tidemark.snapshots.v1","data":wire})
        );
        roundtrip(doc);
        let mut scan_error = json!({"op":"open","errno":2});
        scan_error
            .as_object_mut()
            .unwrap()
            .extend(path_fields.as_object().unwrap().clone());
        roundtrip(serde_json::from_value::<ScanErrorRec>(scan_error).unwrap());
        let mut writer = json!({"mount":"/data","pid":1,"comm":"writer","unit":null,"dev":2,"ino":3,
            "size":4,"disk":5,"growth":6,"window_secs":120});
        writer
            .as_object_mut()
            .unwrap()
            .extend(path_fields.as_object().unwrap().clone());
        let mut holder = json!({"pid":1,"comm":"writer","unit":null,"dev":2,"ino":3,"bytes":4});
        holder
            .as_object_mut()
            .unwrap()
            .extend(path_fields.as_object().unwrap().clone());
        let mut hot = json!({"events":1,"files":2,"bytes_delta":3,"top_unit":null,"lossy":false});
        hot.as_object_mut()
            .unwrap()
            .extend(path_fields.as_object().unwrap().clone());
        let now = json!({"generated_at":0,"host":"test","alerts_firing":[],"scans":[],"degraded":[],
            "mounts":[{"mount":"/data","size":100,"used":10,"avail":90,"rate_15m":null,"rate_1h":null,
                "ttf_secs":null,"lvm":null,"hot_dirs_5m":[hot],"writers_5m":[],
                "phantom":{"mount":"/data","total_bytes":4,"files":1,"holders":[holder]},
                "open_writers_top":[writer],"watch_mode":"fid","lossy_minutes_15m":0}]});
        let report: NowReport = serde_json::from_value(now.clone()).unwrap();
        let doc = JsonDoc {
            schema: "tidemark.now.v1".into(),
            data: report,
        };
        assert_eq!(
            serde_json::to_value(&doc).unwrap(),
            json!({"schema":"tidemark.now.v1","data":now})
        );
        roundtrip(doc);
    }
}

#[test]
fn hot_labels_retain_keys_and_path_representations_conflict() {
    let hot = json!({"key":[119,114,105,116,101,114],"events":1,"files":2,"bytes_delta":3,"top_unit":null,"lossy":false});
    let entry: HotEntry = serde_json::from_value(hot.clone()).unwrap();
    assert_eq!(serde_json::to_value(&entry).unwrap(), hot);
    roundtrip(entry);
    let minute = json!({"key":[255],"mount":"/data","scope":"comm","ts":0,"events":1,"files":2,"bytes_delta":3,"lossy":false});
    let entry: HotMinute = serde_json::from_value(minute.clone()).unwrap();
    assert_eq!(serde_json::to_value(&entry).unwrap(), minute);
    roundtrip(entry);
    let mut conflicting = hot.clone();
    conflicting["path"] = json!("/data");
    assert!(serde_json::from_value::<HotEntry>(conflicting).is_err());
    let mut conflicting = hot;
    conflicting.as_object_mut().unwrap().remove("key");
    conflicting["path"] = json!("/data");
    conflicting["path_b64"] = json!("L2RhdGE=");
    assert!(serde_json::from_value::<HotEntry>(conflicting).is_err());
}
