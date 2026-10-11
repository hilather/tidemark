# Handoff — W2W5-prep: Waves 2 and 5 shared-file preparation

## Needs from coordinator

- No dependency was unavailable offline. No interface disagreement with the current decisions file was found.
- The W2-A block fixes `SinkError` to `Store(StoreError)` and `Other(String)`, while the preparation error rule asks for `NotYetImplemented`. `VecSink` returns `SinkError::Store(StoreError::NotYetImplemented)` so both the frozen enum shape and explicit unavailable error are retained.
- W5-C names `PgError` without specifying additional variants. Its existing `NotYetImplemented` variant is retained; the definition moves to the W5-C-owned collector file and is re-exported at the existing root path. Additional error variants are shape left to the owning task. No speculative public variants were added.
- `EventIter` is named but its fields/item shape are unspecified. The placeholder uses a private borrowed buffer and `Iterator<Item = io::Result<RawEvent>>`; final storage and parser shape are left to W5-A.

- **Branch:** existing hp-6c1a3a19efad2927476fa005a7f69508fcbaaf10e41b6f9c49749c9b4013589d
- **Attempt:** attempt-6c1a3a19efad2927476fa005a7f69508fcbaaf10e41b6f9c49749c9b4013589d
- **Date:** 2026-10-11
- **Status:** preparation implemented as an evidence candidate; independent verification and integration remain coordinator work.

## What was built

Attached the inventory's mandatory and recommended dependencies, resolved Cargo.lock offline, and prepared the whole Wave 2 and Wave 5 interface blocks. All explicit field lists and signatures use frozen core contracts; no duplicate record or default-rule types were introduced. Error-returning bodies fail explicitly; plain-returning bodies are inert under accepted SCR-005. Shared roots and module declarations are ready for seven disjoint implementation tasks. No scan, watcher, process collector, Postgres collector, alert evaluation or delivery runtime behavior is implemented here.

## Workspace additions

| Addition | Justification |
|---|---|
| rustix `event` feature | Safe poll/eventfd support for watcher shutdown; retains fs/process/system features |
| signal-hook = "0.3" | Safe SIGINT handling for CLI scan cancellation; attached to CLI |
| insta json | Already present at baseline; retained for JSON snapshots, no new addition |

All dependencies use `workspace = true`. The sync postgres client's own internal Tokio dependency is transitive; no Tidemark crate attaches an async runtime or uses one directly. ureq retains the workspace rustls configuration. No OpenSSL dependency appears in the resolved lockfile.

## Dependency attachments

Tables include retained dependencies so each crate's complete runtime/dev attachment list is explicit.

### tidemark-scan

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `rustix` | Safe fd, lock, metadata, poll and eventfd wrappers |
| Runtime | `tracing` | Diagnostic events |
| Runtime | `serde_json` | Snapshot/report JSON and webhook bodies |
| Runtime | `serde` | Report and webhook DTO serialization |
| Runtime | `globset` | Scan exclusions and alert globs |
| Runtime | `smallvec` | Avoid hot-loop allocation |
| Runtime | `rayon` | Bulk inode reference sorting only |
| Runtime | `crossbeam-channel` | Bounded producer/consumer queues |
| Runtime | `rustc-hash` | Bounded hot metadata maps |
| Runtime | `thiserror` | Typed library errors |
| Runtime | `tidemark-sys` | XFS metadata and fanotify wrappers |
| Runtime | `tidemark-fs` | Walker, attribution and filesystem discovery composition |
| Runtime | `tidemark-store` | Row sink errors and snapshot/catalog composition |
| Runtime | `tidemark-core` | Frozen config, path, scan, row and live contracts |
| Dev | `tempfile` | Isolated test fixtures and pgtmp state |
| Dev | `proptest` | Generated aggregation and join cases |
| Dev | `test-support` | Shared fixtures and privileged VM helpers, dev-only |

