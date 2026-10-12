**NOT READY** — **1 BLOCKER, 12 SHOULD-FIX, 0 NIT findings**.

This is a review evidence candidate, not a human gate approval. Reviewed the exact tree at `cd7898edff0322e823b3e6d3e94be3f2e746f90c`, relative to `420d801dadfc018cb90a5720e07111055573f8c7`, within the requested Wave 6 API/CLI/metrics/packaging scope. Source locations below refer to that candidate. The report submission baseline `d33369c3ade14757e4b6c31625bdbbcf2fede260` is newer; it is **not** the code tested here. Inspection/builds used a detached checkout of the candidate, then the original attempt branch was restored for the report commit. No implementation files were changed.

Read AGENTS.md, all of Wave 6, relevant 08 contracts, 11 rollout, ADR-018/019 and all four requested handoffs, including assumptions/coordinator needs. The candidate lacks the requested R3 section; it was read from the supplied submission baseline using `git show`. Applied the retained uid-0-only decision/R3-14 over older candidate plan text accepting the daemon's uid. This matches the candidate fix's explicit assumption. R3-4's reduced daemon LVM view and R3-15's current-run history limitation are treated as decided scope/open risks, not new defects.

## Findings

### F01 — BLOCKER: optional HTTP metrics permits unbounded local resource consumption

**Reference:** `finding:W6-edge-F01`. **Locations:** `crates/tidemarkd/src/metrics.rs:471`, `:532`, `:599`.

With `metrics.listen='127.0.0.1:9877'`, any local account can open unlimited idle connections or send an unending request/header line. The single application responder does not bound tiny_http's internal transport. The exact locked dependency, tiny_http 0.12.0, was checked in the host registry: `src/lib.rs:320–338` schedules each client; `src/util/task_pool.rs:67–83` creates workers without a maximum; `src/util/messages_queue.rs:29–33` appends to an unbounded queue; `src/client.rs:80–101` grows a line until CRLF. A slow response reader can also block `respond`, preventing listener reload. Shutdown detaches the responder after 250 ms. Thus threads, queued requests, request bytes, and socket lifetime violate the bounded-everything requirement. HTTP is disabled by default; this becomes reachable when the documented optional listener is enabled. The W6-B handoff acknowledges this unresolved seam.

**Change:** provide bounded admission/queues/header bytes, absolute read/write deadlines, and cancellation that closes sockets and joins transport workers; preserve loopback validation and exact routing. The current dependency's public interface lacks these controls: obtain the normal dependency/contract decision rather than silently claiming the detached worker solves it. **Fix files:** `crates/tidemarkd/src/metrics.rs`, `crates/tidemarkd/tests/w6b.rs` (existing `loopback_http_only_get_metrics`, `exporter_reload_health_and_shutdown`, and new hostile-client cases); `Cargo.toml`, `Cargo.lock`, `crates/tidemarkd/Cargo.toml` only if the approved transport changes; the associated SCR/handoff. Preserve `metrics::start` so its `crates/tidemarkd/src/lib.rs:466` caller needs no edit. Existing `listen`/`respond` external callers are the w6b tests. A finite local probe against the exact candidate `metrics::listen` observed 6 threads at listener readiness, 19 with 32 idle clients, and 51 with 64 idle clients. All sockets were then closed; after 500 ms the process still had 50 threads (not a claim of permanent leakage). Descriptor counts include both ends because clients/server were in one process. `library/metrics-idle.rs` and `.log` record this probe. Unlimited-line/queue and stalled-response claims remain source analysis, not destructive stress tests.

### F02 — SHOULD-FIX: API connections never expire before a complete request

**Reference:** `finding:W6-edge-F02`. **Locations:** `crates/tidemarkd/src/api.rs:147–166`, `:177–198`, `:217`, `:249`.

Open 32 connections and send nothing, or one byte without newline on each. Each 100 ms read timeout simply restarts the loop forever; the ten-second deadline begins only after a whole JSON request has been parsed and a handler spawned. Every admission slot remains occupied, and all subsequent CLI clients are dropped. Peer credentials are fetched before reading, but uid authorization is deferred until `dispatch`: a nonroot peer able to reach the socket is also allowed to hold a slot, feed the parser, and receive `bad_request` for malformed JSON rather than being refused first. A failed credential lookup does correctly close immediately. Response write_all uses a per-write timeout with no total deadline/shutdown check, so a reader making tiny progress can also extend response and connection shutdown well beyond ten seconds.

**Change:** refuse unauthorized uid immediately after SO_PEERCRED, before buffering/parsing; enforce an absolute idle/frame deadline that partial traffic cannot extend indefinitely, then release the permit. Make response writes deadline- and shutdown-aware too. Keep the 1 MiB ceiling. **Fix files:** `crates/tidemarkd/src/api.rs` and `crates/tidemarkd/tests/w6a3.rs`. Update/extend `thirty_two_connections_then_rejects_without_queueing`, framing/idle tests and `injected_nonroot_peer_is_forbidden_over_real_socket`; add idle-slot reclamation, drip-feed, malformed-nonroot and failed-credential assertions. No public type changes are required. The current admission test proves rejection only, not recovery.

### F03 — SHOULD-FIX: API timeout abandons running handlers instead of ending them

**Reference:** `finding:W6-edge-F03`. **Locations:** `crates/tidemarkd/src/api.rs:240–245`, `:252–271`, `:374`.

The handler JoinHandle is dropped after spawn. A timed-out callback retains its permit/context indefinitely, and server shutdown joins only connection workers. The existing `pair_controllable_timeout_and_shutdown` and `controllable_handler_timeout_keeps_admission_bounded` tests prove this directly: they join the server **before** releasing the blocked reload callback. Thirty-two stuck callbacks exhaust future admission until those callbacks finish, indefinitely if they never do; an in-flight reload can continue after transport shutdown. Memory/thread count is capped, but lifetime and complete worker shutdown are not. The W6-A3 handoff explicitly acknowledges the detachment.

**Change:** retain handler ownership and introduce cancellation/deadline-aware execution for synchronous callbacks and their acknowledgements; report shutdown complete only after owned handlers have exited. Preserve the explicit limitation on in-flight kernel I/O from R3-7 rather than promising impossible kernel cancellation. Add a cancellation token/deadline to the internal Reload callback contract and honor cooperative cancellation before a documented configuration commit boundary. A token does not interrupt in-flight kernel I/O, and a bare pre-publication flag check cannot guarantee atomic cancellation of publication. **Fix inventory for that design:** `crates/tidemarkd/src/api.rs` (Reload, Context users, local pair fixture), `crates/tidemarkd/src/lib.rs` (reload_callback definition, lifecycle construction and SIGHUP invocation), `crates/tidemarkd/tests/w6a3.rs` (context and reload closures/timeout tests), `crates/tidemarkd/tests/w6a1.rs:498` (direct reload_callback invocation). Coordinate any scheduler acknowledgement cancellation adjustment in `crates/tidemarkd/src/scheduler.rs` and its `tests/w6a2/runtime.rs` tests. The two existing tests must assert callback exit before service join succeeds, not encode detachment as success. Wire DTOs need no change.

