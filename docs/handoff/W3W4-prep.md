# Handoff — W3W4-prep: Waves 3 and 4 shared-file preparation

## Needs from coordinator

- No dependency attachment was unavailable offline.
- `08` §5 still lists “Running as root” as a blocking deploy check. ADR-018 forbids a uid gate and takes precedence; W4-B must test access by ownership/permissions instead. No uid check is introduced here.
- Wave files name no explicit render paths. Preparation reserves `diff`, `links`, `symlinks`, `export`, `wide`, `lvm`, and `deploy` render modules for their corresponding command owners.
- `FirstbootReport` specifies “inputs + booted_at + healthy + written_at” in prose. Preparation expands the input fields directly, with `booted_at` and `written_at` as `i64` Unix seconds and `healthy: bool`; no nested inputs field. Explicit FirstbootInputs fields and function signatures are unchanged.

Date: 2026-10-11. Attempt: attempt-2422625af614b9f284292a95351f22445f46f22657fa91ecb850042b3995a0e1. Existing attempt branch retained. Status: evidence candidate awaiting independent verification and integration.

## What was built

Prepared Wave 3 query/export/symlink interfaces and Wave 4 deploy, daemon client and firstboot interfaces. Audited the existing W4-A LVM definitions: all signatures and field lists already match, so lvm.rs needs no edit. All error-returning stubs fail explicitly with NotYetImplemented (io results use Unsupported with that message). Error-free stubs return inert values under accepted SCR-005. Regions retains a private RawPath so name can return a safe reference without a panic; build always fails. No runtime I/O, services, commands or threads are added.

## Workspace and shared-file edits

- Workspace Cargo.toml adds `sha2 = "0.10"`: cached pure Rust SHA-256 for W4-B's staged config hashes. Cargo.lock resolves sha2 0.10.9 and cached transitive dependencies offline. No OpenSSL or async runtime occurs in the lockfile.
- Store Cargo.toml attaches rustc-hash/smallvec (bounded region/link aggregation) and dev insta (report snapshots).
- CLI Cargo.toml attaches sha2 (staged config hash), tracing (deployment/client diagnostics), runtime tempfile (atomic config/state staging), and dev proptest (framing/generated cases). Its existing dev tempfile attachment is retained.
- Daemon Cargo.toml normalizes core to workspace inheritance; attaches serde/serde_json (firstboot report), tracing/tracing-journald (firstboot summary), rustix (safe metadata/permissions), dev test-support (shared fixtures); privileged forwards to test-support/privileged.
- Store lib.rs re-exports Snapshot and export_ncdu/NcduOptions/ExportStats for the shorter wave interface paths.
- CLI lib.rs declares client/deploy. render/mod.rs declares seven disjoint render placeholders.
- FS and test-support manifests already provide their needed dependencies; unchanged. FS and daemon roots already declare symlinks/lvm/firstboot, so no edits required.

## Dependency inventory

The following complete per-crate lists include retained attachments; every entry uses workspace inheritance. test-support is dev-only in product crates. No additional crate-graph edges were added beyond allowed daemon core and dev fixture edges.

### tidemark-store

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `rustc-hash` | Bounded link/region maps |
| Runtime | `smallvec` | Small region sets without routine allocation |
| Runtime | `tidemark-core` | Frozen config, byte paths and shared DTOs |
| Runtime | `thiserror` | Typed errors |
| Runtime | `rusqlite` | Read-only SQLite snapshot queries |
| Runtime | `serde` | Report serialization derives |
| Runtime | `serde_json` | JSON reports, ncdu and LVM parsing |
| Runtime | `rustix` | Safe descriptors, metadata and permissions |
| Runtime | `chrono` | Retention calendar arithmetic |
| Runtime | `tracing` | Diagnostics |
| Runtime | `globset` | Regions/exclusions |
| Dev | `insta` | Text/JSON report snapshots |
| Dev | `serde_json` | JSON reports, ncdu and LVM parsing |
| Dev | `test-support` | Shared fixture and privileged VM helpers |
| Dev | `tempfile` | Temporary fixtures or atomic staging |
| Dev | `proptest` | Generated parser/region/framing cases |

