# Handoff — W6-prep: Wave 6 shared workspace preparation

## Needs from coordinator

No requested Rust dependency was missing from the offline host cache. `cargo-generate-rpm` remains an external Cargo tool, not an attached library; it is not installed in the host Cargo bin directory. Container tools, RPM tooling, promtool, Grafana and test VMs were not exercised. Daemon runtime, privileged execution and packaging acceptance remain pending their implementing tasks and VM gates.

- **Attempt:** attempt-c60a7d28769efd8a9528e7a1b156b70101b148ce48cdf4bd79d17e7aefee5a18
- **Branch:** existing attempt branch; no branch creation, merge or push.
- **Date:** 2026-10-11
- **Status:** preparation implemented; evidence candidate pending independent verification.

## What was built

Prepared the daemon modules, shared read/publication API, service entry points, scheduler control/parsers, API dispatch/context, metrics cache/export interface, and daemon startup composition. Existing check-config behavior is preserved in `run_args`; the new `run(config, options)` fails explicitly through preparation stubs. No services, sockets or storage are started by these bodies. CLI render declarations and RPM command dispatch are ready without downstream shared-file edits.

All unspecified shapes are proposed, documented as such in source and below. Implementers should coordinate changes to these seams before callers rely on them; frozen core DTOs and the AlertSource trait remain final. Collections must be capped in implementing collectors and persistence batches, and the writer must create a bounded channel. State returns None for unavailable data rather than fake measured empty collections. Error-returning stubs return NotYetImplemented; infallible constructors return inert markers (SCR-005). No tests asserting stub behavior were added.

## Shared-file edits and dependency justification

`Cargo.toml` enables Chrono `clock` alongside `std` for local-time schedules/DST; the host cache resolves its platform timezone dependencies offline. No new workspace crate declaration was needed. Cargo.lock records sd-notify and Chrono timezone dependencies plus all new attachments. No OpenSSL or async runtime is attached.

Daemon manifest attaches filesystem/scan/store/live/sys libraries, bounded channels, locking, systemd notification, signals, tracing subscriber, tiny_http, typed errors, Chrono, duration parsing, config hashing and command/snapshot test helpers. CLI adds tidemark-sys for safe terminal/system helpers. xtask attaches serde/JSON/TOML for RPM metadata and tempfile/assert_cmd/test-support for packaging tests; its privileged feature now forwards to test-support. CLI/daemon forwarding is retained.

Daemon lib/main owner headers are W6-A1; lib declares all service modules inline so no downstream services/mod.rs edit is needed. `render/mod.rs` declares four live render placeholders. `xtask/src/main.rs` declares and dispatches rpm; W6-C owns rpm.rs. Packaging README documents asset owners and external tools. Existing command files, firstboot, core, other libraries, plan and memory projections were untouched.

Complete per-crate dependency inventories follow; retained attachments are included to remove ambiguity.

### tidemarkd

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `sha2` | staged config hashes |
| Runtime | `humantime` | schedule durations |
| Runtime | `chrono` | local-time scheduling and DST |
| Runtime | `thiserror` | typed daemon errors |
| Runtime | `tiny_http` | optional loopback metrics listener |
| Runtime | `tracing-subscriber` | logging setup and filtering |
| Runtime | `signal-hook` | safe lifecycle signals |
| Runtime | `sd-notify` | READY/STOPPING/STATUS |
| Runtime | `parking_lot` | shared state locks |
| Runtime | `crossbeam-channel` | bounded synchronous queues |
| Runtime | `tidemark-sys` | safe Linux/terminal wrappers |
| Runtime | `tidemark-live` | watchers, procscan, rates and alerts |
| Runtime | `tidemark-store` | catalog/snapshot/time-series access |
| Runtime | `tidemark-scan` | scan orchestration |
| Runtime | `tidemark-fs` | filesystem/LVM collection |
| Runtime | `serde` | serialization |
| Runtime | `serde_json` | wire/report and metadata JSON |
| Runtime | `tracing` | diagnostics |
| Runtime | `tracing-journald` | journal logging |
| Runtime | `rustix` | safe descriptors/permissions/peer credentials |
| Runtime | `tidemark-core` | frozen config and DTOs |
| Runtime | `clap` | frozen daemon options |
| Dev | `insta` | report and exposition snapshots |
| Dev | `assert_cmd` | CLI and packaging command tests |
| Dev | `test-support` | shared fixtures/privileged VM helpers |
| Dev | `tempfile` | isolated fixtures and atomic-write tests |