### F04 — SHOULD-FIX: API path operations depend on unchecked, mutable parent paths

**Reference:** `finding:W6-edge-F04`. **Locations:** `crates/tidemarkd/src/api.rs:313–331`, `:373`; `crates/tidemark-core/src/config.rs:747`.

The final leaf is checked with symlink_metadata, but parent ownership/modes/symlinks are not checked or pinned. Configuration only requires an absolute socket path. With the permitted socket path under an attacker-writable, non-sticky directory, another user can replace the newly bound leaf with a symlink before pathname `set_permissions`, causing the root daemon to chmod the symlink target to 0600. Stale detection/unlink and shutdown unlink also do not verify that they remove the socket previously inspected/created. A full-backlog existing listener can block the startup liveness probe's raw `UnixStream::connect`. The packaged 0700 root-owned `/run/tidemark` mitigates the unprivileged race at the default location; this is a defect in accepted custom-path handling, not a claimed exploit of the default directory.

**Change:** validate and retain a trusted no-follow parent directory, refuse untrusted writable ancestry (accounting for sticky `/tmp` and private owned descendants), make stale/cleanup operations identity-aware through that handle, and bound the active-socket probe. Never chmod a substituted symlink target. **Fix files:** `crates/tidemarkd/src/api.rs`, `crates/tidemarkd/tests/w6a3.rs`; use the already-enabled safe rustix filesystem/socket operations; no unsafe implementation is needed in tidemarkd. Extend `refuses_non_socket_and_active_socket_removes_stale_socket` with leaf/parent symlinks, unsafe-parent refusal, concurrent startup/replacement and full backlog cases. No config key or wire change. Race reproduction needs an unrestricted host; source establishes the unchecked pathname sequence.

### F05 — SHOULD-FIX: scan fallback bypasses the shared client's deadline

**Reference:** `finding:W6-edge-F05`. **Locations:** `crates/tidemark-cli/src/cmd/scan.rs:33`, `:132`, `:145`.

A socket listener that never accepts and has a full backlog first causes bounded `Client::connect` to expire after 11 seconds. The broad `if let Ok` then falls into direct mode and calls blocking `UnixStream::connect` twice, without deadlines. The first can hang forever, before direct signal handling is installed. Timeout/permission errors are also incorrectly treated as evidence to try direct mode.

**Change:** allow fallback only for definitive absent/refused endpoints; return timeout/permission/other transport errors. Replace both legacy guards with deadline-bounded checks and retain the second guard under the scan lock. **Fix files:** `crates/tidemark-cli/src/cmd/scan.rs`, `crates/tidemark-cli/tests/w6a3.rs`; `client.rs` only if factoring a shared helper. Add a full-backlog subprocess test with an external deadline and no state writes; current tests exercise only a responsive fake scan server. No types change.

### F06 — SHOULD-FIX: scan polling follows a later scan on the same root

**Reference:** `finding:W6-edge-F06`. **Location:** `crates/tidemark-cli/src/cmd/scan.rs:93–110`.

After the CLI starts scan A, A can complete and B start before the next poll. A valid status then has `last_complete=A`, `running=B`. The CLI checks A's completion only when running is None, so it follows B and eventually exits 3 claiming A failed when the latest completion becomes B. A signal during that incorrect wait can send root-only cancellation for B.

**Change:** resolve the started ID's completed result first and never follow a different running ID. If the latest-status slot has advanced past A, use its existing read-only catalog entry where the local state directory is authoritative; otherwise report unavailable outcome rather than misattribute B. **Fix files:** `crates/tidemark-cli/src/cmd/scan.rs`, `crates/tidemark-cli/tests/w6a3.rs`; extend the fake scan case in `live_tables_and_shared_byte_path_machine_output` and add A-complete/B-running, A-complete/B-complete and missing-status regressions. No existing test asserts the bad sequence. Atomic “cancel only A” cannot be guaranteed by the frozen root-only cancel method; an ID-conditional cancellation extension requires a separate SCR, not a silent DTO change.

### F07 — SHOULD-FIX: daemon-backed degraded scans exit successfully

**Reference:** `finding:W6-edge-F07`. **Location:** `crates/tidemark-cli/src/cmd/scan.rs:100–104`.

A completed walk with per-directory errors is stored as complete **and degraded** (`crates/tidemark-scan/src/agg/rollup.rs:155`, `orchestrator.rs:575–582`). Scheduler `last_scan` (`crates/tidemarkd/src/scheduler.rs:484–490`) discards that flag because frozen LastScan has no outcome field. The CLI returns Ok whenever the completion ID matches, whereas direct mode at `scan.rs:211–216` correctly exits 3 for degraded completion. This breaks 08's completed-with-errors exit code. Source verified; the candidate's separate known daemon ENOTDIR issue prevents treating this as a demonstrated successful real scan here.

**Change:** read the completed ID's existing persisted outcome with read-only `Catalog::get`, and return Partial/exit 3 for degraded or partial completion; never assume clean success if outcome cannot be determined. This follows the existing CLI's local-state model. If the daemon's authoritative store cannot be located from that model, request an explicit outcome-field contract change before implementing one. **Minimal fix files:** `crates/tidemark-cli/src/cmd/scan.rs`, `crates/tidemark-cli/tests/w6a3.rs`, including complete+degraded/clean catalog-backed fake completions. No core constructor changes for the catalog-based fix; ensure existing fake-success fixture also supplies an outcome.

### F08 — SHOULD-FIX: live tables ignore terminal width

**Reference:** `finding:W6-edge-F08`. **Locations:** `crates/tidemark-cli/src/render/hot.rs:32`, `render/alerts.rs:27`, `:53`, `render/status.rs:32`, `render/now.rs:28–137`.

A 200-character hot path prints a line over 200 columns in an 80-column terminal. These renderers send complete fields to `table::aligned` or writeln; table widths derive from data, not terminal dimensions. The new TIOCGWINSZ handling applies only to `now`. This is separate from the explicitly excluded now mid-word truncation problem. Escaping controls/non-UTF-8 bytes is present in the table fields, but escaping alone does not bound line width.

**Change:** share positive-TTY-width/default-100 selection and wrap/shorten display cells safely, preserving escaped tokens and clear continuation. **Fix files for daemon-facing output:** `crates/tidemark-cli/src/render/{now,hot,alerts,status}.rs`, `crates/tidemark-cli/tests/w6a3.rs`. If covering all direct modes too, include `cmd/{phantom,writers}.rs` and `render/lvm.rs`; do not change the shared table helper without auditing its other callers. Existing substring assertions must be supplemented with injected 40/80-column tests containing long ASCII, Unicode, invalid UTF-8 and controls. Existing render signatures can remain unchanged through a private helper.

### F09 — SHOULD-FIX: snapshot cache rejects valid large inventories before cardinality selection

