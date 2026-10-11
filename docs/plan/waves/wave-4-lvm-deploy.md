# Wave 4 — LVM awareness and staged deployment

**Goal:** Tidemark understands the LVM layer under `/` and `/data` (sizes, thin-pool usage, VG headroom), and can be shipped by the hotfix pipeline: staged at deploy time, active after the next reboot, and verified automatically.

**Agents:** 2. **Entry:** `wave-2-done`. Can overlap with Waves 3 and 5.

| Task | Title | Owned paths | Depends on |
|------|-------|-------------|------------|
| W4-A | LVM collector, `tidemark lvm`, LVM section of `probe` | `tidemark-fs/src/lvm.rs`, `tidemark-cli/src/cmd/lvm.rs`, the LVM section of `tidemark-fs/src/probe.rs`, `test-support` samples under `fixtures/lvm/` | W1-A (mountinfo, probe) |
| W4-B | Staged deployment: `deploy stage/verify/unstage`, first-boot report, API client | `tidemark-cli/src/cmd/deploy/**`, `tidemark-cli/src/deploy/**`, `tidemark-cli/src/client.rs`, `tidemarkd/src/firstboot.rs` | W1 (probe, mountinfo), W0 config |

**Merge order:** W4-A → W4-B.

Both tasks are **strictly read-only** with respect to the system (ADR-008). LVM is only *read* (the `lvs`/`vgs` reporting commands; **not** `--readonly`, see `07` §1), and `deploy` only touches Tidemark's own config, state and service enablement.

---

## W4-A — LVM collector and `tidemark lvm`

**Read first:** `07-live-monitoring.md` §1, `01-requirements.md` FR-24, `lvs(8)`/`vgs(8)` and `lvmreport(7)` on the VM.

### Interface (`tidemark_fs::lvm`)

```rust
pub struct LvInfo { pub vg: String, pub lv: String, pub attr: String, pub segtype: String, pub size: u64,
                    pub data_pct: Option<f64>, pub meta_pct: Option<f64>, pub pool_lv: Option<String>,
                    pub dm_path: Option<String>, pub major: Option<u32>, pub minor: Option<u32> }
pub struct VgInfo { pub vg: String, pub size: u64, pub free: u64 }
pub struct LvmReport { pub lvs: Vec<LvInfo>, pub vgs: Vec<VgInfo>, pub sampled_at: i64 }
pub struct MountLv { pub mount: RawPath, pub vg: String, pub lv: String, pub thin_pool: Option<String> }

pub fn parse_lvs_json(bytes: &[u8]) -> Result<Vec<LvInfo>, LvmError>;   // pure; fuzzed in W7-C
pub fn parse_vgs_json(bytes: &[u8]) -> Result<Vec<VgInfo>, LvmError>;   // pure; fuzzed in W7-C
pub fn collect(cfg: &LvmConfig, now: i64) -> Result<LvmReport, LvmError>;
pub fn map_mounts(report: &LvmReport, mounts: &[MountInfo]) -> Vec<MountLv>;
pub fn lvm_now(report: &LvmReport, m: &MountLv) -> LvmNow;              // core::live::LvmNow
pub fn to_samples(report: &LvmReport, tracked: &[MountLv]) -> Vec<LvmSample>;
```

### Details

- **Commands**, exactly as in `07` §1 (including `lv_kernel_major`/`lv_kernel_minor`). Run them with:
  - `LC_ALL=C` and `LVM_SUPPRESS_FD_WARNINGS=1`;
  - absolute paths (`/usr/sbin/lvs`, `/usr/sbin/vgs`). Tests override them with the env vars `TIDEMARK_LVS_BIN` / `TIDEMARK_VGS_BIN`, which are honoured only in debug builds or with the `privileged` test feature; the frozen config schema is not changed;
  - a hard timeout (`lvm.timeout`): kill the child on expiry.

  A missing binary gives `LvmError::NotInstalled`, which callers render as "n/a".
