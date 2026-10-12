# Handoff — W6-B: Metrics exporter, dashboard and alert rules

## Needs from coordinator

This is an implementation and validation evidence candidate, not independently verified task success.

- The mandated `tiny_http` 0.12 transport has no public socket write timeout/cancellation or connection/request queue limits. One response worker isolates socket writes from the exporter supervisor. Shutdown waits at most 250 ms for that worker and then detaches it if a client is stalled. Ordinary responses are joined. This prevents HTTP from blocking the returned service handle, but does **not** guarantee joining every transport thread, bounded tiny_http internals, or reloading a listener held by a stalled response. A transport/dependency follow-up is needed before claiming the full bounded-everything contract. The needed transport surface is `Server::http_with_limits(address: SocketAddr, max_connections: usize, max_pending_requests: usize, read_timeout: Duration, write_timeout: Duration) -> Result<Server, Error>` plus `Server::cancel_all(&self)` that interrupts active writes and reads. No shared daemon interface or manifest was changed to work around this.
- The immutable cache admits at most 100,000 directories across 128 roots. Refresh rejects an oversized inventory and preserves the previous cache; the scheduler receives the explicit error. Hosts whose configured metric depth exceeds this bound need a store-owned streaming or ranked/counting query seam for exact largest-first selection beyond the cache cap. Existing `Snapshot::subtree_dirs` returns a bounded Vec and its SQL connection is crate-private. Suggested seam: `Snapshot::visit_subtree_dirs(id: u32, depth: u32, visitor: &mut dyn FnMut(DirRow) -> Result<(), StoreError>) -> Result<(), StoreError>`. No store change was made here.
- `promtool` validation, Grafana import and privileged XFS execution remain pending equipped tooling/a VM.

- **Attempt:** attempt-a71b678cf1db7ee53cd747456d4ce19f776abf2b901f703e7b56597d23a32153
- **Branch:** existing `hp-a71b678cf1db7ee53cd747456d4ce19f776abf2b901f703e7b56597d23a32153`; no branch creation, merge or push.
- **Date:** 2026-10-12
- **Status:** implementation submitted for review with the transport/scale limitations above.

## What was built

Implemented every metric family in `08` §4, with HELP/TYPE metadata, exact labels, escaped lossy path display, stable family/label ordering, largest-value-first per-family caps, and dropped-series gauges. Unavailable measurements produce no sample; time-to-full is absent for nonpositive growth. Active alerts supply firing series. Duplicate label identities (including lossy byte-path collisions) retain the largest value rather than producing invalid duplicate samples.

`SnapshotMetrics` stores read-only complete-snapshot aggregates, atomically replaced per root after successful refresh. Directory used bytes are the snapshot's `t_disk` (du-compatible union semantics); reclaimable bytes are `t_reclaimable`. Errors come from snapshot metadata. Rendering performs no snapshot I/O. The scheduler's existing shared-handle refresh/publish hook compiles unchanged.

The exporter writes exclusive `<textfile>.tmp` files in the collector directory, sets 0644, fsyncs the file, then renames it. It does not create the collector directory. Missing/unwritable directories and listener errors report degraded `metrics` health and log at most once per hour. Empty textfile/listen settings disable the corresponding output. Config generation changes update the export schedule and listener. HTTP returns Prometheus text only for exact `GET /metrics`; all other methods/URLs receive 404, and binding refuses non-loopback addresses independently of the already-present core validator.

Added the nine-panel Grafana dashboard covering all seven requested topics and the seven named Prometheus alerts. No PostgreSQL assets, group setup, shared-file edits or new dependencies were added.

## Public interfaces

Existing interfaces retain their signatures; the cache stays non-Clone:

```rust
pub struct SnapshotMetrics { /* private fields; implements Default */ }
impl SnapshotMetrics {
    pub fn refresh(&self, snapshot: &tidemark_store::reader::Snapshot,
        depth: u32) -> Result<(), crate::DaemonError>;
}
pub fn render(view: &crate::state::LiveStateView, snapshots: &SnapshotMetrics,
    config: &tidemark_core::config::MetricsConfig, now: i64)
    -> Result<String, crate::DaemonError>;
pub fn start(config: Arc<tidemark_core::config::Config>,
    state: crate::state::LiveState, shutdown: crate::Shutdown)
    -> Result<crate::ServiceHandle, crate::DaemonError>;
```

Additive helpers used by tests:

```rust
pub fn write_textfile(path: &Path, text: &str) -> std::io::Result<()>;
pub fn listen(address: &str) -> Result<tiny_http::Server, crate::DaemonError>;
pub fn respond(request: tiny_http::Request, text: &str) -> std::io::Result<()>;
```

`render` uses the injected clock already captured by `LiveStateView`; service callers construct `state.view_at(now)`. Its legacy `now` argument is retained. The private `MetricsSource` adapter delegates directly to frozen view reads and lets tests supply deterministic measured inventories without host process attribution or fanotify privileges. It is not another live-state store.

## Assumptions