### tidemark-cli

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `tidemark-sys` | safe Linux/terminal wrappers |
| Runtime | `sha2` | staged config hashes |
| Runtime | `tracing` | diagnostics |
| Runtime | `tempfile` | isolated fixtures and atomic-write tests |
| Runtime | `signal-hook` | safe lifecycle signals |
| Runtime | `rustix` | safe descriptors/permissions/peer credentials |
| Runtime | `crossbeam-channel` | bounded synchronous queues |
| Runtime | `globset` | Retained existing CLI functionality/testing |
| Runtime | `tidemark-live` | watchers, procscan, rates and alerts |
| Runtime | `tidemark-store` | catalog/snapshot/time-series access |
| Runtime | `tidemark-scan` | scan orchestration |
| Runtime | `tidemark-core` | frozen config and DTOs |
| Runtime | `clap` | frozen daemon options |
| Runtime | `clap_complete` | Retained existing CLI functionality/testing |
| Runtime | `anyhow` | binary/test errors |
| Runtime | `thiserror` | typed daemon errors |
| Runtime | `tracing-subscriber` | logging setup and filtering |
| Runtime | `serde` | serialization |
| Runtime | `serde_json` | wire/report and metadata JSON |
| Runtime | `humantime` | schedule durations |
| Runtime | `tidemark-fs` | filesystem/LVM collection |
| Dev | `proptest` | Retained existing CLI functionality/testing |
| Dev | `assert_cmd` | CLI and packaging command tests |
| Dev | `tempfile` | isolated fixtures and atomic-write tests |
| Dev | `test-support` | shared fixtures/privileged VM helpers |
| Dev | `insta` | report and exposition snapshots |

### xtask

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `toml` | RPM metadata/config parsing |
| Runtime | `serde_json` | wire/report and metadata JSON |
| Runtime | `serde` | serialization |
| Runtime | `anyhow` | binary/test errors |
| Runtime | `proc-macro2` | existing CI syntax checks |
| Runtime | `syn` | existing CI source policy checks |
| Dev | `test-support` | shared fixtures/privileged VM helpers |
| Dev | `assert_cmd` | CLI and packaging command tests |
| Dev | `tempfile` | isolated fixtures and atomic-write tests |

## Stub inventory

Every row includes associated methods/variants listed; all bodies are placeholders. “Final” applies only to frozen payload types/trait signatures, not runtime completion.