**Reference:** `finding:W6-edge-F09`. **Locations:** `crates/tidemarkd/src/metrics.rs:46–57`, `:66–74`.

With 100,001 eligible directories, or several roots whose combined cache exceeds 100,000, refresh fails before the configured largest 500 can be chosen. First publication lacks all directory/error series and omission counts; subsequent publications retain old values despite a new successful scan. The contract calls for largest-first retention and dropped-series counts on million-wide trees. The 10,000-directory test never crosses the implementation ceiling. W6-B's proposed store streaming seam remains unresolved.

**Change:** enumerate/count eligible snapshot rows through a bounded streaming/ranked query, retain the global top candidates independently for used/reclaimable families, and cache exact omitted counts plus the latest scan errors. Do not merely enlarge MAX_DIRS. **Fix files:** `crates/tidemark-store/src/reader.rs`, `crates/tidemark-store/tests/storage.rs` (existing subtree_dirs assertions and new reader-query regression); `crates/tidemarkd/src/metrics.rs`; `crates/tidemarkd/tests/w6b.rs`, `crates/tidemarkd/tests/w6b/fixtures.rs`, and `crates/tidemarkd/tests/w6b/source.rs:171–177` (the other CachedRoot literal). Keep the existing refresh signature where possible; if adding config/capacity parameters, also update `crates/tidemarkd/src/scheduler.rs:383–393`, `:440`, `:450`, all w6b refresh calls and the injected-refresh test at `crates/tidemarkd/src/scheduler.rs:1154`. Add >100,000 rows with the largest row after the old name cutoff, multi-root, both-family and previous-cache regressions. New store seams require normal contract coordination.

### F10 — SHOULD-FIX: restart drops metrics for existing latest snapshots

**Reference:** `finding:W6-edge-F10`. **Locations:** `crates/tidemarkd/src/metrics.rs:492–500`, `:565–570`.

LiveState starts with no metrics cache (`state.rs:130`); only post-scan processing publishes it (`scheduler.rs:439–454`). Startup reads latest catalog entries for statuses but never opens them for metrics. Restart with a valid snapshot and `schedule='off'`: directory used/reclaimable and scan-error metrics disappear until a manual scan, although last-success/duration remain. The latest-snapshot contract is not restricted to snapshots from this daemon process. Root removal/depth-increase reloads also lack cache reconciliation.

**Change:** initialize the shared cache from each configured root's latest complete snapshot using existing read-only catalog/snapshot interfaces, before first export, and reconcile root/depth changes without rescanning the filesystem. **Chosen fix files:** `crates/tidemarkd/src/scheduler.rs` (initialization and config-generation reconciliation), `crates/tidemarkd/src/metrics.rs` (cache helper), `crates/tidemarkd/tests/w6b.rs` (preexisting snapshot, off schedule, restart/root-removal/depth reload). No frozen DTO change. Existing no-snapshot omission tests remain valid.

### F11 — SHOULD-FIX: a crash-left textfile temporary prevents every future update

**Reference:** `finding:W6-edge-F11`. **Location:** `crates/tidemarkd/src/metrics.rs:441–448`.

Kill the writer after creation of `<textfile>.tmp` and before rename. Every later export, even after daemon restart, fails create_new; the old complete `.prom` remains stale indefinitely. The error is logged/degraded, but normal restart cannot restore monitoring. The handoff requires manual deletion, and `crates/tidemarkd/tests/w6b.rs:284–287` deliberately asserts unconditional existing-temp refusal. This is a recovery defect, distinct from the already documented limitations of absent(up).

**Change:** safely recover an owned regular stale temporary through a retained collector-directory handle while excluding a concurrent writer; continue refusing symlinks/unowned/nonregular entries. A unique-temp alternative needs a contract decision because Wave 6 specifies `<textfile>.tmp`. **Fix files:** `crates/tidemarkd/src/metrics.rs`, `crates/tidemarkd/tests/w6b.rs`; use the existing safe rustix handle operations. Keep write_textfile's signature. Replace the valid-stale-file refusal assertion with recovery and retain/add hostile-file refusal cases. Preserve same-directory atomic rename and 0644 output.

### F12 — SHOULD-FIX: validated metrics interval panics the exporter

**Reference:** `finding:W6-edge-F12`. **Location:** `crates/tidemarkd/src/metrics.rs:593`.

`[metrics] interval='18446744073709551615s'` passes parsing and validation (`crates/tidemark-core/src/config.rs:864–885` checks only positivity). `Instant::now() + interval` then panics on overflow. **Reproduced against the exact compiled candidate libraries**, with both outputs disabled and no filesystem/service access: validation returned `[]`, followed by the metrics thread panic at line 593 and a failed join. Artifact `library/metrics-overflow.rs`/`.log` contains the probe/output.

**Change:** use checked_add and return a controlled error; reject a nonrepresentable interval before startup/reload publication, retaining the previous config on invalid reload. **Fix files:** `crates/tidemarkd/src/metrics.rs`, `crates/tidemarkd/tests/w6b.rs`; for the chosen validation-time rejection, `crates/tidemark-core/src/config.rs` and `crates/tidemark-core/tests/config_load.rs`. No key/type changes. Do not silently clamp or rely on catching a panic.

### F13 — SHOULD-FIX: install assertions do not discriminate required lifecycle behavior

**Reference:** `finding:W6-edge-F13`. **Locations:** `tests/install/install.sh:72`, `:103–120`; `xtask/src/rpm.rs:21`.

Fresh install is asserted disabled, then the test only starts the service. It never enables it before checking erase disables it, so removal of `systemctl disable` from preun would still pass. The documented upgrade changes only version metadata while shipping identical default config bytes, which does not test a conflicting new default requiring noreplace behavior. Auditd activity is required, but SELinux Enforcing is not, so empty AVC output can also be vacuous. No faulty production scriptlet is alleged; these are missing gate preconditions.

**Change:** enable/assert enabled before erase; upgrade with a test package whose default config actually changes, then assert edited live bytes remain and the new default is in `.rpmnew`; require Enforcing or explicitly mark SELinux acceptance untested. Record MainPID across upgrade to confirm the source's leave-running behavior. **Fix files:** `tests/install/install.sh`, a test-only changed-upgrade-config fixture under `tests/install/`, `packaging/rpm/README.md`, `xtask/tests/w6c.rs`. If provisioning the fixture through xtask, also update `xtask/src/rpm.rs` and `pure_commands_and_invalid_arguments`. Rerun `shell_syntax_and_vm_guard`, privileged install_vm and both install phases. No type/constructor changes.

## Candidate fixes and excluded known issues

The candidate delta enables rustix net/termios, wires production SO_PEERCRED, restricts dispatch to uid 0, selects positive stdout TTY width for now, labels human show-config as the CLI config, and replaces the obsolete phantom/writers exit-4 expectation with direct-fallback tests. Those changes match R3-14 by source inspection. Kernel socket execution remains limited by this sandbox. No undocumented method, group access, PostgreSQL feature, unsafe block or production unwrap/expect was found in the reviewed API/CLI/metrics code. Configuration-reachable panic F12 remains.