- The scheduler publishes/refreshes the cache after completed scans; this exporter does not backfill it from the catalog at startup. Directory samples are absent until that shared cache exists.
- Directory cache bounds are documented constants, avoiding a config-contract change. A failed/partial/oversized refresh does not replace the preceding complete cache.
- Five-minute hot metrics include `dir`, `unit` and `comm` scopes, top ten per mount/scope. Directory hot values prefer exact published windows; otherwise the existing integral-five-minute watcher API is used. Unsupported/unpublished data remains absent.
- Open-writer growth is aggregated by measured `(mount, unit)`, then the ten largest pairs are exported. Writers with unknown units are omitted instead of fabricating a unit label.
- Time-to-full uses positive 15-minute filesystem growth. The plan does not specify choosing between its two growth windows.
- `tidemark_metrics_dropped_series` is a current-render gauge, omitted when nothing was dropped. Its own label inventory is also capped, retaining the largest omission counts.
- Build commit comes from `GIT_COMMIT` when injected by the build; otherwise its label explicitly says `unknown`.
- `TidemarkDown` uses the mandated `absent(tidemark_up)`. A previously published textfile persists after a daemon crash, so absence alone does not detect that case; direct HTTP scraping or separate collector-file freshness monitoring is needed. No undocumented heartbeat metric was added.
- The rules file uses JSON syntax, a YAML subset, so the existing JSON parser validates both shipped asset formats without a forbidden dependency edit. Thresholds are examples: time-to-full 24 h, pool 85%, phantom 1 GiB, scan age 48 h. Operators can adjust these packaging assets.
- Exclusive temporary creation deliberately refuses an existing `.tmp` and leaves it intact. A crash-left temporary file requires operator cleanup; no unrelated/stale file is silently removed.

## Shared-file edits

None. Only the assigned metrics source, W6-B tests, Grafana/Prometheus assets and this note changed. No Cargo manifests, lockfile, module declaration or frozen DTO changed.

## Acceptance evidence

| Requirement | Evidence | Local result |
| --- | --- | --- |
| Every documented family/type/label, no extra family | `metrics::tests::every_contract_metric_and_exact_labels_from_populated_source` parses the normative table and compares exact sample-family sets and labels | passed |
| Exposition parser, unavailable omission, rates/firing gauges | `measured_state_and_unavailable_omission` uses frozen state publications and a strict small text parser | passed |
| Cardinality with 10,000 directories, largest retained, hostile raw names, cached depth | `cardinality_10000_hostile_names_and_cached_depth` builds an immutable snapshot and verifies exact dropped counts, order and refresh | passed |
| Atomic replacement and 0644 permissions | `atomic_publication_and_modes` runs a synchronized concurrent reader over 100 replacements; also checks missing directory and preexisting temp refusal | passed |
| Ephemeral loopback HTTP and exact routing | `loopback_http_only_get_metrics` sends actual requests and checks 200/404/content type; non-loopback refusal is asserted | passed |
| Reload, degraded status and bounded service shutdown | `exporter_reload_health_and_shutdown` waits for states with deadlines, using temporary paths | passed |
| Dashboard/rules syntax and known metric references | `dashboard_and_yaml_rules_contract` parses both assets and checks expressions against the normative metric table | passed |
| `promtool check rules packaging/prometheus/tidemark-alerts.yml` | promtool not installed; no installation attempted | pending |
| Grafana dashboard import | Grafana unavailable; structural JSON checks run | pending |

## Privileged tests

`vm_xfs_textfile_publication_root_owned_and_readable` is compiled only with `--features privileged`. It uses `test-support::xfs::LoopXfs` and asserts successful textfile publication on loop XFS, root:root ownership, 0644 permissions and complete exposition bytes. **Pending VM**; no mount, LVM, systemd or other privileged runtime action ran on this workstation.

## Final validation

Final commands run offline against the host cache with Rust 1.98.0, `PATH=/home/brewerm/.cargo/bin:$PATH`, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_NET_OFFLINE=true`, `RUST_TEST_THREADS=2`, and `timeout 1500` around each long command. The attempt output `library/` contains logs and exit-code files. All six requested commands exited 0. Final CI reran formatting, workspace Clippy, workspace tests and privileged-test compilation on the final Rust source; the standalone fmt/Clippy/rustdoc reruns also passed. Initial workspace test/build logs predate the final small renderer corrections, which are covered by `ci.log`. cargo-deny was unavailable and CI skipped it; advisory/license checking remains pending an equipped host.

| Command | Exit | Evidence |
| --- | --- | --- |
| `cargo fmt --check` | 0 | `library/fmt-final.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | 0 | `library/clippy-final.log` |
| `cargo test --offline --workspace` | 0 | `library/test.log` |
| `cargo build --offline --workspace --features privileged --tests` | 0 | `library/privileged-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | 0 | `library/doc-final.log` |
| `tools/ci.sh (CARGO_NET_OFFLINE=true)` | 0 | `library/ci.log` |


## Definition of Done

- [x] Required fmt/Clippy/workspace-test/privileged-build/rustdoc/CI commands exited 0 locally.
- [x] Only owned paths changed; no shared-file edits.
- [x] Public items documented and owner headers present; no unsafe code.
- [x] Unprivileged exporter tests written and locally passed.
- [x] Privileged test written; execution pending VM.
- [x] Handoff records assumptions and gaps.
- [ ] Full transport bounds and all-worker join guarantee: coordinator follow-up above.
- [ ] promtool and Grafana import acceptance: equipped environment pending.
