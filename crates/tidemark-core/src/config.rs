//! Owned by: W0-B1
//! Config contracts and helpers.

use crate::{
    error::CoreError,
    time::parse_duration,
    units::{parse_rate, ByteSize},
};
use serde::{Deserialize, Serialize};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::{Path, PathBuf},
    time::Duration,
};
/// RootMode configuration choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RootMode {
    /// Auto setting.
    /// Auto variant.
    Auto,
    /// Walk setting.
    /// Walk variant.
    Walk,
    /// Bulk setting.
    /// Bulk variant.
    Bulk,
}
/// ThrottlePreset configuration choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ThrottlePreset {
    /// Gentle setting.
    /// Gentle variant.
    Gentle,
    /// Normal setting.
    /// Normal variant.
    Normal,
    /// Max setting.
    /// Max variant.
    Max,
}
/// LinkInstances configuration choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum LinkInstances {
    /// All setting.
    /// All variant.
    All,
    /// None setting.
    /// None variant.
    None,
}
/// AlertKind configuration choices.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AlertKind {
    /// FsUsedPct setting.
    /// FsUsedPct variant.
    FsUsedPct,
    /// TimeToFull setting.
    /// TimeToFull variant.
    TimeToFull,
    /// HotDirGrowth setting.
    /// HotDirGrowth variant.
    HotDirGrowth,
    /// ThinPoolPct setting.
    /// ThinPoolPct variant.
    ThinPoolPct,
    /// VgFreeLow setting.
    /// VgFreeLow variant.
    VgFreeLow,
    /// DirGrowth setting.
    /// DirGrowth variant.
    DirGrowth,
    /// PhantomBytes setting.
    /// PhantomBytes variant.
    PhantomBytes,
    /// ScanStale setting.
    /// ScanStale variant.
    ScanStale,
    /// WatchLossy setting.
    /// WatchLossy variant.
    WatchLossy,
}
/// Numeric or unit-bearing alert threshold, interpreted by rule kind.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum AlertThreshold {
    /// Percentage or count.
    /// Number variant.
    Number(f64),
    /// Size, duration or rate string.
    /// Text variant.
    Text(String),
}
/// GeneralConfig settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct GeneralConfig {
    /// state_dir setting.
    pub state_dir: PathBuf,
    /// socket setting.
    pub socket: PathBuf,
    /// log_level setting.
    pub log_level: String,
}
impl Default for GeneralConfig {
    fn default() -> Self {
        Self {
            state_dir: PathBuf::from("/var/lib/tidemark"),
            socket: PathBuf::from("/run/tidemark/api.sock"),
            log_level: "info".into(),
        }
    }
}
/// RootConfig settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RootConfig {
    /// path setting.
    pub path: PathBuf,
    /// mode setting.
    pub mode: RootMode,
    /// schedule setting.
    pub schedule: String,
    /// window setting.
    pub window: Option<String>,
    /// throttle setting.
    pub throttle: String,
    /// exclude setting.
    pub exclude: Vec<String>,
    /// series_depth setting.
    pub series_depth: u32,
    /// big_file_threshold setting.
    pub big_file_threshold: ByteSize,
    /// top_files setting.
    pub top_files: usize,
}
impl Default for RootConfig {
    fn default() -> Self {
        Self {
            path: PathBuf::from("/data"),
            mode: RootMode::Auto,
            schedule: "every 6h".into(),
            window: None,
            throttle: "normal".into(),
            exclude: vec![],
            series_depth: 2,
            big_file_threshold: ByteSize(1073741824),
            top_files: 10000,
        }
    }
}
/// ScanConfig settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ScanConfig {
    /// threads setting.
    pub threads: u32,
    /// memory_budget setting.
    pub memory_budget: ByteSize,
    /// getdents_buf setting.
    pub getdents_buf: ByteSize,
    /// bulk_batch setting.
    pub bulk_batch: u32,
    /// link_instances setting.
    pub link_instances: LinkInstances,
    /// Positive notable-file cap, at most `u32::MAX`; consumers assign checked zero-based IDs.
    pub max_notable: usize,
    /// max_stored_errors setting.
    pub max_stored_errors: usize,
}
impl Default for ScanConfig {
    fn default() -> Self {
        Self {
            threads: 8,
            memory_budget: ByteSize(1342177280),
            getdents_buf: ByteSize(262144),
            bulk_batch: 4096,
            link_instances: LinkInstances::All,
            max_notable: 200000,
            max_stored_errors: 10000,
        }
    }
}
/// SamplerConfig settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SamplerConfig {
    /// mounts setting.
    pub mounts: Vec<PathBuf>,
    /// interval setting.
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
}
impl Default for SamplerConfig {
    fn default() -> Self {
        Self {
            mounts: vec![PathBuf::from("/"), PathBuf::from("/data")],
            interval: Duration::from_secs(30),
        }
    }
}
/// LvmConfig settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LvmConfig {
    /// enabled setting.
    pub enabled: bool,
    /// interval setting.
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
    /// timeout setting.
    #[serde(with = "humantime_serde")]
    pub timeout: Duration,
}
impl Default for LvmConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: Duration::from_secs(300),
            timeout: Duration::from_secs(30),
        }
    }
}
/// WatchConfig settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct WatchConfig {
    /// enabled setting.
    pub enabled: bool,
    /// mounts setting.
    pub mounts: Vec<PathBuf>,
    /// events setting.
    pub events: Vec<String>,
    /// hot_depth setting.
    pub hot_depth: u32,
    /// exclude_prefixes setting.
    pub exclude_prefixes: Vec<PathBuf>,
    /// lru_entries setting.
    pub lru_entries: usize,
}
impl Default for WatchConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            mounts: vec![PathBuf::from("/"), PathBuf::from("/data")],
            events: vec!["close_write".into()],
            hot_depth: 6,
            exclude_prefixes: vec![],
            lru_entries: 200000,
        }
    }
}
/// ProcscanConfig settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ProcscanConfig {
    /// enabled setting.
    pub enabled: bool,
    /// interval setting.
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
    /// open_writers setting.
    pub open_writers: bool,
    /// exclude_prefixes setting.
    pub exclude_prefixes: Vec<PathBuf>,
}
impl Default for ProcscanConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            interval: Duration::from_secs(60),
            open_writers: true,
            exclude_prefixes: vec![],
        }
    }
}
/// MetricsConfig settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct MetricsConfig {
    /// textfile setting.
    pub textfile: PathBuf,
    /// interval setting.
    #[serde(with = "humantime_serde")]
    pub interval: Duration,
    /// dir_metrics_depth setting.
    pub dir_metrics_depth: u32,
    /// max_series_per_metric setting.
    pub max_series_per_metric: usize,
}
impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            textfile: PathBuf::new(),
            interval: Duration::from_secs(60),
            dir_metrics_depth: 2,
            max_series_per_metric: 500,
        }
    }
}
/// SnapshotRetention settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SnapshotRetention {
    /// Compress retained snapshots outside the recent set.
    pub cold_after_recent: bool,
    /// Zstd compression level (1 through 19).
    pub cold_level: i32,
    /// recent setting.
    pub recent: usize,
    /// daily setting.
    pub daily: usize,
    /// weekly setting.
    pub weekly: usize,
}
impl Default for SnapshotRetention {
    fn default() -> Self {
        Self {
            cold_after_recent: true,
            cold_level: 9,
            recent: 4,
            daily: 14,
            weekly: 8,
        }
    }
}
/// SampleRetention settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SampleRetention {
    /// raw setting.
    #[serde(with = "humantime_serde")]
    pub raw: Duration,
    /// five_min setting.
    #[serde(with = "humantime_serde")]
    pub five_min: Duration,
    /// hourly setting.
    #[serde(with = "humantime_serde")]
    pub hourly: Duration,
    /// hot_minutes setting.
    #[serde(with = "humantime_serde")]
    pub hot_minutes: Duration,
}
impl Default for SampleRetention {
    fn default() -> Self {
        Self {
            raw: Duration::from_secs(172800),
            five_min: Duration::from_secs(3024000),
            hourly: Duration::from_secs(34560000),
            hot_minutes: Duration::from_secs(604800),
        }
    }
}
/// ThrottleProfile settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThrottleProfile {
    /// max_util_pct setting.
    pub max_util_pct: f64,
    /// target_latency_ms setting.
    pub target_latency_ms: f64,
}
impl Default for ThrottleProfile {
    fn default() -> Self {
        Self {
            max_util_pct: 70.0,
            target_latency_ms: 25.0,
        }
    }
}
/// ThrottleConfig settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ThrottleConfig {
    /// max_ops_per_sec setting.
    pub max_ops_per_sec: u32,
    /// min_ops_per_sec setting.
    pub min_ops_per_sec: u32,
    /// sample_interval setting.
    #[serde(with = "humantime_serde")]
    pub sample_interval: Duration,
    /// User profiles; `max` is reserved for the no-throttling selector.
    #[serde(deserialize_with = "deserialize_profiles")]
    pub profiles: BTreeMap<String, ThrottleProfile>,
}
impl Default for ThrottleConfig {
    fn default() -> Self {
        Self {
            max_ops_per_sec: 50000,
            min_ops_per_sec: 200,
            sample_interval: Duration::from_millis(500),
            profiles: default_profiles(),
        }
    }
}
fn default_profiles() -> BTreeMap<String, ThrottleProfile> {
    BTreeMap::from([
        (
            "gentle".into(),
            ThrottleProfile {
                max_util_pct: 50.0,
                target_latency_ms: 15.0,
            },
        ),
        ("normal".into(), ThrottleProfile::default()),
    ])
}
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
struct ProfileOverride {
    max_util_pct: Option<f64>,
    target_latency_ms: Option<f64>,
}
fn deserialize_profiles<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<BTreeMap<String, ThrottleProfile>, D::Error> {
    let overrides = BTreeMap::<String, ProfileOverride>::deserialize(deserializer)?;
    let mut profiles = default_profiles();
    for (name, overrides) in overrides {
        let profile = profiles.entry(name).or_default();
        if let Some(value) = overrides.max_util_pct {
            profile.max_util_pct = value;
        }
        if let Some(value) = overrides.target_latency_ms {
            profile.target_latency_ms = value;
        }
    }
    Ok(profiles)
}