The newer named `W6-fix-daemon-scan`, `STORE-catalog-paths`, `PKG-rpm-metadata`, `CI-rpm`, `PKG-unit-fix`, `BULK-size-contract` and `TEST-vm-harness` fixes/handoffs are absent from the reviewed candidate. In particular, candidate RPM generator integration and inline-comment memory limits remain known blockers. They are **not** new findings or counted above. The known scan ENOTDIR, scheduling/retry, bulk writeback, hot publication/top-unit, alert seams, rates/status display and LVM JSON-shape issues are also excluded. No review of the in-progress LIVE-hot/WATCH-perf implementation or unrelated daemon core was undertaken beyond necessary call-boundary tracing.

## API requirements and scan path

Implemented means source contract/mapping is present; it does not imply privileged runtime acceptance. All typed params use the frozen core adapters with unknown-field/type rejection. Dispatch recognizes exactly the twelve names below; bad version/unknown method returns bad_request. Daemon errors map to the six documented codes (busy is currently matched by the Scan("Busy") string); root is checked before method/param dispatch, with the earlier transport authorization gap in F02.

| Method | Params/result and disposition |
|---|---|
| status | Empty params; exact StatusResult, components/watch modes/retained errors; implemented. |
| now | Optional mount; exact NowReport/path adapters; implemented. Known producer/render gaps excluded. |
| hot | Optional mount, enum window/by, usize limit; Vec<HotEntry>; implemented, underlying window inventory not re-reviewed. |
| lvm | Optional mount; LvmResult mounts/vgs/sampled_at; implemented reduced daemon view, R3-4. |
| phantom | Optional mount; Vec<PhantomSummary> with byte-path adapters; implemented. |
| writers | Optional mount; positive arbitrary humantime window; Vec<OpenWriter>; implemented. |
| alerts.active | Optional limit truncates active set; Vec<AlertSummary>; implemented. |
| alerts.history | Optional limit; Vec<AlertEvent>; current-run persisted history only, recorded open risk R3-15. |
| scan.start | Exactly root/root_b64, optional mode/throttle; scan_id; bounded scheduler admission and host-lock Busy; implemented with known candidate scan failure excluded. |
| scan.status | Zero/one root/root_b64; Vec<ScanStatus>; exact root filtering; implemented, latest-slot limitations F06/F07. |
| scan.cancel | Exactly root/root_b64; cancelled bool; exact active-root equality; implemented root-level cancellation. |
| config.reload | Empty params; reloaded bool plus warning strings; validation retains old config on invalid input; callback lifetime F03. |

CLI scan resolves realpath before RawPath construction and sends UTF-8 root or padded standard root_b64. The DTO rejects both/neither where forbidden, invalid base64, and invalid RawPath. API callers receive lexical validated byte paths; the API does not itself canonicalize symlinks. Scheduler `launch` matches configured roots by exact PathBuf equality (`scheduler.rs:588`). An unconfigured directory or directory below a configured root is admitted (subject cap permitting), gets default root settings and no inherited parent excludes; orchestrator later requires a real directory. This behavior is not prohibited by the quoted scan-root wire contract. A regular file fails directory setup; it is not scanned as file contents. Missing/I/O scan failures are wrapped in DaemonError::Scan and can become internal instead of the direct-I/O not_found mapping; the plan supplies no more specific per-method missing-root result. No runtime claim is made through the known ENOTDIR blocker. Connected CLI threads/exclude/keep-partial overrides are refused with usage error before scan.start, as required.

Transport has a 32-permit admission cap and 1 MiB request/response serialization limits, sequential framing, bounded one-item response channels, credential-lookup failure refusal and standard malformed-JSON/type/version errors. Disconnect/write failures end that connection. Read/write polling responds to shutdown for normal clients. Gaps are F02–F04; the 100 ms per-write socket timeout is not a complete-response absolute deadline either. A handler result is materialized as serde_json::Value before serialization, so the 1 MiB output check is not itself an allocation limit; backing state inventories provide bounds. No runtime listener/race/slow-reader acceptance is claimed.

## CLI requirements

| CLI surface / shared service | Candidate evidence / disposition |
|---|---|
| shared Client framing | Implemented: 1MiB including newline; buffered repeated frames; version/id/exclusive result-error validation; typed JSON; poisoned connection after transport/protocol error; tests in client.rs. |
| shared Client deadlines | Implemented for Client calls: whole-call remaining-time checks each write/read, EINTR retries recompute; initial connect waiter deadline and at most 16 outstanding connects. Scan has bypass finding above. Runtime bind test can skip EPERM; socketpair and pure I/O tests do exercise internals. |
| now | Client now call and JsonDoc raw NowReport; no-daemon exit 4/hint; positive TTY columns or100 fallback; shared now renderer. Known empty-hot and rate/text discrepancies excluded. |
| hot | Canonical UTF8 mount label, four windows, three scopes, limit; sorted growth/key output and shared path/path_b64 JSON. No daemon exit 4/hint. Width deviation above. |
| alerts.active | Typed no-limit call, sorted rule/subject output, escaped text and JsonDoc. Missing daemon exit 4/hint; width deviation. |
| alerts.history | --all calls frozen history and text/help labels current-run-only, accepted open risk R3-15, excluded from new findings. |
| status | Calls status and scan.status; version, uptime, component states, modes, retained errors, scan next due shown. Human show-config explicitly labels CLI config. JSON remains StatusResult (no scan/config section); this is recorded assumption, not separately recommended finding. Known false/None raw formatting excluded. |
| scan.start CLI | Canonicalizes CLI root by realpath before RawPath; core adapter sends root for UTF8 or padded root_b64 for invalid UTF8; passes mode/throttle only. Connected --threads/--exclude/--keep-partial refused usage exit 2. Busy maps precondition exit 5. Follow/degraded findings above. |
| scan.status CLI | Polls every100ms with 11s individual client call deadline; no overall duration cap (legitimate long scan). Missing status list continues forever; no scan-ID matching for active row. |
| scan.cancel CLI | Canonicalizes root and sends byte-preserving ScanCancelParams. Explicit cancel result rendered with schema tidemark.scan.v1; signal follower sends same root. No daemon returns exit4. |
| scan root config boundary | scheduler launch :588–601 finds configured root by exact PathBuf equality; unconfigured directory/below-root admitted and gets defaults, not not_found; orchestrator later verifies actual directory. API path itself is lexical RawPath validation; CLI performs realpath. This is observed behavior, no frozen contract cited requiring rejection. |
| lvm | Daemon first; one stderr fallback notice, including -q; read-only 24-hour trend query matches direct helper; reduced-field notice (R3-4). Known direct/daemon JSON shapes excluded. |
| phantom | Explicit --direct bypasses socket; absent-daemon fallback has one stderr notice; same direct setup, shared byte adapters. Fake test plus missing-mount seam test ensure fallback without real /proc scan. |
| writers | Explicit --direct bypass; positive arbitrary humantime window; two-pass direct fallback with configured interval; one stderr notice. Backend returns frozen OpenWriter byte adapters. |
| safety | No request/config-reachable unwrap/expect/panic in reviewed production CLI code; unsafe absent; no shell; control-bearing row fields use shared escapes. `status.version` printed raw but real daemon sets static build version, so no finding for fake response only. |


