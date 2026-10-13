# Handoff — EDGE-metrics

## Needs from coordinator

This is implementation and validation evidence for review, not a verified acceptance decision. Privileged XFS/ownership execution, equipped-host dependency auditing, and any sandbox-blocked readiness tests remain coordinator checks. No frozen contract, scheduler, state, CLI, packaging or plan file changed.

## Changes by finding

- **F01:** Removed the daemon's `tiny_http` dependency. The responder uses `std::net::TcpListener`/`TcpStream`, with safe existing rustix socket setup solely to set reuse-before-bind for reloads. Loopback validation remains. There are at most eight owned connection workers, no application queue, an 8 KiB fixed request-head buffer, independent absolute five-second read/write deadlines, and one request per connection. GET and HEAD for exact `/metrics` return 200 and the textfile exposition (HEAD suppresses the body and retains its length). Malformed heads, other paths and other methods return 400, 404 and 405 respectively; every connection closes. Listener reload closes client sockets and joins workers. A service ownership guard also cancels/joins HTTP on exporter error exits. Tests cover the ninth idle client, an unending header, oversized head, slow response reader, malformed routing, normal scrape/HEAD, identical HTTP/textfile body, disabling/re-enabling the same address, and shutdown with idle clients within one second. TCP tests print a single diagnostic and return if bind is unavailable. Loopback ran here; these tests do not launch a subprocess, so there is no daemon stderr to attach.
- **F12 (exporter half):** Both generation initialization and subsequent interval scheduling use checked monotonic-clock arithmetic. An unrepresentable interval publishes `metrics=error` with an interval explanation and suspends exports until another configuration generation. No panic, clamping or validation-key change. `huge_interval_reports_error_and_recovers_after_reload` exercises `u64::MAX` seconds and subsequent recovery.
- **F11:** Textfile operations retain a collector-directory handle and use no-follow, nonblocking openat plus handle-relative rename. An owned regular, single-link temporary is exclusively flocked before truncation and reuse; a live writer, symlink, other owner, hard link or nonregular leaf is refused. A directory flock serializes our open/rename cycle, including the race where a writer opened a temporary just before another writer renamed it. Failed writes retain a recoverable staging file. File sync, 0644 and atomic same-directory rename remain. Tests update stale-file refusal into recovery, retain symlink refusal, add live-lock/nonregular refusal, and add another-owner refusal to the existing privileged XFS test.
- **F10:** The exporter seeds the shared cache before first exposition from configured roots' latest complete catalog entries and immutable snapshots, read-only. Each export compares read-only catalog identities, configured roots, depth and candidate capacity, loading a complete replacement only when those change. This also repairs startup/reload races with post-scan publication and retries transient seed failures. The scheduler's refresh/publication interface is unchanged. A two-root fixture with schedules off tests two exporter starts, depth increase and root removal without a filesystem scan. Seeding ranks each root once, after staging all latest source identities.
- **F09:** Added only `Snapshot::metrics_candidates(depth, limit)` to the store reader. It returns independently ranked used/reclaimable candidates, exact eligible-row count and the immutable source path; no existing signature or schema changes. SQL ranks before LIMIT and counts separately, with the existing one-million-row query ceiling. The cache retains global top candidates independently per family, bounded by `max_series_per_metric` (default 500), rather than retaining/rejecting entire inventories. Each merge holds at most twice that cap per family; a query holds at most the cap per family. Roots remain capped at 128. Sources are reopened read-only when refreshing another root, so replacing a formerly dominant root uncovers previously omitted candidates. Exact eligible counts generate dropped-row counts even for roots with no retained directory candidates. Latest errors update with the replacement. Store and exporter tests cross 100,000 rows, put the largest used row after the former name cutoff, rank reclaimable independently, combine roots, replace a dominant root and shrink depth.

## Regression evidence

Targeted tests passed with the fixes. To check that they detect missing fixes, temporary source mutations were applied one at a time and restored afterward. Each command exited 101 with a test assertion failure, not a compilation failure; logs are in the attempt report's `library/`:

| Finding | Mutation | Failing test |
| --- | --- | --- |
| F01 | Allow nine workers | `metrics::tests::http_ninth_idle_client_closed_and_shutdown_joins_all_workers` |
| F12 | Restore unchecked `Instant + interval` | `huge_interval_reports_error_and_recovers_after_reload` (also records exporter panic) |
| F11 | Refuse every preexisting temporary | `atomic_publication_and_modes` |
| F10 | Bypass catalog reconciliation | `exporter_seeds_restart_and_reconciles_depth_and_root_removal` |
| F09 | Restore the 100,000-row inventory rejection | `ranked_large_inventory_keeps_both_families_and_exact_counts` |

These are controlled mutation checks of the failure mechanisms, not a claim that the entire old tree was rerun. Final tests and check results appear below.

## Existing tests updated

