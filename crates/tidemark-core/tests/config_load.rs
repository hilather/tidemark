//! Owned by: W0-B1
//! Configuration loading, default rule merging and resource bounds.

use std::io::Write;
use tidemark_core::config::{Config, MAX_CONFIG_BYTES};

#[test]
fn load_merges_defaults_preserves_override_and_is_idempotent() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(include_bytes!("example.toml")).unwrap();
    let mut config = Config::load(file.path()).unwrap();
    assert!(config.validate().is_empty(), "{:?}", config.validate());
    let rules = config.alerts.rule.clone();
    let override_rule = rules
        .iter()
        .filter(|r| r.name == "fs_full_soon")
        .collect::<Vec<_>>();
    assert_eq!(override_rule.len(), 1);
    assert_eq!(
        override_rule[0].mount.as_deref(),
        Some(std::path::Path::new("/data"))
    );
    assert_eq!(override_rule[0].for_.as_secs(), 600);
    assert!(rules.iter().any(|r| r.name == "thin_pool_pct"));
    config.merge_default_rules();
    assert_eq!(config.alerts.rule, rules);
}

#[test]
fn load_bounds_input_and_reports_missing_file() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(&vec![b' '; MAX_CONFIG_BYTES as usize + 1])
        .unwrap();
    assert!(Config::load(file.path())
        .unwrap_err()
        .to_string()
        .contains("1 MiB"));
    let missing = file.path().with_extension("missing");
    assert!(Config::load(&missing)
        .unwrap_err()
        .to_string()
        .contains("cannot read configuration"));
}

#[test]
fn defaults_follow_configured_mounts_and_duplicate_rules_are_not_hidden() {
    let config: Config =
        toml::from_str("[sampler]\nmounts = ['/srv']\n[watch]\nmounts = ['/srv']").unwrap();
    assert!(config.validate().is_empty());
    assert_eq!(
        config
            .alerts
            .rule
            .iter()
            .filter_map(|r| r.mount.as_deref())
            .collect::<Vec<_>>(),
        vec![std::path::Path::new("/srv"); 4]
    );
    assert_eq!(toml::from_str::<Config>("").unwrap(), Config::default());
    let config: Config = toml::from_str("[[alerts.rule]]\nname = 'x'\nkind = 'scan_stale'\nolder_than = 'auto'\n[[alerts.rule]]\nname = 'x'\nkind = 'scan_stale'\nolder_than = 'auto'").unwrap();
    assert!(config
        .validate()
        .iter()
        .any(|i| i.message.contains("unique")));
    assert!(toml::from_str::<Config>("[[alerts.rule]]\nname = 'x'")
        .unwrap_err()
        .to_string()
        .contains("kind"));
}

#[test]
fn custom_throttle_profile_is_validated() {
    let mut config: Config = toml::from_str("[throttle.profiles.pilot]\nmax_util_pct = 40\ntarget_latency_ms = 12\n[[roots]]\npath = '/srv'\nthrottle = 'pilot'\nschedule = 'off'").unwrap();
    assert!(config.validate().is_empty(), "{:?}", config.validate());
    assert!(config.throttle.profiles.contains_key("normal"));
    assert!(config.throttle.profiles.contains_key("gentle"));
    config.roots[0].throttle = "missing".into();
    assert!(config
        .validate()
        .iter()
        .any(|i| i.field == "roots[0].throttle"));
}

#[test]
fn partial_profile_override_preserves_named_defaults() {
    let config: Config = toml::from_str("[throttle.profiles.gentle]\nmax_util_pct = 40").unwrap();
    assert_eq!(config.throttle.profiles["gentle"].target_latency_ms, 15.0);
    assert_eq!(config.throttle.profiles["gentle"].max_util_pct, 40.0);
    assert_eq!(config.throttle.profiles["normal"].max_util_pct, 70.0);
}

#[test]
fn removed_database_table_is_an_unknown_key() {
    let mut file = tempfile::NamedTempFile::new().unwrap();
    file.write_all(b"[postgres]\nenabled = true\n").unwrap();
    let error = Config::load(file.path()).unwrap_err().to_string();
    assert!(error.contains("unknown field `postgres`"), "{error}");
}

#[test]
fn byte_parser_agrees_with_loader() {
    for bytes in [
        include_bytes!("example.toml").to_vec(),
        vec![b' '; MAX_CONFIG_BYTES as usize + 1],
        vec![0xff],
    ] {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(&bytes).unwrap();
        match (Config::load(file.path()), Config::from_toml_bytes(&bytes)) {
            (Ok(a), Ok(b)) => assert_eq!(a, b),
            (Err(a), Err(b)) => assert_eq!(a.to_string(), b.to_string()),
            other => panic!("loader/parser disagree: {other:?}"),
        }
    }
}

#[test]
fn metrics_textfile_is_disabled_by_default_and_when_omitted() {
    assert!(Config::default().metrics.textfile.as_os_str().is_empty());
    for contents in [
        "",
        "[metrics]\ninterval = '30s'\n",
        include_str!("example.toml"),
    ] {
        let mut file = tempfile::NamedTempFile::new().unwrap();
        file.write_all(contents.as_bytes()).unwrap();
        let config = Config::load(file.path()).unwrap();
        assert!(config.metrics.textfile.as_os_str().is_empty());
        assert!(config.validate().is_empty());
    }
}

#[test]
fn cold_retention_defaults_validation_and_unknown_fields() {
    let mut config: Config = toml::from_str("").unwrap();
    assert!(config.retention.snapshots.cold_after_recent);
    assert_eq!(config.retention.snapshots.cold_level, 9);
    for level in [0, 20] {
        config.retention.snapshots.cold_level = level;
        assert!(config
            .validate()
            .iter()
            .any(|i| i.field == "retention.snapshots.cold_level"));
    }
    for level in [1, 19] {
        config.retention.snapshots.cold_level = level;
        assert!(config.validate().is_empty());
    }
    assert!(toml::from_str::<Config>("[retention.snapshots]\ncold_typo = true").is_err());
    let config: Config =
        toml::from_str("[retention.snapshots]\ncold_after_recent = false\ncold_level = 3").unwrap();
    assert!(!config.retention.snapshots.cold_after_recent);
    assert_eq!(config.retention.snapshots.cold_level, 3);
}

#[test]
fn historical_upgrade_config_receives_cold_defaults() {
    let config = Config::from_toml_bytes(include_bytes!(
        "../../../tests/install/upgrade-config/tidemark.toml"
    ))
    .unwrap();
    assert!(config.validate().is_empty());
    assert!(config.retention.snapshots.cold_after_recent);
    assert_eq!(config.retention.snapshots.cold_level, 9);
}