/// RetentionConfig settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct RetentionConfig {
    /// max_state_size setting.
    pub max_state_size: ByteSize,
    /// min_free_pct setting.
    pub min_free_pct: f64,
    /// snapshots setting.
    pub snapshots: SnapshotRetention,
    /// samples setting.
    pub samples: SampleRetention,
}
impl Default for RetentionConfig {
    fn default() -> Self {
        Self {
            max_state_size: ByteSize(20 << 30),
            min_free_pct: 5.0,
            snapshots: SnapshotRetention::default(),
            samples: SampleRetention::default(),
        }
    }
}
/// AlertRule settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AlertRule {
    /// name setting.
    pub name: String,
    /// kind setting.
    pub kind: AlertKind,
    /// enabled setting.
    #[serde(default = "rule_enabled_default")]
    pub enabled: bool,
    /// for_ setting.
    #[serde(with = "humantime_serde")]
    #[serde(rename = "for", default = "rule_for_default")]
    pub for_: Duration,
    /// resolve_for setting.
    #[serde(with = "humantime_serde")]
    #[serde(default = "rule_resolve_default")]
    pub resolve_for: Duration,
    /// mount setting.
    pub mount: Option<PathBuf>,
    /// Threshold; numeric hot-directory growth values are bytes per second.
    pub above: Option<AlertThreshold>,
    /// below setting.
    pub below: Option<String>,
    /// window setting.
    pub window: Option<String>,
    /// path setting.
    pub path: Option<String>,
    /// vg setting.
    pub vg: Option<String>,
    /// pool setting.
    pub pool: Option<String>,
    /// data_above setting.
    pub data_above: Option<f64>,
    /// meta_above setting.
    pub meta_above: Option<f64>,
    /// root setting.
    pub root: Option<PathBuf>,
    /// depth setting.
    pub depth: Option<u32>,
    /// older_than setting.
    pub older_than: Option<String>,
}
fn rule_enabled_default() -> bool {
    true
}
fn rule_for_default() -> Duration {
    Duration::from_secs(300)
}
fn rule_resolve_default() -> Duration {
    Duration::from_secs(600)
}