Workspace-wide search found the affected assertions/helpers in `tidemarkd/tests/w6b.rs` and `w6b/source.rs`; no other workspace tests asserted temporary refusal or used the old HTTP helper types. Updated `loopback_http_only_get_metrics` for the standard listener/responder, HEAD, 405 and malformed 400; `atomic_publication_and_modes` for recovery; the populated `CachedRoot` literal for independent families/counts; and `vm_xfs_textfile_publication_root_owned_and_readable` for other-owner refusal. Readiness tests in `w6a1.rs` were inspected; startup DTO/lifecycle behavior is unchanged and their paths are outside this task.

Baseline affected suites contained 22 daemon unit tests, six unprivileged w6b tests and 29 storage tests. Final suites contain 26, 12 and 30 respectively (eleven new unprivileged tests total); one existing privileged test gained assertions. The initial six-test w6b run passed, but was after transport/recovery changes, not an untouched-baseline execution.

## Shared files and dependencies

Removed one daemon manifest dependency and its daemon lockfile edge. The workspace `tiny_http` line and locked crate remain because `tidemark-live` uses it in tests. No dependency was added, no module declaration changed, and `deny.toml` needed no edit. Offline metadata resolution was checked.

## Assumptions and limitations

- Configured series capacity is the cache bound; default-only standalone `SnapshotMetrics` helpers use 500 until exporter reconciliation supplies configuration. Counts refer to omitted eligible snapshot rows, including collisions merged by the existing lossy-label renderer. Candidate ties are deterministic raw root/path bytes; exposition retains its existing label ordering and duplicate-label maximum policy.
- Collector directory ancestry is administrator configured. Handle-relative operations pin the collector directory; flock coordinates participating writers. Symlinks, other owners and hard links are refused rather than treating an aliased file as disposable Tidemark staging data.
- Latest immutable snapshot files remain available while refreshing; a store failure preserves the prior replacement and reports degradation. Refresh may read other roots' latest snapshot files to recover global candidates, never regular files in scanned roots.
- HTTP network cancellation is bounded; immutable SQLite initialization and textfile fsync retain the project's existing in-flight kernel I/O limitation. No privileged test, mount/LVM command, service operation or host configuration write ran.
- The store's existing maximum result size applies to the new query; unsupported oversized query limits produce a controlled store error rather than an enlarged cache.

## Proposed plan text (not applied)

Replace Wave 6's tiny_http paragraph with: “Optional loopback metrics uses a standard TCP responder, admits at most eight active connections with no application queue, reads one request head into 8 KiB under an absolute five-second deadline, and writes under a separate five-second deadline. Exact GET/HEAD `/metrics` returns text exposition; malformed requests, other paths and methods receive 400/404/405. All responses close. Reload/shutdown closes owned sockets and joins every HTTP worker within one second.”

Add: “Owned, single-link regular crash staging files are flocked, truncated and reused through a collector-directory handle; live/hostile staging files are refused. Exporter startup and root/depth/capacity reload reconcile the read-only latest-complete snapshot cache. Independent global directory rankings retain at most the configured series cap per family and count omitted eligible rows exactly. A nonrepresentable metrics interval reports component error and waits for another configuration generation.”

## Final validation

Final frozen-source results (logs and exit codes in the attempt output's `library/`):

| Command | Result |
| --- | --- |
| `cargo metadata --offline` | Pass |
| `cargo fmt --check` | Pass |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Pass |
| `cargo test --offline --workspace --no-fail-fast` | 568 passed, 5 failed, 13 ignored; exit 101 |
| `cargo build --offline --workspace --features privileged --tests` | Pass; compilation only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Pass |
| `tools/ci.sh` with offline Cargo environment | Exit 1 at workspace tests, same five failures |

The five failures are the documented sandbox readiness failures in `w6a1`: `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`. Each reports `deadline exceeded` at w6a1.rs:27. These require coordinator execution with Unix sockets permitted: `cargo test --offline -p tidemarkd --test w6a1`. Targeted daemon unit tests (26) and w6b tests (12) all passed, including actual loopback HTTP tests. Storage passed 29 with one existing scale test ignored. Cargo-deny is absent on this workstation (`cargo deny --version` exits 101); the CI wrapper stops at tests before dependency auditing, so that audit remains unevaluated here.

Commands ran with Cargo/Rustup homes under `/home/brewerm`, `CARGO_NET_OFFLINE=true`, four build jobs and two test threads, under `timeout 1500`; privileged tests were compiled only. Equipped CI should rerun `CARGO_NET_OFFLINE=true tools/ci.sh`; Rocky guest execution is `cargo xtask vm-test --host <vm> --filter vm_xfs_textfile_publication_root_owned_and_readable` (or the existing full VM gate). Prometheus exposition documentation was read at https://prometheus.io/docs/instrumenting/exposition_formats/; metric names/types/labels and output format remain frozen.

## Findings not done

No requested implementation finding intentionally left out. F12 validation-time rejection remains with its separate task. Environmental verification limitations are listed above and will be reflected in the final command results.