## Metrics requirements

All rows are statically verified against `08` §4; bare source line numbers in this table refer to `crates/tidemarkd/src/metrics.rs`. “Implemented” describes metric definition and source mapping, not VM production availability. Every family goes through finite-value filtering, escaped label string keys, max-value deduplication of identical identities, largest-value-first selection and stable label order. Two counters are TYPE counter; everything else is TYPE gauge. Non-UTF-8 paths are explicitly lossily decoded, as the metrics contract requires. No undocumented metric family is emitted. `# HELP` and `# TYPE` are supplied for emitted families; unavailable data yields absent samples.

| Family | Type / labels / unit | Result / implementation evidence |
| --- | --- | --- |
| tidemark_up | gauge; none; boolean 1 | Implemented :234. Persistent textfile is not a heartbeat; documented handoff limitation. |
| tidemark_build_info | gauge; version,commit; 1 | Implemented :235–242; commit explicitly unknown if build does not inject GIT_COMMIT. |
| tidemark_fs_size_bytes | gauge; mount; bytes | Implemented :243–252 from FsSample.size. |
| tidemark_fs_used_bytes | gauge; mount; bytes | Implemented :243–252 from FsSample.used. |
| tidemark_fs_avail_bytes | gauge; mount; bytes | Implemented :243–252 from FsSample.avail. |
| tidemark_fs_inodes_used | gauge; mount; inode count | Implemented :243–252. |
| tidemark_fs_growth_bytes_per_second | gauge; mount,window=15m/1h; bytes/s | Implemented :254–260, actual 900/3600-second view calls. |
| tidemark_fs_time_to_full_seconds | gauge; mount; seconds | Implemented :261–266, available bytes / positive 15m growth, absent if nonpositive/unavailable. |
| tidemark_lvm_lv_size_bytes | gauge; vg,lv; bytes | Implemented :280–285 from mounted LVM inventory. |
| tidemark_lvm_vg_free_bytes | gauge; vg; bytes | Implemented :272–279. |
| tidemark_lvm_thin_data_percent | gauge; vg,pool; 0–100 percent | Implemented :286–294, absent if unavailable. |
| tidemark_lvm_thin_metadata_percent | gauge; vg,pool; 0–100 percent | Implemented :286–294, absent if unavailable. |
| tidemark_dir_used_bytes | gauge; root,path; bytes | Mapping t_disk/depth correct :417–430; deviates on restart/cache overflow (F10/F09). |
| tidemark_dir_reclaimable_bytes | gauge; root,path; bytes | Mapping t_reclaimable/depth correct :417–430; deviates F10/F09. |
| tidemark_hot_bytes | gauge; mount,scope,key; byte delta | :298–345 top 10 per mount/scope, 300s; dir/unit/comm. Production availability depends on watcher seams excluded by user. |
| tidemark_phantom_bytes | gauge; mount; bytes | Implemented :346–353 from measured total, no fabricated zero when unavailable. |
| tidemark_open_writer_growth_bytes | gauge; mount,unit; byte delta | Implemented :355–376; aggregates measured known-unit writers over requested 300s then largest 10 pairs. |
| tidemark_scan_last_success_timestamp_seconds | gauge; root; Unix seconds | Implemented :378–385, complete scans only. |
| tidemark_scan_duration_seconds | gauge; root,mode; seconds | Implemented :386–390, duration_ms / 1000. |
| tidemark_scan_errors | gauge; root; errors | Mapping snapshot.meta.errors correct :419–422; missing/stale F10/F09. |
| tidemark_watch_events_total | counter; mount; event count | Implemented :394–400, view WatchStats.events_total. |
| tidemark_watch_overflows_total | counter; mount; overflow count | Implemented :401–406, view WatchStats.overflows_total. |
| tidemark_alert_firing | gauge; rule,subject; 1 for active | Implemented :408–415 from active_alerts. |
| tidemark_metrics_dropped_series | gauge; metric; omitted series count | :124–145 implemented for admitted cache/inventories. Itself capped to largest omission counts. Does not report cache rejections F09. |

## Other metrics service / packaged-asset requirements

| Requirement | Result / evidence |
| --- | --- |
| Textfile exact configured path, same-directory .tmp, fsync, chmod 0644, rename; don't create parent | Implemented `metrics.rs:436–459`; root:root follows daemon euid/default gid, privileged ownership test exists. `create_new` refuses symlink target. Fixed stale temporary causes F11. |
| Textfile write failure does not stop daemon, degraded status, at most hourly logs | Implemented :573–591; service test covers missing parent then reload recovery. It does not verify hourly log count. |
| Optional HTTP loopback-only, only GET /metrics, 404 else, proper exposition content type | Implemented :461–481; config validates numeric loopback too. No socket limits/cancellation F01. |
| Metrics interval / live reload | Textfile schedule and listener generations implemented :517–527, :557–562; blocked listener reload F01, oversized interval F12, cache depth increase/root removal F10. |
| `packaging/prometheus/tidemark-alerts.yml` | Seven required named alerts present. All referenced names exist. TimeToFull seconds<86400; ThinPool percent>85; Phantom bytes>1GiB; ScanStale age seconds>172800; WatcherLossy increase(overflows[15m])>0; Down mandated absent(up); firing==1. JSON is valid YAML subset. promtool unavailable. |
| `packaging/grafana/tidemark.json` | Nine panels cover all seven requested topics and only known metrics. JSON parser passes. Grafana import/schema validation not performed; uncounted display limitation: generic legends omit root/instance and watcher panel mixes rates with window counts. |


## Packaging requirements

Every listed RPM asset is explicitly `root:root` in `packaging/rpm/rpm.toml:10–19`. Manifest declarations are source evidence only; a real rpm query/install remains pending.