impl Default for AlertRule {
    fn default() -> Self {
        Self {
            name: String::new(),
            kind: AlertKind::FsUsedPct,
            enabled: true,
            for_: Duration::from_secs(300),
            resolve_for: Duration::from_secs(600),
            mount: None,
            above: None,
            below: None,
            window: None,
            path: None,
            vg: None,
            pool: None,
            data_above: None,
            meta_above: None,
            root: None,
            depth: None,
            older_than: None,
        }
    }
}
/// AlertsConfig settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct AlertsConfig {
    /// renotify setting.
    #[serde(with = "humantime_serde")]
    pub renotify: Duration,
    /// rule setting.
    pub rule: Vec<AlertRule>,
}
impl Default for AlertsConfig {
    fn default() -> Self {
        Self {
            renotify: Duration::from_secs(4 * 3600),
            rule: Vec::new(),
        }
    }
}
/// Config settings from 08 §3.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// general setting.
    pub general: GeneralConfig,
    /// roots setting.
    pub roots: Vec<RootConfig>,
    /// scan setting.
    pub scan: ScanConfig,
    /// throttle setting.
    pub throttle: ThrottleConfig,
    /// sampler setting.
    pub sampler: SamplerConfig,
    /// lvm setting.
    pub lvm: LvmConfig,
    /// watch setting.
    pub watch: WatchConfig,
    /// procscan setting.
    pub procscan: ProcscanConfig,
    /// metrics setting.
    pub metrics: MetricsConfig,
    /// alerts setting.
    pub alerts: AlertsConfig,
    /// retention setting.
    pub retention: RetentionConfig,
}
impl Default for Config {
    fn default() -> Self {
        let mut config = Self {
            general: GeneralConfig::default(),
            roots: vec![
                RootConfig {
                    window: Some("00:00-06:00".into()),
                    ..RootConfig::default()
                },
                RootConfig {
                    path: PathBuf::from("/"),
                    mode: RootMode::Walk,
                    schedule: "daily 03:30".into(),
                    window: None,
                    big_file_threshold: ByteSize(256 << 20),
                    top_files: 2000,
                    ..RootConfig::default()
                },
            ],
            scan: ScanConfig::default(),
            throttle: ThrottleConfig::default(),
            sampler: SamplerConfig::default(),
            lvm: LvmConfig::default(),
            watch: WatchConfig::default(),
            procscan: ProcscanConfig::default(),
            metrics: MetricsConfig::default(),
            alerts: AlertsConfig::default(),
            retention: RetentionConfig::default(),
        };
        config.merge_default_rules();
        config
    }
}
// Separate deserialization defaults from effective defaults: subjects depend on
// the parsed mounts, not the built-in mount list.
#[derive(Debug, Clone, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct ConfigInput {
    general: GeneralConfig,
    roots: Vec<RootConfig>,
    scan: ScanConfig,
    throttle: ThrottleConfig,
    sampler: SamplerConfig,
    lvm: LvmConfig,
    watch: WatchConfig,
    procscan: ProcscanConfig,
    metrics: MetricsConfig,
    alerts: AlertsConfig,
    retention: RetentionConfig,
}
impl Default for ConfigInput {
    fn default() -> Self {
        Self {
            general: GeneralConfig::default(),
            roots: Config::default().roots,
            scan: ScanConfig::default(),
            throttle: ThrottleConfig::default(),
            sampler: SamplerConfig::default(),
            lvm: LvmConfig::default(),
            watch: WatchConfig::default(),
            procscan: ProcscanConfig::default(),
            metrics: MetricsConfig::default(),
            alerts: AlertsConfig::default(),
            retention: RetentionConfig::default(),
        }
    }
}
impl<'de> Deserialize<'de> for Config {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let input = ConfigInput::deserialize(deserializer)?;
        let mut config = Self {
            general: input.general,
            roots: input.roots,
            scan: input.scan,
            throttle: input.throttle,
            sampler: input.sampler,
            lvm: input.lvm,
            watch: input.watch,
            procscan: input.procscan,
            metrics: input.metrics,
            alerts: input.alerts,
            retention: input.retention,
        };
        config.merge_default_rules();
        Ok(config)
    }
}
/// Maximum config input size (1 MiB); bounds file reading and parser allocations.
pub const MAX_CONFIG_BYTES: u64 = 1024 * 1024;