### tidemark-fs

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `tidemark-core` | Frozen config, byte paths and shared DTOs |
| Runtime | `thiserror` | Typed errors |
| Runtime | `globset` | Regions/exclusions |
| Runtime | `crossbeam-channel` | Bounded metadata channels |
| Runtime | `tidemark-sys` | Safe kernel metadata wrappers |
| Runtime | `libc` | Existing metadata constants |
| Runtime | `rustix` | Safe descriptors, metadata and permissions |
| Runtime | `crossbeam-deque` | Walker work queues |
| Runtime | `parking_lot` | Walker/gate synchronization |
| Runtime | `smallvec` | Small region sets without routine allocation |
| Runtime | `rustc-hash` | Bounded link/region maps |
| Runtime | `serde` | Report serialization derives |
| Runtime | `serde_json` | JSON reports, ncdu and LVM parsing |
| Runtime | `tracing` | Diagnostics |
| Dev | `test-support` | Shared fixture and privileged VM helpers |
| Dev | `tempfile` | Temporary fixtures or atomic staging |
| Dev | `proptest` | Generated parser/region/framing cases |
| Dev | `insta` | Text/JSON report snapshots |

### tidemark-cli

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `sha2` | Staged config SHA-256 |
| Runtime | `tracing` | Diagnostics |
| Runtime | `tempfile` | Temporary fixtures or atomic staging |
| Runtime | `signal-hook` | Scan cancellation |
| Runtime | `rustix` | Safe descriptors, metadata and permissions |
| Runtime | `crossbeam-channel` | Bounded metadata channels |
| Runtime | `globset` | Regions/exclusions |
| Runtime | `tidemark-live` | Live report composition |
| Runtime | `tidemark-store` | Snapshot/catalog/time-series queries |
| Runtime | `tidemark-scan` | Scan orchestration |
| Runtime | `tidemark-core` | Frozen config, byte paths and shared DTOs |
| Runtime | `clap` | Frozen CLI argument parsing |
| Runtime | `clap_complete` | Shell completions |
| Runtime | `anyhow` | Existing command error context |
| Runtime | `thiserror` | Typed errors |
| Runtime | `tracing-subscriber` | CLI diagnostic setup |
| Runtime | `serde` | Report serialization derives |
| Runtime | `serde_json` | JSON reports, ncdu and LVM parsing |
| Runtime | `humantime` | Duration parsing/display |
| Runtime | `tidemark-fs` | Mount, probe and gated filesystem operations |
| Dev | `proptest` | Generated parser/region/framing cases |
| Dev | `assert_cmd` | CLI executable tests |
| Dev | `tempfile` | Temporary fixtures or atomic staging |
| Dev | `test-support` | Shared fixture and privileged VM helpers |
| Dev | `insta` | Text/JSON report snapshots |

### tidemarkd

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `serde` | Report serialization derives |
| Runtime | `serde_json` | JSON reports, ncdu and LVM parsing |
| Runtime | `tracing` | Diagnostics |
| Runtime | `tracing-journald` | Firstboot summary |
| Runtime | `rustix` | Safe descriptors, metadata and permissions |
| Runtime | `tidemark-core` | Frozen config, byte paths and shared DTOs |
| Runtime | `clap` | Frozen CLI argument parsing |
| Dev | `test-support` | Shared fixture and privileged VM helpers |
| Dev | `tempfile` | Temporary fixtures or atomic staging |

### test-support

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `tidemark-core` | Frozen config, byte paths and shared DTOs |
| Runtime | `tempfile` | Temporary fixtures or atomic staging |
| Runtime | `serde` | Report serialization derives |
| Runtime | `toml` | Fixture configuration |
| Runtime | `rustix` | Safe descriptors, metadata and permissions |

## Stub inventory

Explicit fields, enum variants, trait methods and signatures are final unless marked below. Bodies remain placeholders. Existing LVM items are listed as audited, unchanged. Every function listed in the waves was compared to source; evidence is library/contract-audit.txt (44 records).