### tidemark-live

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `serde_json` | Snapshot/report JSON and webhook bodies |
| Runtime | `serde` | Report and webhook DTO serialization |
| Runtime | `ureq` | Bounded synchronous webhook requests with rustls |
| Runtime | `tracing-journald` | Structured alert journal delivery |
| Runtime | `globset` | Scan exclusions and alert globs |
| Runtime | `rustix` | Safe fd, lock, metadata, poll and eventfd wrappers |
| Runtime | `tracing` | Diagnostic events |
| Runtime | `rustc-hash` | Bounded hot metadata maps |
| Runtime | `parking_lot` | Watcher/cache synchronization |
| Runtime | `crossbeam-channel` | Bounded producer/consumer queues |
| Runtime | `thiserror` | Typed library errors |
| Runtime | `tidemark-sys` | XFS metadata and fanotify wrappers |
| Runtime | `tidemark-fs` | Walker, attribution and filesystem discovery composition |
| Runtime | `tidemark-core` | Frozen config, path, scan, row and live contracts |
| Dev | `insta` | CLI and now report snapshots |
| Dev | `tiny_http` | Local webhook test server |
| Dev | `tempfile` | Isolated test fixtures and pgtmp state |
| Dev | `test-support` | Shared fixtures and privileged VM helpers, dev-only |

### tidemark-pg

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `serde` | Report and webhook DTO serialization |
| Runtime | `tracing` | Diagnostic events |
| Runtime | `postgres` | Synchronous Postgres connections |
| Runtime | `tidemark-core` | Frozen config, path, scan, row and live contracts |
| Runtime | `thiserror` | Typed library errors |
| Dev | `tempfile` | Isolated test fixtures and pgtmp state |
| Dev | `test-support` | Shared fixtures and privileged VM helpers, dev-only |

### tidemark-cli

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `signal-hook` | Safe CLI SIGINT cancellation |
| Runtime | `rustix` | Safe fd, lock, metadata, poll and eventfd wrappers |
| Runtime | `crossbeam-channel` | Bounded producer/consumer queues |
| Runtime | `globset` | Scan exclusions and alert globs |
| Runtime | `tidemark-live` | CLI watcher and process collector composition |
| Runtime | `tidemark-store` | Row sink errors and snapshot/catalog composition |
| Runtime | `tidemark-scan` | CLI scan composition |
| Runtime | `tidemark-core` | Frozen config, path, scan, row and live contracts |
| Runtime | `clap` | CLI argument parser |
| Runtime | `clap_complete` | CLI completion support |
| Runtime | `anyhow` | Binary error context |
| Runtime | `thiserror` | Typed library errors |
| Runtime | `tracing-subscriber` | CLI diagnostic initialization |
| Runtime | `serde` | Report and webhook DTO serialization |
| Runtime | `serde_json` | Snapshot/report JSON and webhook bodies |
| Runtime | `humantime` | CLI duration rendering |
| Runtime | `tidemark-fs` | Walker, attribution and filesystem discovery composition |
| Runtime | `tidemark-pg` | CLI Postgres probe/collector composition |
| Dev | `assert_cmd` | CLI executable tests |
| Dev | `tempfile` | Isolated test fixtures and pgtmp state |
| Dev | `test-support` | Shared fixtures and privileged VM helpers, dev-only |
| Dev | `insta` | CLI and now report snapshots |

### tidemark-sys

| Kind | Dependency | Purpose |
|---|---|---|
| Runtime | `rustix` | Safe fd, lock, metadata, poll and eventfd wrappers |
| Runtime | `libc` | Kernel syscall/FFI constants |
| Dev | `test-support` | Shared fixtures and privileged VM helpers, dev-only |
| Dev | `tempfile` | Isolated test fixtures and pgtmp state |

## Shared-file edits