/// A validation issue tied to a configuration field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ConfigIssue {
    /// Dotted field location.
    pub field: String,
    /// Actionable explanation.
    pub message: String,
}
impl std::fmt::Display for ConfigIssue {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.field, self.message)
    }
}
impl std::error::Error for ConfigIssue {}
impl Config {
    /// Read exactly one TOML file, capped at 1 MiB. Missing files are reported to the caller.
    pub fn load(path: &Path) -> Result<Self, CoreError> {
        use std::io::Read;
        let file = std::fs::File::open(path)?;
        let mut bytes = Vec::new();
        file.take(MAX_CONFIG_BYTES + 1).read_to_end(&mut bytes)?;
        Self::from_toml_bytes(&bytes)
    }
    /// Parse bounded UTF-8 TOML bytes and merge the configured default rules.
    pub fn from_toml_bytes(bytes: &[u8]) -> Result<Self, CoreError> {
        // Match load's bounded read and UTF-8 error precedence, including its
        // one-byte lookahead, without processing an unbounded caller buffer.
        let bounded = &bytes[..bytes.len().min(MAX_CONFIG_BYTES as usize + 1)];
        let text = std::str::from_utf8(bounded).map_err(|_| {
            std::io::Error::new(
                std::io::ErrorKind::InvalidData,
                "stream did not contain valid UTF-8",
            )
        })?;
        if bytes.len() as u64 > MAX_CONFIG_BYTES {
            return Err(CoreError::Parse(
                "configuration exceeds the 1 MiB limit".into(),
            ));
        }
        let value: toml::Value = toml::from_str(text)?;
        for (table, key) in [("metrics", "listen"), ("alerts", "webhook")] {
            if value.get(table).and_then(|v| v.get(key)).is_some() {
                return Err(CoreError::Parse(format!(
                    "{table}.{key}: feature was removed in v1 (ADR-020)"
                )));
            }
        }
        let mut config: Self = toml::from_str(text)?;
        config.merge_default_rules();
        Ok(config)
    }
    /// Validate without checking filesystem existence or performing I/O.
    pub fn validate(&self) -> Vec<ConfigIssue> {
        let mut issues = Vec::new();
        self.validate_into(&mut issues);
        issues
    }
}
fn issue(issues: &mut Vec<ConfigIssue>, field: impl Into<String>, message: impl Into<String>) {
    issues.push(ConfigIssue {
        field: field.into(),
        message: message.into(),
    });
}
fn absolute(issues: &mut Vec<ConfigIssue>, field: impl Into<String>, path: &Path) {
    if !path.is_absolute() {
        issue(issues, field, "path must be absolute");
    }
}
fn clock_time(s: &str) -> bool {
    let Some((h, m)) = s.split_once(':') else {
        return false;
    };
    h.len() == 2
        && m.len() == 2
        && h.bytes().all(|b| b.is_ascii_digit())
        && m.bytes().all(|b| b.is_ascii_digit())
        && h.parse::<u8>().is_ok_and(|h| h < 24)
        && m.parse::<u8>().is_ok_and(|m| m < 60)
}
fn positive_duration(s: &str) -> bool {
    parse_duration(s).is_ok_and(|d| !d.is_zero())
}
impl Config {
    fn validate_into(&self, issues: &mut Vec<ConfigIssue>) {
        absolute(issues, "general.state_dir", &self.general.state_dir);
        absolute(issues, "general.socket", &self.general.socket);
        if !matches!(
            self.general.log_level.as_str(),
            "error" | "warn" | "info" | "debug" | "trace"
        ) {
            issue(
                issues,
                "general.log_level",
                "expected error, warn, info, debug or trace",
            );
        }
        let mut roots = BTreeSet::new();
        for (i, root) in self.roots.iter().enumerate() {
            let field = format!("roots[{i}]");
            absolute(issues, format!("{field}.path"), &root.path);
            if !roots.insert(&root.path) {
                issue(issues, format!("{field}.path"), "duplicate root");
            }
            if root.throttle != "max" && !self.throttle.profiles.contains_key(&root.throttle) {
                issue(
                    issues,
                    format!("{field}.throttle"),
                    "unknown throttle profile",
                );
            }
            let schedule = &root.schedule;
            if !(schedule == "off"
                || schedule
                    .strip_prefix("every ")
                    .is_some_and(positive_duration)
                || schedule.strip_prefix("daily ").is_some_and(clock_time))
            {
                issue(
                    issues,
                    format!("{field}.schedule"),
                    "expected every <duration>, daily HH:MM or off",
                );
            }
            if let Some(window) = &root.window {
                if !window
                    .split_once('-')
                    .is_some_and(|(a, b)| clock_time(a) && clock_time(b) && a != b)
                {
                    issue(
                        issues,
                        format!("{field}.window"),
                        "expected HH:MM-HH:MM with distinct endpoints",
                    );
                }
            }
            for (j, glob) in root.exclude.iter().enumerate() {
                if !glob.starts_with('/') {
                    issue(
                        issues,
                        format!("{field}.exclude[{j}]"),
                        "glob must use a full absolute path",
                    );
                }
            }
        }
        for (field, mounts) in [
            ("sampler.mounts", &self.sampler.mounts),
            ("watch.mounts", &self.watch.mounts),
            ("watch.exclude_prefixes", &self.watch.exclude_prefixes),
            ("procscan.exclude_prefixes", &self.procscan.exclude_prefixes),
        ] {
            for (i, mount) in mounts.iter().enumerate() {
                absolute(issues, format!("{field}[{i}]"), mount);
            }
        }
        if !self.metrics.textfile.as_os_str().is_empty() {
            absolute(issues, "metrics.textfile", &self.metrics.textfile);
        }
        if self.scan.max_notable > u32::MAX as usize {
            issue(
                issues,
                "scan.max_notable",
                "must be at most u32::MAX (4294967295)",
            );
        }
        if !(1..=64).contains(&self.scan.threads) {
            issue(issues, "scan.threads", "expected 1–64 workers");
        }
        for (field, value) in [
            ("scan.memory_budget", self.scan.memory_budget.0),
            ("scan.getdents_buf", self.scan.getdents_buf.0),
            ("scan.bulk_batch", u64::from(self.scan.bulk_batch)),
            ("scan.max_notable", self.scan.max_notable as u64),
            ("watch.lru_entries", self.watch.lru_entries as u64),
            (
                "metrics.max_series_per_metric",
                self.metrics.max_series_per_metric as u64,
            ),
        ] {
            if value == 0 {
                issue(issues, field, "must be positive");
            }
        }
        if self.throttle.min_ops_per_sec == 0
            || self.throttle.min_ops_per_sec > self.throttle.max_ops_per_sec
        {
            issue(
                issues,
                "throttle.min_ops_per_sec",
                "must be positive and at most max_ops_per_sec",
            );
        }
        for (field, duration) in [
            ("throttle.sample_interval", self.throttle.sample_interval),
            ("sampler.interval", self.sampler.interval),
            ("lvm.interval", self.lvm.interval),
            ("lvm.timeout", self.lvm.timeout),
            ("procscan.interval", self.procscan.interval),
            ("metrics.interval", self.metrics.interval),
            ("alerts.renotify", self.alerts.renotify),
            ("retention.samples.raw", self.retention.samples.raw),
            (
                "retention.samples.five_min",
                self.retention.samples.five_min,
            ),
            ("retention.samples.hourly", self.retention.samples.hourly),
            (
                "retention.samples.hot_minutes",
                self.retention.samples.hot_minutes,
            ),
        ] {
            if duration.is_zero() {
                issue(issues, field, "duration must be positive");
            }
        }
        if self.throttle.profiles.contains_key("max") {
            issue(
                issues,
                "throttle.profiles.max",
                "max is reserved for no throttling; remove this profile definition",
            );
        }
        for (field, profile) in &self.throttle.profiles {
            if !profile.max_util_pct.is_finite() || !(0.0..=100.0).contains(&profile.max_util_pct) {
                issue(
                    issues,
                    format!("throttle.profiles.{field}.max_util_pct"),
                    "expected 0–100 percent",
                );
            }
            if !profile.target_latency_ms.is_finite() || profile.target_latency_ms <= 0.0 {
                issue(
                    issues,
                    format!("throttle.profiles.{field}.target_latency_ms"),
                    "must be positive and finite",
                );
            }
        }
        if !(1..=19).contains(&self.retention.snapshots.cold_level) {
            issue(issues, "retention.snapshots.cold_level", "expected 1–19");
        }
        if !self.retention.min_free_pct.is_finite()
            || !(0.0..=100.0).contains(&self.retention.min_free_pct)
        {
            issue(issues, "retention.min_free_pct", "expected 0–100 percent");
        }
        if self.watch.events.is_empty()
            || self
                .watch
                .events
                .iter()
                .any(|e| !matches!(e.as_str(), "close_write" | "modify"))
        {
            issue(issues, "watch.events", "expected close_write and/or modify");
        }
        let mut names = BTreeSet::new();
        for (i, rule) in self.alerts.rule.iter().enumerate() {
            if rule.name.is_empty() || !names.insert(&rule.name) {
                issue(
                    issues,
                    format!("alerts.rule[{i}].name"),
                    "rule name must be non-empty and unique",
                );
            }
            rule.validate_into(i, issues);
        }
    }
}
impl AlertRule {
    fn validate_into(&self, index: usize, issues: &mut Vec<ConfigIssue>) {
        use AlertKind::*;
        let base = format!("alerts.rule[{index}]");
        let (required, optional): (&[&str], &[&str]) = match self.kind {
            FsUsedPct => (&["mount", "above"], &[]),
            TimeToFull => (&["mount", "below", "window"], &[]),
            HotDirGrowth => (&["mount", "path", "above", "window"], &[]),
            ThinPoolPct => (&["data_above", "meta_above"], &["vg", "pool"]),
            VgFreeLow => (&["below"], &["vg"]),
            DirGrowth => (&["root", "depth", "above"], &[]),
            PhantomBytes => (&["mount", "above"], &[]),
            ScanStale => (&["root", "older_than"], &[]),
            WatchLossy => (&["mount", "above", "window"], &[]),
        };
        let fields = [
            ("mount", self.mount.is_some()),
            ("above", self.above.is_some()),
            ("below", self.below.is_some()),
            ("window", self.window.is_some()),
            ("path", self.path.is_some()),
            ("vg", self.vg.is_some()),
            ("pool", self.pool.is_some()),
            ("data_above", self.data_above.is_some()),
            ("meta_above", self.meta_above.is_some()),
            ("root", self.root.is_some()),
            ("depth", self.depth.is_some()),
            ("older_than", self.older_than.is_some()),
        ];
        for (field, present) in fields {
            if required.contains(&field) && !present {
                issue(
                    issues,
                    format!("{base}.{field}"),
                    "required for this rule kind",
                );
            }
            if present && !required.contains(&field) && !optional.contains(&field) {
                issue(
                    issues,
                    format!("{base}.{field}"),
                    "not used by this rule kind",
                );
            }
        }
        for (field, path) in [("mount", &self.mount), ("root", &self.root)] {
            if let Some(path) = path {
                absolute(issues, format!("{base}.{field}"), path);
            }
        }
        if let Some(window) = &self.window {
            if !positive_duration(window) {
                issue(
                    issues,
                    format!("{base}.window"),
                    "expected positive duration",
                );
            }
        }
        if let Some(age) = &self.older_than {
            if age != "auto" && !positive_duration(age) {
                issue(
                    issues,
                    format!("{base}.older_than"),
                    "expected auto or positive duration",
                );
            }
        }
        if let Some(below) = &self.below {
            let valid = if self.kind == TimeToFull {
                positive_duration(below)
            } else {
                below.parse::<ByteSize>().is_ok()
            };
            if !valid {
                issue(
                    issues,
                    format!("{base}.below"),
                    "expected a duration (time_to_full) or size (vg_free_low)",
                );
            }
        }
        if let Some(above) = &self.above {
            let valid = match (self.kind, above) {
                (FsUsedPct, AlertThreshold::Number(n)) => {
                    n.is_finite() && (0.0..=100.0).contains(n)
                }
                (WatchLossy, AlertThreshold::Number(n)) => {
                    n.is_finite() && *n >= 0.0 && n.fract() == 0.0
                }
                (HotDirGrowth, AlertThreshold::Text(s)) => parse_rate(s).is_ok(),
                (HotDirGrowth, AlertThreshold::Number(n)) => n.is_finite() && *n >= 0.0,
                (DirGrowth | PhantomBytes, AlertThreshold::Text(s)) => {
                    s.parse::<ByteSize>().is_ok()
                }
                (DirGrowth | PhantomBytes, AlertThreshold::Number(n)) => {
                    n.is_finite() && *n >= 0.0 && n.fract() == 0.0
                }
                _ => false,
            };
            if !valid {
                issue(
                    issues,
                    format!("{base}.above"),
                    "threshold type or range does not match rule kind",
                );
            }
        }
        for (field, value) in [
            ("data_above", self.data_above),
            ("meta_above", self.meta_above),
        ] {
            if value.is_some_and(|v| !v.is_finite() || !(0.0..=100.0).contains(&v)) {
                issue(issues, format!("{base}.{field}"), "expected 0–100 percent");
            }
        }
    }
}
/// Shipped enabled alert rules from 07 §6.4, scoped to configured mounts.
/// Only `/data` uses the base name. Other mounts and scheduled roots append `:<path>`.
/// Suffixes retain `/` and ASCII `[A-Za-z0-9._-]`; every other byte (including
/// percent, colon, space, backslash and UTF-8 multibyte bytes) becomes uppercase `%XX`.
/// Roots with schedule `off` have no generated stale rule. Collector-dependent
/// rules remain visible, but consumers must not fire them without available input;
/// thin-pool rules apply only to tracked thin LVs.
pub fn default_rules(cfg: &Config) -> Vec<AlertRule> {
    use AlertKind::*;
    let mut rules = Vec::new();
    for mount in &cfg.sampler.mounts {
        let suffix = subject_suffix(mount);
        for (name, kind, above, below, window, seconds) in [
            (
                "fs_used_pct",
                FsUsedPct,
                Some(AlertThreshold::Number(90.0)),
                None,
                None,
                300,
            ),
            (
                "fs_full_soon",
                TimeToFull,
                None,
                Some("12h"),
                Some("1h"),
                600,
            ),
            (
                "phantom_bytes",
                PhantomBytes,
                Some(AlertThreshold::Text("10GiB".into())),
                None,
                None,
                600,
            ),
        ] {
            rules.push(AlertRule {
                name: format!("{name}{suffix}"),
                kind,
                mount: Some(mount.clone()),
                above,
                below: below.map(str::to_owned),
                window: window.map(str::to_owned),
                for_: Duration::from_secs(seconds),
                ..AlertRule::default()
            });
        }
    }
    for mount in &cfg.watch.mounts {
        let suffix = subject_suffix(mount);
        rules.push(AlertRule {
            name: format!("watch_lossy{suffix}"),
            kind: WatchLossy,
            mount: Some(mount.clone()),
            above: Some(AlertThreshold::Number(10.0)),
            window: Some("15m".into()),
            ..AlertRule::default()
        });
    }
    rules.extend([AlertRule {
        name: "thin_pool_pct".into(),
        kind: ThinPoolPct,
        vg: Some("*".into()),
        pool: Some("*".into()),
        data_above: Some(80.0),
        meta_above: Some(80.0),
        ..AlertRule::default()
    }]);
    for root in &cfg.roots {
        if root.schedule != "off" {
            rules.push(AlertRule {
                name: format!("scan_stale{}", subject_suffix(&root.path)),
                kind: ScanStale,
                root: Some(root.path.clone()),
                older_than: Some("auto".into()),
                ..AlertRule::default()
            });
        }
    }
    rules
}
/// Preserve `/` and ASCII `[A-Za-z0-9._-]`; encode every other path byte as
/// uppercase `%XX`. Only `/data` has no suffix.
fn subject_suffix(path: &Path) -> String {
    use std::os::unix::ffi::OsStrExt;
    if path == Path::new("/data") {
        return String::new();
    }
    let mut suffix = String::from(":");
    for &byte in path.as_os_str().as_bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'/' | b'.' | b'_' | b'-') {
            suffix.push(char::from(byte));
        } else {
            suffix.push_str(&format!("%{byte:02X}"));
        }
    }
    suffix
}
impl Config {
    /// Fill in shipped alerts; user rules with equal names override defaults.
    /// Duplicate user names are preserved so validation can report them.
    pub fn merge_default_rules(&mut self) {
        let names: BTreeSet<_> = self.alerts.rule.iter().map(|r| r.name.as_str()).collect();
        let defaults: Vec<_> = default_rules(self)
            .into_iter()
            .filter(|r| !names.contains(r.name.as_str()))
            .collect();
        self.alerts.rule.extend(defaults);
        for rule in &mut self.alerts.rule {
            if matches!(rule.kind, AlertKind::ThinPoolPct | AlertKind::VgFreeLow)
                && rule.vg.is_none()
            {
                rule.vg = Some("*".into());
            }
            if rule.kind == AlertKind::ThinPoolPct && rule.pool.is_none() {
                rule.pool = Some("*".into());
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    const EXAMPLE: &str = include_str!("../tests/example.toml");
    #[test]
    fn example_verbatim_and_roundtrip() {
        let document = std::fs::read_to_string(
            Path::new(env!("CARGO_MANIFEST_DIR")).join("../../docs/plan/08-interfaces.md"),
        )
        .unwrap();
        let section = document
            .split_once("## 3. Configuration (")
            .unwrap()
            .1
            .split_once("\n## 4.")
            .unwrap()
            .0;
        let example = section
            .split_once("```toml\n")
            .unwrap()
            .1
            .split_once("\n```")
            .unwrap()
            .0;
        assert_eq!(EXAMPLE.as_bytes(), format!("{example}\n").as_bytes());
        let config: Config = toml::from_str(EXAMPLE).unwrap();
        assert!(config.validate().is_empty(), "{:?}", config.validate());
        assert_eq!(config.scan.memory_budget, ByteSize(1280 << 20));
        assert_eq!(config.roots[1].window, None);
        let encoded = toml::to_string(&config).unwrap();
        assert_eq!(toml::from_str::<Config>(&encoded).unwrap(), config);
    }
    #[test]
    fn unknown_key_is_helpful() {
        let e = toml::from_str::<Config>(&EXAMPLE.replace("log_level", "log_levle"))
            .unwrap_err()
            .to_string();
        assert!(
            e.contains("unknown field") && e.contains("log_levle") && e.contains("log_level"),
            "{e}"
        );
        assert!(toml::from_str::<Config>("[general]\nsocket_group = 'tidemark'").is_err());
    }
    #[test]
    fn validation_rejects_paths_windows_and_rules() {
        let mut c: Config = toml::from_str(EXAMPLE).unwrap();
        c.general.state_dir = "relative".into();
        c.roots[0].window = Some("25:00-06:00".into());
        c.alerts.rule[0].above = Some(AlertThreshold::Number(5.0));
        let issues = c.validate();
        assert!(issues.iter().any(|i| i.field == "general.state_dir"));
        assert!(issues.iter().any(|i| i.field == "roots[0].window"));
        assert!(issues.iter().any(|i| i.field == "alerts.rule[0].above"));
        assert!(toml::from_str::<Config>(&EXAMPLE.replace("time_to_full", "made_up")).is_err());
    }
}