- **JSON shape:** `{"report":[{"lv":[{…}]}]}`, where every value is a string. With `--units b --nosuffix`, sizes are integer strings and percentages are decimal strings, or `""` when not applicable. Parse defensively: unknown fields are ignored and empty strings become `None`.
- **Mapping:** match a mount's `major:minor` to an LV's kernel major/minor. For a thin LV (`segtype = "thin"`), record `pool_lv`, so thin data/metadata usage is reported for the **pool** (that's what fills). Snapshots, cache and RAID LV types are listed but not specially interpreted in v1. Inactive LVs report kernel major/minor `"-1"` → `None`; hidden LVs and pools may be bracketed (`[pool0_tdata]`) → strip the brackets for display, but keep the raw name for matching.
- **Probe wiring:** W1-A's `probe` calls `lvm::collect` through the W0 stub. After this task, the probe's LVM section shows each root's LV/VG, thin pool and VG free. W4-A owns that section of `probe.rs`, and the edit is limited to it.
- **CLI `tidemark lvm`** (direct mode; a daemon branch is added by W6-A). For each tracked mount it shows LV, VG, size, and segtype. For thin LVs it also shows the pool's data% and metadata%, with a trend over the last 24 h from `timeseries.db` when present. It then shows each VG's size and free space, labelled "headroom for lvextend + xfs_growfs". Finally it lists other LVs in the VGs, for context. `--json` is supported.
- Never run `lvs` in a hot path; the daemon calls `collect` every `lvm.interval`.

### Tests

- Parser tests on captured JSON from EL8 VMs (commit them under `test-support/fixtures/lvm/`): linear LVs, a thin pool with thin LVs, a snapshot, empty fields, and odd VG names with hyphens (device-mapper escapes them as `--` in `dm_path`, which is why mapping uses major:minor).
- `map_mounts` with synthetic mountinfo, including `/dev/dm-N`, `/dev/mapper/…` and `/dev/<vg>/<lv>` sources.
- Timeout: a fake `lvs` script that sleeps → `LvmError::Timeout` within the deadline; the child is killed (no zombie).
- Privileged (`LoopVg`): `lvm_maps_mounts_to_lvs`, `lvm_thin_pool_usage_tracks_writes`, `lvm_absent_is_na` (`09` §5).

### Acceptance

- [ ] On the VM (root and `/data` on LVM), `tidemark lvm` matches `lvs`/`vgs` output.
- [ ] `collect` takes < 2 s on the VM (recorded).

---

## W4-B — Staged deployment, first-boot report, API client

**Read first:** `08-interfaces.md` §5 (normative), `02-architecture.md` §10, `11-rollout-runbook.md` P3/P5, ADR-016.

### Interfaces

```rust
// tidemark-cli::deploy
pub enum CheckStatus { Pass, Warn, Fail }
pub struct CheckResult { pub name: &'static str, pub status: CheckStatus, pub detail: String }
pub struct StagedRecord { pub version: String, pub staged_at: i64, pub config_sha256: String,
                          pub checks: Vec<CheckResult>, pub start_now: bool }
pub trait Systemctl {   // real impl shells out to /usr/bin/systemctl; tests use a fake
    fn is_enabled(&self, unit: &str) -> io::Result<bool>;
    fn is_active(&self, unit: &str) -> io::Result<bool>;
    fn enable(&self, unit: &str) -> io::Result<()>;
    fn disable_now(&self, unit: &str) -> io::Result<()>;
    fn restart(&self, unit: &str) -> io::Result<()>;
}
pub fn preflight(cfg: &Config, env: &dyn Env) -> Vec<CheckResult>;     // Env abstracts statvfs/mountinfo/probes for tests
pub fn stage(cfg_path: &Path, opts: &StageOptions, sys: &dyn Systemctl, env: &dyn Env) -> Result<StagedRecord, DeployError>;
pub fn verify(state_dir: &Path, wait: Duration, sys: &dyn Systemctl, api: &dyn ApiPing) -> Result<VerifyResult, DeployError>;
pub fn unstage(state_dir: &Path, purge_state: bool, sys: &dyn Systemctl) -> Result<(), DeployError>;

// tidemark-cli::client (shared by deploy verify now and the live commands in W6-A)
pub struct Client;
impl Client {
    pub fn connect(socket: &Path, timeout: Duration) -> Result<Client, ClientError>;   // ClientError::Unavailable → exit 4
    pub fn call<P: Serialize, R: DeserializeOwned>(&mut self, method: &str, params: &P) -> Result<R, ClientError>;
}

// tidemarkd::firstboot (called by W6-A's main once components start)
pub struct ComponentState { pub name: String, pub state: String /* ok|degraded|disabled|error */, pub detail: String }
pub struct FirstbootInputs { pub version: String, pub config_sha256: String, pub daemon_started_at: i64,
                             pub components: Vec<ComponentState>, pub watch_modes: Vec<(String, String)>,
                             pub first_fs_sample: Option<i64>, pub first_lvm_sample: Option<i64>,
                             pub next_scans: Vec<(String, i64)> }
pub struct FirstbootReport { /* inputs + booted_at (btime from /proc/stat) + healthy: bool + written_at */ }
pub fn needs_report(state_dir: &Path, config_sha256: &str) -> bool;    // staged.json exists and no matching report yet
pub fn build(inputs: &FirstbootInputs, now: i64) -> FirstbootReport;
pub fn write(state_dir: &Path, r: &FirstbootReport) -> io::Result<()>; // atomic; also one journald summary line
```

### Behaviour

- **`deploy stage`**: run every check in `08` §5.
  - **If any check is `Fail`:** print the table, exit 5, and change **nothing**. Test this explicitly.
  - **Otherwise:**
    1. Install the config atomically (temp file, then rename) as `0600 root:root`. Back up any differing existing config.
    2. `systemctl enable tidemarkd`.
    3. With `--start-now`, also restart (or start) the service.
    4. Write `deploy/staged.json`.
    5. Print a summary, including warnings such as "watcher will be disabled: fanotify unavailable".
  - **Idempotent:** re-running with the same config is a no-op that reports "already staged".
- **`healthy` (first-boot report)** is true when the core components (`sampler`, `scheduler`, `api`, `tswriter`; canonical names in `08` §5) are `ok`. Non-core components may be `degraded`/`disabled`/`error`; they are listed but never block, so a host without LVM or Postgres verifies.
- **`deploy verify`:**
  1. The service must be active.
  2. `firstboot.json` must exist with `config_sha256` matching `staged.json`, and be `healthy`.
  3. An API `status` ping must succeed.
  4. With `--wait`, poll every 5 s until healthy or timeout.

  Exit 0 if healthy; exit 6 with the failing items listed. With the global `--json`, it emits `{healthy, items:[…], firstboot, staged}`.
- **`deploy unstage`:** `systemctl disable --now tidemarkd` and remove `staged.json`. With `--purge-state --yes`, also delete `state_dir`, but only after the service has stopped, and only if the path is the configured `state_dir` and contains Tidemark's marker file.
- **Edge cases:**
  - **No reboot for weeks:** `deploy stage` without `--start-now` leaves the service enabled but stopped; `deploy verify` reports "staged, waiting for start".
  - **Re-staging with a new config:** `staged.json` gets the new hash, and a new first-boot report is written after the next start.

### Tests

- Unit (fake `Systemctl`, fake `Env`, temp dirs):
  - Every pre-flight check in pass, warn and fail states.
  - **Fail → no side effects:** config untouched, no enable call, no `staged.json`.
  - Config backup and permissions.
  - Idempotent re-stage.
  - The verify state machine: missing report, stale hash, unhealthy component, healthy, and timeout.
  - Unstage with and without purge, including the marker-file guard.
- `firstboot`: `needs_report` logic; `build` health rules; atomic write.
- Client: framing, timeout, and daemon-absent errors mapping to exit 4.
- Privileged `deploy_stage_preflight_blocks` and `deploy_unstage` run in W4. **`deploy_stage_reboot_verify` needs the daemon, so it is written here and executed at the W6 gate.**

### Acceptance

- [ ] All unit tests pass; the privileged pre-flight and unstage tests pass on both VMs.
- [ ] `08` §5 matches the implemented checks and exit codes; any difference goes through an SCR.

---

## Wave 4 gate checklist

- [ ] `tidemark lvm` verified on a VM whose root and `/data` are XFS on LVM, plus a thin-pool loop VG.
- [ ] `deploy stage` refuses cleanly on each injected failure, and stages cleanly otherwise (VM).
- [ ] Tag `wave-4-done`.