| Path | Edit and purpose |
|---|---|
| Cargo.toml | rustix/event and signal-hook workspace declarations |
| Cargo.lock | Offline resolution for attached dependencies |
| scan/live/pg/cli/sys Cargo.toml | Runtime/dev attachments; scan/live/pg privileged feature forwards to dev-only test-support, like existing cli/sys |
| scan/src/lib.rs | Re-export mode, request/outcome/recorder/error/run_scan, progress and SinkError |
| scan/src/agg/mod.rs and bulk/mod.rs | Retain existing child module declarations, provide final interfaces |
| live/src/watch/mod.rs and alerts/mod.rs | Retain child declarations; alerts re-exports delivery types and core default_rules |
| pg/src/lib.rs | Re-export PgCollector, PgRow, PgConn, PgConnector and relocated PgError alongside existing probe exports |
| cli/src/render/mod.rs | Declare tree and scan_progress |
| cli/src/render/tree.rs and scan_progress.rs | W2-C placeholders, no public functions specified |

All five touched crates have `privileged = ["test-support/privileged"]`; test-support remains dev-only. No CLI cmd root, fs/store/core/daemon/xtask source or plan was changed.

## Stub inventory

Each explicit struct field list, enum variant list, trait method and function signature is final unless marked below. Bodies remain placeholders. Method rows also cover the associated type's cross-task implementation contract. Re-used core records (`DirRecord`, `GoneRecord`, `NsRecord`, `InodeAttrs`, all seven row types and live DTOs) remain unchanged in core.