| Packaged file / service | Candidate implementation vs contract |
|---|---|
| `/usr/bin/tidemark` | Declared from target/release/tidemark, 0755 root:root; release container build and real ELF/RPM pending |
| `/usr/bin/tidemarkd` | Declared from target/release/tidemarkd, 0755 root:root; release container build and real ELF/RPM pending |
| `/usr/lib/systemd/system/tidemarkd.service` | 0644 root:root; byte-identical to candidate 08 §6, Type=notify, root, Restart=on-failure/5s, Nice=10, CPUQuota=200%, LimitNOFILE=65536, private state/runtime; known invalid memory-limit values present |
| `/usr/lib/tmpfiles.d/tidemark.conf` | 0644 root:root; exact `d /run/tidemark 0700 root root -`; no collector-directory creation |
| `/etc/tidemark/tidemark.toml` | 0600 root:root, config=noreplace declared, byte-identical to candidate example; HTTP/webhook disabled; `/` and `/data` scans/watch/sampler and textfile configured, activated only after deliberate service start |
| `/usr/share/doc/tidemark/README.md` | 0644 root:root, supplied from owned README-installed.md, describes stopped/disabled install and stage/reboot/verify |
| `/usr/share/doc/tidemark/07-live-monitoring.md` | 0644 root:root, source included with doc flag |
| `/usr/share/doc/tidemark/08-interfaces.md` | 0644 root:root, source included with doc flag |
| `/usr/share/doc/tidemark/11-rollout-runbook.md` | 0644 root:root, source included with doc flag |
| `/usr/share/bash-completion/completions/tidemark` | 0644 root:root; generated by release CLI before RPM command; completions bypass installed-config/privilege handling in cli lib |
| `%post` | Exactly tmpfiles --create and daemon-reload, errors ignored, no enable/start/account/group creation |
| `%preun` | Stop and disable only when `$1 == 0` (erase); upgrade leaves running daemon untouched |
| `%postun` | daemon-reload only; never deletes state |
| Build image | Containerfile FROM rockylinux:8, rustup 1.98.0, cargo-generate-rpm 0.20.0; operator bootstrap downloads explicit; xtask run uses cached image `--pull=never`, `cargo build --offline --locked --release`; no provisioning executed |
| Grafana dashboard | Nine panels covering all seven requested topics; all referenced metrics exist and byte/second/percent units align; watcher panel mixes events/s and 15-minute overflow counts under ops unit, legends weak (not counted as a separate finding); JSON parse is not import/schema-lint evidence |
| Prometheus rules | Seven required names and known metric names only, numeric thresholds use seconds/bytes/percent correctly; promtool unavailable |