| Item | Wave/task | Source path | Shape |
|---|---|---|---|
| `DiffMetric` | wave-3 / W3-A | `crates/tidemark-store/src/diff.rs` | final |
| `DiffClass` | wave-3 / W3-A | `crates/tidemark-store/src/diff.rs` | final |
| `DiffRow` | wave-3 / W3-A | `crates/tidemark-store/src/diff.rs` | final |
| `ExplainStep` | wave-3 / W3-A | `crates/tidemark-store/src/diff.rs` | final |
| `Explanation` | wave-3 / W3-A | `crates/tidemark-store/src/diff.rs` | final |
| `diff` | wave-3 / W3-A | `crates/tidemark-store/src/diff.rs` | final |
| `explain` | wave-3 / W3-A | `crates/tidemark-store/src/diff.rs` | final |
| `RegionSpec` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | final |
| `CrossingSet` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | final |
| `LinkReport` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | final |
| `Regions::build` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | final |
| `Regions::region_of` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | final |
| `Regions::name` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | final |
| `crossing_sets` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | final |
| `link_report` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | final |
| `Regions` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | shape left to the owning task |
| `RegionId` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | shape left to the owning task |
| `CrossingTotals` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | shape left to the owning task |
| `LinkTotals` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | shape left to the owning task |
| `RegionPair` | wave-3 / W3-B | `crates/tidemark-store/src/links.rs` | shape left to the owning task |
| `SymlinkOpts` | wave-3 / W3-B | `crates/tidemark-fs/src/symlinks.rs` | final |
| `SymlinkClass` | wave-3 / W3-B | `crates/tidemark-fs/src/symlinks.rs` | final |
| `SymlinkReport` | wave-3 / W3-B | `crates/tidemark-fs/src/symlinks.rs` | final |
| `analyze` | wave-3 / W3-B | `crates/tidemark-fs/src/symlinks.rs` | final |
| `NcduOptions` | wave-3 / W3-C | `crates/tidemark-store/src/export_ncdu.rs` | final |
| `export_ncdu` | wave-3 / W3-C | `crates/tidemark-store/src/export_ncdu.rs` | final |
| `ExportStats` | wave-3 / W3-C | `crates/tidemark-store/src/export_ncdu.rs` | shape left to the owning task |
| `LvInfo` | wave-4 / W4-A | `crates/tidemark-fs/src/lvm.rs` | final |
| `VgInfo` | wave-4 / W4-A | `crates/tidemark-fs/src/lvm.rs` | final |
| `LvmReport` | wave-4 / W4-A | `crates/tidemark-fs/src/lvm.rs` | final |
| `MountLv` | wave-4 / W4-A | `crates/tidemark-fs/src/lvm.rs` | final |
| `parse_lvs_json` | wave-4 / W4-A | `crates/tidemark-fs/src/lvm.rs` | final |
| `parse_vgs_json` | wave-4 / W4-A | `crates/tidemark-fs/src/lvm.rs` | final |
| `collect` | wave-4 / W4-A | `crates/tidemark-fs/src/lvm.rs` | final |
| `map_mounts` | wave-4 / W4-A | `crates/tidemark-fs/src/lvm.rs` | final |
| `lvm_now` | wave-4 / W4-A | `crates/tidemark-fs/src/lvm.rs` | final |
| `to_samples` | wave-4 / W4-A | `crates/tidemark-fs/src/lvm.rs` | final |
| `CheckStatus` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `CheckResult` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `StagedRecord` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `Systemctl` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `Systemctl::is_enabled` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `Systemctl::is_active` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `Systemctl::enable` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `Systemctl::disable_now` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `Systemctl::restart` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `preflight` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `stage` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `verify` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `unstage` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | final |
| `Env` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | shape left to the owning task |
| `ApiPing` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | shape left to the owning task |
| `StageOptions` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | shape left to the owning task |
| `VerifyResult` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | shape left to the owning task |
| `DeployError` | wave-4 / W4-B | `crates/tidemark-cli/src/deploy/mod.rs` | shape left to the owning task |
| `Client::connect` | wave-4 / W4-B | `crates/tidemark-cli/src/client.rs` | final |
| `Client::call` | wave-4 / W4-B | `crates/tidemark-cli/src/client.rs` | final |
| `Client` | wave-4 / W4-B | `crates/tidemark-cli/src/client.rs` | shape left to the owning task |
| `ClientError` | wave-4 / W4-B | `crates/tidemark-cli/src/client.rs` | shape left to the owning task |
| `ComponentState` | wave-4 / W4-B | `crates/tidemarkd/src/firstboot.rs` | final |
| `FirstbootInputs` | wave-4 / W4-B | `crates/tidemarkd/src/firstboot.rs` | final |
| `FirstbootReport` | wave-4 / W4-B | `crates/tidemarkd/src/firstboot.rs` | final |
| `needs_report` | wave-4 / W4-B | `crates/tidemarkd/src/firstboot.rs` | final |
| `build` | wave-4 / W4-B | `crates/tidemarkd/src/firstboot.rs` | final |
| `write` | wave-4 / W4-B | `crates/tidemarkd/src/firstboot.rs` | final |

Name-only errors contain only preparation's NotYetImplemented (ClientError also carries the explicitly named Unavailable); owners extend their error shapes locally. Env/ApiPing are empty placeholder traits, RegionId a unit placeholder, and other name-only records are unit structs. None represents usable runtime data. No tests asserting these placeholders were added outside their owners' files.

## Exact downstream write scopes

No downstream task needs to edit Cargo.toml, Cargo.lock, crate roots or shared render/mod.rs. Task-exclusive deploy/mod.rs belongs to W4-B. Unit tests can live in owned source files. Integration entry files below are automatically discovered by Cargo; their matching directories hold supporting modules without manifest edits.