| Item | Wave/task | Crate path | Shape |
|---|---|---|---|
| `AggConfig` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final |
| `Aggregator` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | shape left to the owning task |
| `Aggregator::new` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Aggregator::add_dir_record` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Aggregator::add_gone` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Aggregator::add_skeleton` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Aggregator::set_dir_inode` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Aggregator::add_single` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Aggregator::add_link_set` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Aggregator::add_races` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Aggregator::prepare` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Prepared` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | shape left to the owning task |
| `Prepared::summary` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Prepared::needs_names` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Prepared::set_names` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Prepared::annotate` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `Prepared::write` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `RowSink` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final |
| `dirs` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `files` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `link_sets` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `link_instances` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `skipped` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `errors` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `annotations` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `VecSink` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final |
| `AggSummary` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final |
| `AggError` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | shape left to the owning task |
| `SinkError` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final |
| `RowSink::dirs` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `RowSink::files` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `RowSink::link_sets` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `RowSink::link_instances` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `RowSink::skipped` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `RowSink::errors` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `RowSink::annotations` | wave-2 / W2-A | `crates/tidemark-scan/src/agg/mod.rs` | final signature |
| `BulkParams` | wave-2 / W2-B | `crates/tidemark-scan/src/bulk/mod.rs` | final |
| `BulkStats` | wave-2 / W2-B | `crates/tidemark-scan/src/bulk/mod.rs` | final |
| `estimate_memory` | wave-2 / W2-B | `crates/tidemark-scan/src/bulk/mod.rs` | final signature |
| `run_bulk` | wave-2 / W2-B | `crates/tidemark-scan/src/bulk/mod.rs` | final signature |
| `BulkError` | wave-2 / W2-B | `crates/tidemark-scan/src/bulk/mod.rs` | shape left to the owning task |
| `ModeRequest` | wave-2 / W2-C | `crates/tidemark-scan/src/mode.rs` | final |
| `ModeFacts` | wave-2 / W2-C | `crates/tidemark-scan/src/mode.rs` | final |
| `choose_mode` | wave-2 / W2-C | `crates/tidemark-scan/src/mode.rs` | final signature |
| `Progress` | wave-2 / W2-C | `crates/tidemark-scan/src/progress.rs` | final |
| `ScanRequest` | wave-2 / W2-C | `crates/tidemark-scan/src/orchestrator.rs` | final |
| `ScanOutcome` | wave-2 / W2-C | `crates/tidemark-scan/src/orchestrator.rs` | final |
| `ScanRecorder` | wave-2 / W2-C | `crates/tidemark-scan/src/orchestrator.rs` | final |
| `scan_started` | wave-2 / W2-C | `crates/tidemark-scan/src/orchestrator.rs` | final signature |
| `scan_finished` | wave-2 / W2-C | `crates/tidemark-scan/src/orchestrator.rs` | final signature |
| `dir_series` | wave-2 / W2-C | `crates/tidemark-scan/src/orchestrator.rs` | final signature |
| `run_scan` | wave-2 / W2-C | `crates/tidemark-scan/src/orchestrator.rs` | final signature |
| `ScanError` | wave-2 / W2-C | `crates/tidemark-scan/src/orchestrator.rs` | shape left to the owning task |
| `FanotifyCaps` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final |
| `probe` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final signature |
| `InitMode` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final |
| `Fanotify` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | shape left to the owning task |
| `Fanotify::init` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final signature |
| `Fanotify::mark_mount` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final signature |
| `Fanotify::mark_filesystem` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final signature |
| `Fanotify::read` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final signature |
| `Fanotify::as_fd` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final signature |
| `RawEvent` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final |
| `open_by_handle` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final signature |
| `CLOSE_WRITE` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final |
| `MODIFY` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final |
| `CREATE` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final |
| `DELETE` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final |
| `MOVED_FROM` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final |
| `MOVED_TO` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final |
| `ONDIR` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | final |
| `EventIter` | wave-5 / W5-A | `crates/tidemark-sys/src/fanotify.rs` | shape left to the owning task |
| `WatcherOptions` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final |
| `WatchMode` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final |
| `HotScope` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final |
| `WatchStats` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final |
| `Watcher` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | shape left to the owning task |
| `Watcher::start` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final signature |
| `Watcher::mode` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final signature |
| `Watcher::hot` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final signature |
| `Watcher::drain_completed_minutes` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final signature |
| `Watcher::take_dirty` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final signature |
| `Watcher::stats` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final signature |
| `Watcher::stop` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final signature |
| `namespace_ok` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | final signature |
| `WatchError` | wave-5 / W5-A | `crates/tidemark-live/src/watch/mod.rs` | shape left to the owning task |
| `ProcScannerOptions` | wave-5 / W5-B | `crates/tidemark-live/src/procscan.rs` | final |
| `ProcScanResult` | wave-5 / W5-B | `crates/tidemark-live/src/procscan.rs` | final |
| `ProcScanner` | wave-5 / W5-B | `crates/tidemark-live/src/procscan.rs` | shape left to the owning task |
| `ProcScanner::new` | wave-5 / W5-B | `crates/tidemark-live/src/procscan.rs` | final signature |
| `ProcScanner::scan_once` | wave-5 / W5-B | `crates/tidemark-live/src/procscan.rs` | final signature |
| `ProcScanner::phantom` | wave-5 / W5-B | `crates/tidemark-live/src/procscan.rs` | final signature |
| `ProcScanner::writers` | wave-5 / W5-B | `crates/tidemark-live/src/procscan.rs` | final signature |
| `ProcScanner::take_samples` | wave-5 / W5-B | `crates/tidemark-live/src/procscan.rs` | final signature |
| `PgProbe` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final |
| `probe_from_config` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final signature |
| `PgRow` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final |
| `PgConn` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final |
| `query` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final signature |
| `PgConnector` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final |
| `connect` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final signature |
| `PgCollector` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | shape left to the owning task |
| `PgCollector::new` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final signature |
| `PgCollector::with_connector` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final signature |
| `PgCollector::collect_fast` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final signature |
| `PgCollector::collect_relations` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final signature |
| `PgCollector::path_labels` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final signature |
| `PgCollector::latest` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final signature |
| `PgCollector::backoff_until` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` | final signature |
| `slope_bytes_per_sec` | wave-5 / W5-D | `crates/tidemark-live/src/rates.rs` | final signature |
| `window_rate` | wave-5 / W5-D | `crates/tidemark-live/src/rates.rs` | final signature |
| `time_to_full` | wave-5 / W5-D | `crates/tidemark-live/src/rates.rs` | final signature |
| `MountRates` | wave-5 / W5-D | `crates/tidemark-live/src/rates.rs` | final |
| `DirGrowth` | wave-5 / W5-D | `crates/tidemark-live/src/rates.rs` | final |
| `AlertInputs` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/mod.rs` | final |
| `AlertEngine` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/mod.rs` | shape left to the owning task |
| `AlertEngine::new` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/mod.rs` | final signature |
| `AlertEngine::evaluate` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/mod.rs` | final signature |
| `AlertEngine::active` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/mod.rs` | final signature |
| `AlertError` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/mod.rs` | shape left to the owning task |
| `Deliver` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/deliver.rs` | final |
| `deliver` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/deliver.rs` | final signature |
| `JournalDeliver` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/deliver.rs` | shape left to the owning task |
| `WebhookDeliver` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/deliver.rs` | shape left to the owning task |
| `DeliverError` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/deliver.rs` | shape left to the owning task |
| `Deliver::deliver` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/deliver.rs` | final signature |
| `Deliver::deliver` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/deliver.rs` | final signature |
| `NowInputs` | wave-5 / W5-D | `crates/tidemark-live/src/now.rs` | final |
| `build_now` | wave-5 / W5-D | `crates/tidemark-live/src/now.rs` | final signature |
| `render_now_text` | wave-5 / W5-D | `crates/tidemark-live/src/now.rs` | final signature |
| `PgError` | wave-5 / W5-C | `crates/tidemark-pg/src/collector.rs` (root re-export) | shape left to the owning task; NotYetImplemented retained |
| `default_rules` | wave-5 / W5-D | `crates/tidemark-live/src/alerts/mod.rs` | re-export of final core function |