Unit capabilities exactly match architecture §8: SYS_ADMIN (fanotify/bulkstat), DAC_READ_SEARCH (tree traversal), FOWNER (O_NOATIME), SYS_PTRACE (proc fd access). No mount-namespace-creating directive is present; RestrictNamespaces limits creation and does not itself create a mount namespace. All used setting names exist in upstream [systemd v239's parser registry](https://raw.githubusercontent.com/systemd/systemd/v239/src/core/load-fragment-gperf.gperf.m4); no unknown-directive issue identified from that comparison. Seccomp-backed hardening depends on the target build. Actual Rocky behavior, namespace identity, capabilities and cgroup values remain VM checks. Do not infer EL8 rejects `systemd-analyze security` simply from upstream version: downstream systemd 239 packages backported it (EL8 source history records this); check the guest's actual command.

Default safety: package installation is inert because scriptlets do not start/enable. The default configuration itself is active once started: it schedules root scans, watches both fleet mounts, samples/procscans, reports LVM, and emits a textfile if its parent exists. This is expressly what Wave 6 and rollout P2 specify, not a finding that defaults should silently be empty.

Additional monitoring limitation: `TidemarkDown` is the mandated `absent(tidemark_up)`; stale node_exporter textfiles survive daemon death (W6-B handoff already notes this). It also only returns a vector when **all** matching series are gone, not one missing fleet instance. Address via explicit contract decision and target/freshness detection, not a silent undocumented metric. Do not claim per-host down coverage from this rule.


## Acceptance, gate and build evidence

All commands ran against `cd7898edff0322e823b3e6d3e94be3f2e746f90c`, offline with Rust 1.98.0. Used `PATH=/home/brewerm/.cargo/bin:$PATH`, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_HOME=/tmp/tidemark-edge-cargo` (private copy of the populated host registry), `CARGO_NET_OFFLINE=true`, and `RUST_TEST_THREADS=2`. Every long command had `timeout 1500`; finite compiled probes had an additional `timeout 15`. Logs and sources are in this attempt's worker-output `library/`.

| Actual command/check | Result | Evidence and limit |
|---|---|---|
| `timeout 1500 tools/ci.sh` | **FAIL**, exit 1 | `ci.log`: source policy/140 owner tags, format and workspace Clippy succeeded; workspace tests stopped at w6a1 with totals 491 passed, 5 failed, 13 scale ignores. |
| `timeout 1500 cargo build --offline --workspace --features privileged --tests` | **PASS**, exit 0 | `privileged-build.log`; compiled only, no privileged execution. |
| `RUSTDOCFLAGS='-D warnings' timeout 1500 cargo doc --offline --workspace --no-deps` | **PASS**, exit 0 | `doc.log`; warning-clean docs. |
| `timeout 1500 cargo test --offline -p tidemarkd -p tidemark-cli --test w6a3 -- --nocapture` | Harness exit 0; **NEEDS-HUMAN** socket acceptance | `w6a3.log`: 14 harness passes (CLI 6, daemon 8); **9 EPERM early returns, 5 exercised tests**. |
| `timeout 1500 cargo test --offline -p tidemarkd --test w6b -- --nocapture` | **PASS**, exit 0 | `w6b.log`: 6 passed, including actual loopback HTTP routing, 10,000-directory cardinality, atomic publication and reload. This sandbox permits TCP but refuses AF_UNIX socket creation. |
| `timeout 1500 cargo test --offline -p xtask --test w6c -- --nocapture` | **PASS**, exit 0 | `w6c.log`: 4 source/guard tests passed; no package generation. |
| Exact-library overflow probe | **Reproduced F12**, probe exit 0 | `metrics-overflow.rs`/`.log`: accepted config, exporter panic, assertion of failed join. Probe disables textfile/HTTP. |
| Exact-library 64-idle-client probe | **Reproduced F01 resource growth**, exit 0 | `metrics-idle.rs`/`.log`: threads 6 → 19 → 51; all test sockets closed. |
| Static unit/config equality, shell syntax, asset JSON/reference checks | **PASS** | Reviewed sources, four passing w6c tests, metrics asset test, parallel review notes in `library/`. |
| `timeout 1500 cargo clean --offline` | **PASS**, exit 0 | `clean.log`: removed 26,034 files / 14.8 GiB before submission. |

After validation, restored the original attempt branch at `d33369c3ade14757e4b6c31625bdbbcf2fede260`; only this report is a repository change.

The five w6a1 failures are `binary_ready_and_stopping`, `corrupt_catalog_is_preserved_before_recovery`, `lifecycle_reload_and_shutdown`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`. They wait for readiness that cannot follow the sandbox's refused API socket; a separate Python AF_UNIX creation probe also raised EPERM. This reproduces the candidate handoff's environment blocker, not evidence that production bind should be ignored. CI stops before subsequent daemon/xtask suites and cargo-deny; the focused commands above fill only the scoped suites. The daemon-core w6a2 runtime suite was not separately executed. No green whole-workspace or advisory/license claim is made.

| Acceptance/gate item in this scope | Status | Basis |
|---|---|---|
| Exactly documented API names, parameter/result adapters and uid-0 dispatch | **PASS** source/pure dispatch; **NEEDS-HUMAN** real socket | Twelve-case test exercised; kernel/listener tests early-returned. |
| Hostile-client framing/admission/time/shutdown bounds | **FAIL** | F01–F04; pair tests cover only the narrower listed cases. |
| CLI unavailable/fallback/direct/config-label behavior | **PASS** exercised control flow/source; **NEEDS-HUMAN** live integration | Missing endpoint/mount tests and source; fake server/direct-listener tests skipped. |
| Scan completion identity and completed-with-errors exit codes | **FAIL** | F05–F07; source traces and absent regression cases. |
| Human byte/control-safe display and terminal widths | **FAIL** overall | Escaping present; F08 width failures. Known now presentation issues excluded. |
| Every documented metric name/type/label/unit, no extra family | **PASS** mapping | Populated-source test in CI plus w6b; actual measurement availability not implied. |
| Largest-first bounded cardinality for valid wide trees/latest snapshots | **FAIL** | F09/F10; 10,000-row success is below failing ceiling. |
| Atomic 0644 textfile, degraded write failure, restart recovery | **PASS** basic write/degrade; **FAIL** recovery | w6b + source; F11; ownership on real XFS **NEEDS-HUMAN**. |
| Config-safe bounded exporter and live HTTP reload/shutdown | **FAIL** | F01/F12; disabled-HTTP shutdown test is insufficient. |
| promtool rule validation | **NEEDS-HUMAN** | Tool absent; JSON/YAML-subset parse and metric references pass. |
| Grafana import or genuine schema lint | **NEEDS-HUMAN** | Only JSON/field/reference checks ran. |
| Root-only package layout/unit/tmpfiles/config/scriptlets | **PASS** static | Ten declared files and four tests; real artifact ownership/modes **NEEDS-HUMAN**. |
| Installable RPM and effective memory cap | **FAIL** at candidate due to excluded known defects | Later PKG metadata/unit fixes absent; no new duplicate findings. |
| Install/upgrade/erase/reboot on Rocky 8.8 and 8.10 | **NEEDS-HUMAN**; test coverage **FAIL** | No VM; F13 preconditions must be strengthened. |
| Namespace/fanotify/capabilities/SELinux and systemd security output | **NEEDS-HUMAN** | Source matches intended capability set; no guest execution. |
| Daemon end-to-end on both guests | **NEEDS-HUMAN** | Privileged binary compiled; known producer/scan blockers remain. |
| Idle 1-hour RSS ≤150 MiB, post-scan within 20%, 24-hour mini-soak flat RSS/fds/continuous metrics | **NEEDS-HUMAN** | No measurements performed; F01/F10/F11 relevant. |
| Human gate and `wave-6-done` | **NEEDS-HUMAN / not ready** | Findings, known blockers and unexecuted runtime gates remain; no tag/approval made. |

## Test quality and limits

- API/client socketpair tests exercise framing, malformed and oversized lines, response envelopes, repeated calls, EINTR/deadline handling and connection poisoning without bind. They are useful evidence even in this sandbox. The handler timeout test proves a timeout response and detached lifetime, not completion of every worker (F03).
- Daemon w6a3's six bind-dependent transport scenarios return early on EPERM. Its operator test also needs a root caller. CLI's fake-server table and direct-no-connect tests likewise return early on EPERM. Harness “ok” for an early return is not exercised acceptance. The injected-uid dispatch test checks every method without a socket; production SO_PEERCRED coverage remains pending an unrestricted host.
- CLI fallback tests intentionally fail on a missing mount before scanning this host's `/proc`. They meaningfully prove one notice and bypass/fallback control flow, but not real phantom/writer collection. The obsolete W5 exit-4 assertions were replaced in the candidate. Fake-server tests check method names but not complete outgoing params, so end-to-end root_b64/canonicalization and unsupported-option forwarding need stronger assertions.
- API admission tests fill 32 slots and assert rejection, not eventual reclamation. No current test proves a frame deadline, authorization before parsing, slow-reader total deadline or race-safe socket path publication. Real operator scan coverage is both environment-limited and dependent on the excluded candidate ENOTDIR bug.
- Metrics populated-source tests fabricate all families and cannot prove producer availability. The 10,000-path cardinality/hostile-label test is meaningful below the cache ceiling; restart and >100,000-row cases are absent. Atomic-write tests use rename and a synchronized reader, but `reader.join() > 0` can be satisfied by a pre-update read alone; it is not proof that a read overlapped replacement. The HTTP route test does not skip bind failure; it fails. Service shutdown tests keep HTTP disabled, leaving F01 untested.
- Packaging's exact-text unit/config tests compare against the same candidate plan; the unit parser strips `#` comments, masking the already-known invalid systemd limit values. Script syntax, source metadata and guarded-entry tests cannot establish that cargo-generate-rpm accepts the metadata or what the installed RPM does. F13 identifies vacuous runtime assertions. JSON parsing/metric-name membership is neither Grafana schema validation nor a dashboard import.
- Polling/deadline tests use wall-clock waits; an overloaded host may fail them. The timeout seam makes the API callback test short/deterministic in its synchronization, but no statistical timing or production load guarantee follows. Scale tests intentionally ignored as `scale` were not run.

## For the VM run

These are **commands to run later on disposable Rocky 8.8 and 8.10 guests as root**, not commands executed by this review. Provide the offline Rust cache/toolchain, promtool, prepared cached EL8 build image, auditd/SELinux Enforcing, and a persistent disposable loop-XFS mount through the existing harness. Do not run them on production or this workstation. Record the exact tested commit: the candidate still contains excluded known blockers, so a later fixed commit must have separate results.

First run the source suites without silent socket skips; require the named test counts/nonzero execution, not merely exit 0:

```sh
git rev-parse HEAD
export CARGO_NET_OFFLINE=true
export RUST_TEST_THREADS=2
timeout 1500 cargo test --offline -p tidemark-cli --test w6a3 -- --nocapture
timeout 1500 cargo test --offline -p tidemark-cli client::tests -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6a3 -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --test w6b -- --nocapture
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6a3 privileged::daemon_end_to_end -- --exact --nocapture
timeout 1500 cargo test --offline -p tidemarkd --features privileged --test w6b vm_xfs_textfile_publication_root_owned_and_readable -- --exact --nocapture
```

The w6a3 operator scenario must actually complete status/scan/top/now/reload/cancel/SIGTERM and remove its socket within the asserted deadline. No “pending unrestricted host” message is runtime acceptance. The privileged end-to-end scenario must observe both tracked mounts, a separately launched writer's hot directory/unit, and thin-pool percentages; known pending producer fixes may prevent this. The XFS metric test must execute one test and assert root:root/0644/complete bytes. Reload/shutdown, one-hour idle RSS ≤150 MiB, post-scan RSS within 20%, and the 24-hour soak are separate required gates; do not infer them from feature compilation.

On an installed disposable guest after startup, this bounded idle-client probe checks F02. All clients are closed afterward. Candidate behavior is EOF/reset on the new request after 12 seconds; a corrected deadline implementation should reclaim slots and return a valid status response:

```sh
python3 - <<'PY'
import socket,time
path='/run/tidemark/api.sock'
clients=[]
try:
    for _ in range(32):
        s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
        s.settimeout(2); s.connect(path); clients.append(s)
        time.sleep(.05)
    time.sleep(12)
    s=socket.socket(socket.AF_UNIX,socket.SOCK_STREAM)
    s.settimeout(2); s.connect(path); clients.append(s)
    try:
        s.sendall(b'{"v":1,"id":"idle-probe","method":"status","params":{}}\n')
        print('status after idle deadline:',repr(s.recv(4096)))
    except OSError as e:
        print('status after idle deadline failed:',e)
finally:
    for s in clients: s.close()
PY
```

For F01, set the VM-only config's existing HTTP setting to `listen='127.0.0.1:9877'`, reload, then run the following **finite** probe. Before/after thread/fd counts must obey the eventual documented cap; candidate tiny_http permits growth with idle clients. This does not replace oversized-line/queued-request/slow-reader/shutdown regression tests requested in F01:

```sh
python3 - <<'PY'
import pathlib,socket,subprocess,time
pid=int(subprocess.check_output(['systemctl','show','--property=MainPID','--value','tidemarkd']))
p=pathlib.Path('/proc')/str(pid)
def sample(label):
    print(label,next(x for x in (p/'status').read_text().splitlines() if x.startswith('Threads:')),
          'fds',len(list((p/'fd').iterdir())))
s=[]
try:
    sample('before')
    for _ in range(64):
        try: s.append(socket.create_connection(('127.0.0.1',9877),timeout=2))
        except OSError as e: print('admission refused:',e); break
    time.sleep(3); sample('idle clients')
finally:
    for x in s: x.close()
time.sleep(7); sample('after close')
PY
```

For F05, a fake Unix listener with a filled backlog and no accepts must cause `timeout 20 tidemark -c <temporary-config> scan <temporary-root>` to exit with its own timeout error near 11 seconds, not outer exit 124. For a listener that accepts but never answers, `timeout 20 tidemark -c <temporary-config> status` should exit 1 with “daemon call timed out” near 11 seconds. Build those fixtures in the new w6a3 regression tests so paths, backlog admission and cleanup are deterministic. F06/F07 need fake A/B statuses with persisted clean/degraded outcomes; F04 needs a controlled temp-parent replacement seam, without chmod targets outside that fixture. These regression tests do not exist yet; the current named suite cannot establish their fixes.

For snapshot persistence F10, configure one disposable root with schedule off and successful scanning, wait one metrics interval, then:

```sh
grep '^tidemark_dir_used_bytes' /var/lib/node_exporter/textfile_collector/tidemark.prom
systemctl restart tidemarkd
sleep 65
grep '^tidemark_dir_used_bytes' /var/lib/node_exporter/textfile_collector/tidemark.prom
promtool check rules packaging/prometheus/tidemark-alerts.yml
promtool check metrics < /var/lib/node_exporter/textfile_collector/tidemark.prom
stat -c '%U:%G %a %n' /var/lib/node_exporter/textfile_collector/tidemark.prom
```

Both grep calls must retain latest-snapshot data without another scan (candidate loses it); adjust the 65-second wait if the interval differs from the default 60 seconds. Both promtool checks must exit 0; stat must show root:root 644. Use unprivileged fixture regressions for F09/F11/F12 rather than manipulating the live collector. Import the dashboard into a test Grafana with a real Prometheus datasource and at least two roots/hosts; check all nine panels, units and series identity. No local import is claimed.

Package build/install checks, after recording the current known generation failure or applying its separately reviewed fix:

```sh
timeout 1500 cargo xtask rpm --container
timeout 1500 cargo xtask rpm --container --version 0.1.1
rpm -qlp target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm
rpm -qplv target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm
rpm -qp --configfiles target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm
rpm -qp --scripts target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm
getenforce
systemd-analyze verify packaging/systemd/tidemarkd.service
export TIDEMARK_DEPLOY_TEST_VM=1
export TIDEMARK_INSTALL_MOUNT=/mnt/tidemark-install-xfs
export TIDEMARK_INSTALL_RPM="$PWD/target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm"
export TIDEMARK_INSTALL_UPGRADE_RPM="$PWD/target/generate-rpm/tidemark-0.1.1-1.el8.x86_64.rpm"
timeout 1500 bash tests/install/install.sh
```

The mount/artifacts must be preprovisioned in that guest. Expect exactly the ten files/modes/owners listed above, only the private config marked config(noreplace), no install start/enable/group creation, and Enforcing SELinux. The script must reach staging, retaining edited config and state and observing external-unit writes. Amend its fixture/preconditions for F13 before claiming disable/config-conflict coverage. A changed-default upgrade must produce `.rpmnew` while preserving the live edited config; identical defaults do not exercise that branch.

After the harness's ordinary reboot with the persistent fixture restored:

```sh
TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_INSTALL_PHASE=verify timeout 1500 bash tests/install/install.sh
systemctl show tidemarkd -p Type -p User -p Group -p MainPID -p MemoryLimit -p MemoryMax -p CPUQuotaPerSecUSec -p CapabilityBoundingSet -p NoNewPrivileges
pid=$(systemctl show tidemarkd -p MainPID --value)
readlink /proc/1/ns/mnt /proc/"$pid"/ns/mnt
stat -c '%a:%U:%G %n' /etc/tidemark/tidemark.toml /var/lib/tidemark /run/tidemark /run/tidemark/api.sock
systemd-analyze security tidemarkd
journalctl -u tidemarkd --no-pager
ausearch -m AVC -ts recent -c tidemarkd
```

Require successful post-reboot verification with a newer boot report, notify/root, the four planned capabilities, no unintended mount namespace, modes 600/700/700/600 root:root, no AVCs, and memory limits 2147483648 rather than infinity. Attach actual security/SELinux output; absence of unexpected-directive errors and runtime cgroup values are the EL8 evidence, not the exact-text unit test.

## What was not checked

No VM access, privileged test execution, RPM/image provisioning, actual package generation/install/upgrade/erase, service operation, mount/LVM command, reboot, one-hour RSS measurement, 24-hour soak, Grafana import or promtool run occurred here. No writes were made under host /etc, /var/lib, /run or /usr. No direct /proc collector workload was deliberately launched outside the requested unprivileged tests. The other reviewer's daemon internals and excluded LIVE-hot/WATCH-perf areas were not independently re-reviewed. F01 has a finite idle-client thread-growth reproduction and F12 an interval-panic reproduction against candidate libraries; the remaining threat scenarios are source-backed, not claimed live reproductions. Dependency advisory/license auditing remains unestablished. All findings are proposals for human triage, not applied fixes or an acceptance decision.