| Task | Crate path | Stubbed items | Signature status |
|---|---|---|---|
| W6-A1 | `crates/tidemarkd/src/lib.rs` | `DaemonError::NotYetImplemented`, `Shutdown`, `ServiceHandle`, `RunOptions`, `run(config, options)` | proposed |
| W6-A1 | `crates/tidemarkd/src/state.rs` | `LiveState`, `new`, `view`, `publish`, `publish_component`; `Publication::{Fs,Lvm,Watcher,Procscan,Alerts,Scan,DirGrowth,SnapshotMetrics,Deployment,StateSpaceRefused,Degraded}` | proposed; payloads reuse frozen types |
| W6-A1 | `crates/tidemarkd/src/state.rs` | `LiveStateView::{firstboot_inputs,clear_watch_dirty,state_space_refused,alerts_history,snapshot_metrics,status,now,hot,lvm_report,phantom,writers,active_alerts,alert_diagnostics,scans,fs_samples,watch_stats,degraded}` | proposed |
| W6-A1 | `crates/tidemarkd/src/state.rs` | `AlertSource::{fs_sample,fs_rate,hot_dirs,watch_overflows,phantom_bytes,lvm,dir_growth,scan_age}` implementation | final trait signatures; all return None |
| W6-A1 | `crates/tidemarkd/src/services/sampler.rs` | `start` | proposed common service signature |
| W6-A1 | `crates/tidemarkd/src/services/lvm.rs` | `start` | proposed common service signature |
| W6-A1 | `crates/tidemarkd/src/services/tswriter.rs` | `Message::{ScanStarted,ScanFinished,Fs,Lvm,Hot,Phantom,Writers,Alert,DirSeries,Flush}`, `Sender`, `start` | proposed; core records final |
| W6-A2 | `crates/tidemarkd/src/services/watcher.rs` | `start` | proposed common service signature |
| W6-A2 | `crates/tidemarkd/src/services/procscan.rs` | `start` | proposed common service signature |
| W6-A2 | `crates/tidemarkd/src/services/alerts.rs` | `start` | proposed common service signature |
| W6-A2 | `crates/tidemarkd/src/scheduler.rs` | `Schedule`, `Window`, `parse_schedule`, `parse_window`, `next_run`, `ScanControl::{start,cancel,status}`, service `start` | proposed; scan request/result DTOs final |
| W6-A3 | `crates/tidemarkd/src/api.rs` | `Reload`, `Context`, `dispatch`, `start` | proposed; Request/Response DTOs final |
| W6-B | `crates/tidemarkd/src/metrics.rs` | `SnapshotMetrics`, `refresh`, `render`, `start` | proposed |
| W6-A3 | `crates/tidemark-cli/src/render/{now,hot,alerts,status}.rs` | empty declared rendering modules | owner chooses implementation using frozen reports |
| W6-C | `xtask/src/rpm.rs` | `run(root, args)` | proposed; explicit binary error |

## Exact downstream write scopes

No downstream task needs manifest, lockfile or shared module-declaration edits. Test entry files are Cargo auto-discovered; sibling directories hold helpers and fixtures without manifest changes. Source-local unit tests are also allowed. W6-A2 wires firstboot from scheduler.rs using the existing firstboot public API; firstboot.rs stays W4-B's file. Shared CLI direct implementations must stay intact; only daemon-first branches belong to W6-A3, after current scan work is integrated.

| Task | Exact owned implementation and test paths |
|---|---|
| W6-A1 | `crates/tidemarkd/src/lib.rs`, `crates/tidemarkd/src/main.rs`, `crates/tidemarkd/src/state.rs`, `crates/tidemarkd/src/services/sampler.rs`, `crates/tidemarkd/src/services/lvm.rs`, `crates/tidemarkd/src/services/tswriter.rs`; `crates/tidemarkd/tests/w6a1.rs`, `crates/tidemarkd/tests/w6a1/**` |
| W6-A2 | `crates/tidemarkd/src/scheduler.rs`, `crates/tidemarkd/src/services/watcher.rs`, `crates/tidemarkd/src/services/procscan.rs`, `crates/tidemarkd/src/services/alerts.rs`; `crates/tidemarkd/tests/w6a2.rs`, `crates/tidemarkd/tests/w6a2/**` |
| W6-A3 | `crates/tidemarkd/src/api.rs`; `crates/tidemark-cli/src/cmd/now.rs`, `crates/tidemark-cli/src/cmd/hot.rs`, `crates/tidemark-cli/src/cmd/alerts.rs`, `crates/tidemark-cli/src/cmd/status.rs`; daemon-first branches only in `crates/tidemark-cli/src/cmd/scan.rs`, `crates/tidemark-cli/src/cmd/lvm.rs`, `crates/tidemark-cli/src/cmd/phantom.rs`, `crates/tidemark-cli/src/cmd/writers.rs`; `crates/tidemark-cli/src/render/now.rs`, `crates/tidemark-cli/src/render/hot.rs`, `crates/tidemark-cli/src/render/alerts.rs`, `crates/tidemark-cli/src/render/status.rs`; `crates/tidemarkd/tests/w6a3.rs`, `crates/tidemarkd/tests/w6a3/**`, `crates/tidemark-cli/tests/w6a3.rs`, `crates/tidemark-cli/tests/w6a3/**` |
| W6-B | `crates/tidemarkd/src/metrics.rs`, `packaging/grafana/**`, `packaging/prometheus/**`; `crates/tidemarkd/tests/w6b.rs`, `crates/tidemarkd/tests/w6b/**` |
| W6-C | `xtask/src/rpm.rs`, `packaging/systemd/**`, `packaging/tmpfiles.d/**`, `packaging/config/**`, `packaging/rpm/**`, `tests/install/**`; `xtask/tests/w6c.rs`, `xtask/tests/w6c/**` |