## Exact downstream write scopes

All paths below are repository-relative. No task may edit Cargo.toml, Cargo.lock, crate lib.rs/main.rs or shared root/module declarations. Task-local agg/bulk/watch/alerts mod.rs files have only one owner and are included explicitly below. Existing descendants and new files inside listed directories belong to that single task. Preparation tests remain prep-owned; implementers should replace their unavailable-result assertions through a coordinator-owned follow-up when operational implementations land, rather than preserve stub behavior.

| Task | Owned implementation files and exclusive new-file directories |
|---|---|
| W2-A | `crates/tidemark-scan/src/agg/mod.rs`, `agg/lca.rs`, `agg/linkset.rs`, `agg/notable.rs`, `agg/rollup.rs`, new files under `crates/tidemark-scan/src/agg/`; task tests under new `crates/tidemark-scan/tests/w2a.rs` and `crates/tidemark-scan/tests/w2a/` |
| W2-B | `crates/tidemark-scan/src/bulk/mod.rs`, `bulk/join.rs`, `bulk/names.rs`, new files under `crates/tidemark-scan/src/bulk/`; task tests under new `crates/tidemark-scan/tests/w2b.rs` and `crates/tidemark-scan/tests/w2b/` |
| W2-C | `crates/tidemark-scan/src/orchestrator.rs`, `mode.rs`, `progress.rs`; `crates/tidemark-cli/src/cmd/scan.rs`, `snapshots.rs`, `top.rs`, `files.rs`; `crates/tidemark-cli/src/render/tree.rs`, `scan_progress.rs`; task tests under new `crates/tidemark-scan/tests/w2c.rs` and `crates/tidemark-scan/tests/w2c/` and `crates/tidemark-cli/tests/w2c/` |
| W5-A | `crates/tidemark-sys/src/fanotify.rs`; `crates/tidemark-live/src/watch/mod.rs`, `watch/fdmode.rs`, `watch/fidmode.rs`, `watch/rings.rs`, new files under `crates/tidemark-live/src/watch/`; `crates/tidemark-cli/src/cmd/watch.rs`; task tests under new `crates/tidemark-live/tests/w5a.rs` and `crates/tidemark-live/tests/w5a/` and `crates/tidemark-cli/tests/w5a/` |
| W5-B | `crates/tidemark-live/src/procscan.rs`; `crates/tidemark-cli/src/cmd/phantom.rs`, `writers.rs`; task tests under new `crates/tidemark-live/tests/w5b.rs` and `crates/tidemark-live/tests/w5b/` and `crates/tidemark-cli/tests/w5b/` |
| W5-C | `crates/tidemark-pg/src/collector.rs` (including PgError), `labels.rs`, `queries.rs`, new files under `crates/tidemark-pg/src/` except `lib.rs`; `crates/tidemark-cli/src/cmd/pg.rs`; task tests under new `crates/tidemark-pg/tests/w5c.rs` and `crates/tidemark-pg/tests/w5c/` and `crates/tidemark-cli/tests/w5c/` |
| W5-D | `crates/tidemark-live/src/rates.rs`, `now.rs`; `crates/tidemark-live/src/alerts/mod.rs`, `alerts/rules.rs`, `alerts/deliver.rs`, new files under `crates/tidemark-live/src/alerts/`; task tests under new `crates/tidemark-live/tests/w5d.rs` and `crates/tidemark-live/tests/w5d/` |