| Task | Exact implementation files and exclusive test/fixture directories |
|---|---|
| W3-A | `crates/tidemark-store/src/diff.rs`; `crates/tidemark-cli/src/cmd/diff.rs`; `crates/tidemark-cli/src/render/diff.rs`; new `crates/tidemark-store/tests/w3a.rs`, `crates/tidemark-store/tests/w3a/**`, `crates/tidemark-cli/tests/w3a.rs`, `crates/tidemark-cli/tests/w3a/**` |
| W3-B | `crates/tidemark-store/src/links.rs`; `crates/tidemark-fs/src/symlinks.rs`; `crates/tidemark-cli/src/cmd/links.rs`, `crates/tidemark-cli/src/cmd/symlinks.rs`; `crates/tidemark-cli/src/render/links.rs`, `crates/tidemark-cli/src/render/symlinks.rs`; new `crates/tidemark-store/tests/w3b.rs`, `crates/tidemark-store/tests/w3b/**`, `crates/tidemark-fs/tests/w3b.rs`, `crates/tidemark-fs/tests/w3b/**`, `crates/tidemark-cli/tests/w3b.rs`, `crates/tidemark-cli/tests/w3b/**` |
| W3-C | `crates/tidemark-store/src/export_ncdu.rs`; `crates/tidemark-cli/src/cmd/export.rs`, `crates/tidemark-cli/src/cmd/wide.rs`; `crates/tidemark-cli/src/render/export.rs`, `crates/tidemark-cli/src/render/wide.rs`; new `crates/tidemark-store/tests/w3c.rs`, `crates/tidemark-store/tests/w3c/**`, `crates/tidemark-cli/tests/w3c.rs`, `crates/tidemark-cli/tests/w3c/**` |
| W4-A | `crates/tidemark-fs/src/lvm.rs`; only the LVM section of `crates/tidemark-fs/src/probe.rs`; `crates/tidemark-cli/src/cmd/lvm.rs`; `crates/tidemark-cli/src/render/lvm.rs`; `crates/test-support/fixtures/lvm/**`; new `crates/tidemark-fs/tests/w4a.rs`, `crates/tidemark-fs/tests/w4a/**`, `crates/tidemark-cli/tests/w4a.rs`, `crates/tidemark-cli/tests/w4a/**` |
| W4-B | `crates/tidemark-cli/src/cmd/deploy/**`; `crates/tidemark-cli/src/deploy/**` including mod.rs and any new descendants; `crates/tidemark-cli/src/client.rs`; `crates/tidemark-cli/src/render/deploy.rs`; `crates/tidemarkd/src/firstboot.rs`; new `crates/tidemark-cli/tests/w4b.rs`, `crates/tidemark-cli/tests/w4b/**`, `crates/tidemarkd/tests/w4b.rs`, `crates/tidemarkd/tests/w4b/**` |

## Validation and limitations

Validation commands use pinned Rust 1.98.0, a private copy of the host cargo cache at /tmp/tidemark-w3w4-cargo, and CARGO_NET_OFFLINE=true. Logs and exit files are in the canonical attempt output library. Required local checks are recorded below after completion. No privileged runtime operations were performed. VM behavior, report accuracy, performance and deployment acceptance remain the implementing tasks' responsibility, pending VM. This prep adds no runtime tests for unimplemented operations. Existing LVM stub tests remain owner-local and can be replaced by W4-A.

No core contracts, command files, plan files, memory projections, scan/live/sys code or external system metadata were changed. No branches, merges or pushes occurred. Source items are documented with final-owner headers. Submission and this handoff are evidence candidates, not independent verification or release approval.

## Final local check results

| Check | Result | Evidence |
|---|---|---|
| `cargo fmt --check` | exit 0 | library/fmt.log and .exit |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0 | library/clippy.log and .exit |
| `cargo test --workspace` | exit 0 | library/test.log and .exit |
| `cargo build --workspace --features privileged --tests` | exit 0 | library/privileged.log and .exit |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | exit 0 | library/doc.log and .exit |
| `tools/ci.sh` | exit 0 | library/ci.log and .exit |

Workspace tests: 285 passed, zero failed, five existing scale tests ignored. Privileged-feature build is compilation only, pending VM execution. CI found 129 owner tags and passed the forbidden-symbol scan; cargo-deny was unavailable and skipped, so advisory/license auditing remains pending an equipped host. Contract audit compared 24 public function signatures and 19 explicit field/variant lists; five Systemctl trait methods and core re-exports were also checked. Whitespace and scope checks passed locally.