## Startup and cross-task notes

Run has all eight normative startup comments and composes service/scheduler/metrics/API calls. Steps 2–5 and READY remain lifecycle-owner work. The tswriter stub fails before any runtime work; later startup calls are compiled, never executed successfully by preparation. Lifecycle shutdown must stop API, cancel scans, stop watchers, flush/join writer within 10 seconds. Service handles are join handles; owners may add private supervisor structures within their own files.

A1 publishes the SHA-256 of the actually loaded config bytes and startup time through Deployment; A2 reads firstboot_inputs to call the existing firstboot build/write API. Reload is supplied by A1 as a callback to API; current stub returns NotYetImplemented. A1/A2 must coordinate live config propagation and restart affected services in their implementation. SnapshotMetrics refresh accepts a read-only Snapshot and depth; A2 publishes refreshed cache through Publication, B renders it. ScanStarted/ScanFinished messages carry one-shot bounded acknowledgment senders so A2 can implement the existing ScanRecorder without bypassing the single writer. A2 clears watcher dirtiness only after successful scan publication. A1 supplies complete bounded inventories/exact rule windows and at least 24h effective cadence for windowed scan ages. A3 maps daemon errors to frozen wire codes and applies peer uid policy to every request. No uid gate, socket group, PostgreSQL service or group packaging was added.

## Validation

Pinned Rust 1.98.0, host cache, `CARGO_NET_OFFLINE=true`. Rustup wrappers require `RUSTUP_HOME=/home/brewerm/.rustup`, `RUSTUP_TOOLCHAIN=1.98.0` in this worker; initial wrapper failures were tooling setup and rerun with those settings. Evidence is in this attempt's output `library/`. Required checks and their final exit codes are recorded below after completion.

Privileged builds are compile-only; no privileged runtime commands, mount/LVM changes, systemd operations or services were run. Prep adds no privileged tests for stub behavior; daemon/install tests belong to the listed task-specific directories, pending VM. Performance, RSS and all Wave 6 runtime acceptance are pending implementers/VM gates. No independent task-success assertion is made here.

## Final local check evidence

| Command | Exit | Log |
|---|---|---|
| `cargo fmt --check` | 0 | `library/fmt.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | 0 | `library/clippy.log` |
| `cargo test --workspace` | 0 | `library/test.log` |
| `cargo build --workspace --features privileged --tests` | 0 | `library/privileged.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | 0 | `library/doc-final.log` |
| `tools/ci.sh` | 0 | `library/ci-final.log` |

CI counted 139 owner tags and passed the forbidden-symbol scan. cargo-deny was unavailable and skipped; advisory/license auditing remains pending an equipped host. Workspace tests recorded 472 passed, zero failed and 12 existing ignored tests. Privileged validation was compilation only, pending VM execution. Whitespace/scope checks passed locally. Initial wrapper-only CI failure remains in library/ci.log; library/ci-final.log records the corrected successful run.

After the last firstboot-only interface addition, fmt/workspace Clippy/rustdoc were rerun successfully. Tests and privileged-feature compilation for `-p tidemarkd -p tidemark-cli -p xtask` also exited 0 (`library/changed-test.log`, `library/changed-privileged.log`). `library/scope.log` confirms all 26 changed paths are owned.