Use task-owned module unit tests for new tests without a manifest edit. The proposed integration entry files are non-overlapping ownership reservations: Cargo auto-discovers `tests/w2a.rs` etc.; those files can include their matching task directory without a manifest edit. The stub tests in `tests/preparation.rs` are auto-discovered as well. W5-C additionally owns a new `crates/tidemark-pg/tests/support/pgtmp.rs` if it follows the wave file's integration-fixture layout.

The current W5 interfaces place render_now_text in live/src/now.rs, and specify no CLI render module. No extra CLI render path was added outside the approved tree/scan_progress scope. PgError's relocation means W5-C can extend it without touching its crate root.

## Tests and validation

Four compile-level tests cover scan, live, pg and sys. Constructors, mutators, pure placeholders, required trait methods, every Result-returning stub and iterator/reference behavior are named and exercised. CLI only receives empty module placeholders, so it has no new callable stub; existing workspace CLI tests compile those modules. Tests use harmless directory descriptors, synthetic DTOs and injected timestamps. No privileged commands were executed; future task-specific runtime tests are pending VM.

Final validation results are appended below. Logs are under the canonical attempt output library. Initial Clippy caught a test-only unwrap lint; fixture-only allow attributes fixed it. An initial test build overlapped a sink-error representation edit and observed stale implementation output; final validation is run against the settled sources.

## Known limitations

Stubs do not provide runtime acceptance evidence or useful operational metrics. Constructor-only objects use minimal unit/private fields. Fanotify as_fd borrows its private descriptor; init always fails, so operational callers cannot create a stub instance. Unit tests construct an inert instance with an ordinary directory fd and perform no fanotify calls. Name-only errors retain only required unavailable/cancel/busy/no-space/namespace/unsupported variants; owning tasks define remaining shapes. No memory/performance or VM claim applies to this preparation.

No branch creation, merges, pushes, privileged runtime operations, host metadata modification, system clocks, unsafe code, todo!/unimplemented! or production panics were introduced. Public declarations have rustdoc; source ownership tags are preserved. Library stub logic performs no I/O, starts no threads and calls no callbacks.

## Final local check results

Environment: pinned Rust 1.98.0, RUSTUP_HOME=/home/brewerm/.rustup, CARGO_HOME=/tmp/tidemark-prep-cargo copied from the host registry, CARGO_NET_OFFLINE=true. Host registry was not modified.

| Check | Result | Evidence |
|---|---|---|
| cargo fmt --check | exit 0 | library/fmt.log and .exit |
| cargo clippy --offline --workspace --all-targets -- -D warnings | exit 0 | library/clippy.log and .exit |
| cargo test --offline --workspace | exit 0; 172 passed, 0 failed, 3 existing scale tests ignored | library/test.log and .exit |
| cargo build --offline --workspace --features privileged --tests | exit 0; compilation only | library/privileged.log and .exit |
| RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps | exit 0 | library/doc.log and .exit |
| tools/ci.sh with offline environment | exit 0; cargo-deny unavailable and skipped | library/ci.log and .exit |
| Signature/field audit | 67 interface function signatures and 19 explicit field lists match | library/contract-audit.txt |
| git diff --check | exit 0 | local whitespace check |

All required workspace commands passed locally, so no crate-only fallback was needed. Privileged runtime checks remain pending VM. Advisory/license auditing remains pending an equipped host because cargo-deny is unavailable. No independent verification or wave-entry approval is claimed.
