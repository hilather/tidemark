**NOT READY** — gate checkout: **1 BLOCKER, 4 SHOULD-FIX, 1 NIT** new findings. The separately specified blind candidate has **2 BLOCKER, 14 SHOULD-FIX, 4 NIT**: 12 retained original findings, five shared new findings (G2–G6), and three candidate-specific gaps below. Its partial documentation resolutions are counted once under contract reconciliation. These are review recommendations and evidence, not acceptance or release decisions.

## Commits, scope and evidence

The supplied instructions identify two different trees. The gate task and submission contract supply checkout `f5aed2f10e4d7300941e85f944ca704135fa4736`; the blind review session identifies `c2240139763075c1e8f0b54748bfcee717e4bdaa`, relative to `74e16714453fd3992269de91d92fae70274c84af`, with scope `candidate_tree`. They are not interchangeable: 122 files differ. This report reviews the gate checkout and explicitly records the earlier candidate's differences. No later fix is credited to the blind candidate. The latter was exported unchanged under ignored `target/review-candidate` for independent builds. No branch creation, merge, push, VM contact or privileged test execution occurred.

Read AGENTS, both whole wave files, their relevant Read-first sections, the five original reviews and task/fix handoffs, associated SCRs, contract decisions, ADR-018/019, playbook §§5/7, and Wave 6 interfaces. VM man pages, actual EL8 report captures and kernel behavior were unavailable. Historical PostgreSQL implementation was not reviewed. Current production removal was checked.

Evidence directory: `/home/brewerm/.herdr-projects/tidemark/.state/worker-output/attempt-3d59888bbbcc49ee54255bc9270a620ec225a5194d6612a50557c3c268d6518e/library/`. It contains required-check transcripts, supplemental test logs, standalone probe sources/logs and component review notes. Line references are for `f5aed2f` unless explicitly labeled `c224`. Only this report is a repository change.

## Original findings, independently rechecked

“RESOLVED” here means the original code/test defect is addressed, not that VM acceptance passed. The candidate column uses its own source; “documentation pending” means the implementation exists but that tree's plan was not reconciled. Test names below identify inspected regressions; execution and restrictions are reported later.

| Original finding | Gate checkout status and evidence | Blind candidate `c224` |
|---|---|---|
| W4-A F1, mandatory privileged scenarios (`w4a-privileged-scenarios`) | RESOLVED in fixtures. `fs/tests/w4a/lvm.rs:132` checks linear/thin identities and size/free oracles; `cli/tests/w4a.rs:208,267` compares thin percentages, fires the actual alert engine, and renders non-LVM `n/a`. Runtime pending VM. | NOT RESOLVED: linear-only mapping, thin-increase-only assertion, absent-binary substitute for non-LVM fixture. |
| W4-A F2, renderer exports (`w4a-render-visibility`) | RESOLVED. `cli/src/render/lvm.rs:9,23,38,51` and fields are `pub(crate)`; callers compile. | NOT RESOLVED: public exports remain. |
| W4-B R1, ordinary unstage symlink deletion | RESOLVED for original case. `deploy/mod.rs:662,669,684` guards before service changes, retains a no-follow deploy handle, then unlinks relative to it. `state_symlinks_refused_before_service_for_all_operations` protects outside bytes. | NOT RESOLVED: `mod.rs:651–652` removes by pathname through `deploy`. |
| W4-B R2, state `/` chmod | RESOLVED for original root/lexical cases. `files.rs:53` rejects root, parents, protected/tracked containment and mounts; `firstboot.rs:108` uses it. `unsafe_state_values_never_mutate`, `state_cannot_contain_tracked_root` and daemon unsafe-state regression. G1 below remains. | NOT RESOLVED: stage and firstboot can reach unconditional directory chmod. No destructive root probe was run. |
| W4-B R3, identical restage ignores start/enable | RESOLVED. `mod.rs:316–322`; `identical_restage_honors_start_and_repairs_enablement` plus original no-op case. | NOT RESOLVED: shortcut ignores options and service state. |
| W4-B R4, global config destination/replacement | RESOLVED. `cli/src/lib.rs:105`, `cmd/deploy/stage.rs:24`; `custom_destination_and_replacement_use_source_with_fake_environment` and executable invalid-installed-config regression. | NOT RESOLVED: hardcoded default destination and installed-config dispatch gate remain. |
| W4-B R5, installed symlink copied into backup | RESOLVED for static destination case. `files.rs:167` uses component handles, `O_NOFOLLOW`, nonblocking regular-file validation. `installed_symlink_blocks_identical_and_differing_config`. G2 is a separate parsing race. | NOT RESOLVED: ordinary `File::open` for installed bytes. |
| W4-B R6, public implementation constants | RESOLVED. `deploy/mod.rs:16`, `client.rs:14` private; compiled callers. | NOT RESOLVED: both public. |
| W5-A 1, descriptor shortage kills watcher (`w5a-fd-exhaustion`) | RESOLVED in code. `watch/mod.rs:377–462` retains group, retries EMFILE/ENFILE/ENOMEM/ENOBUFS with shutdown-aware 50–800 ms waits, persists fatal `Failed`. Injected transport test processes a subsequent fixture fd and verifies recovery/failure. | Same code and regressions; resolved in code. |
| W5-A 2, overflow leaves stale paths (`w5a-overflow-cache`) | RESOLVED for cache defect. `watch/mod.rs:439` clears handle/path caches on all loss paths. Synthetic processor regression moves a directory across an exclusion boundary and verifies fresh resolution. G3 is separate. | Same, resolved. |
| W5-A 3, clock rollback (`w5a-clock-rollback`) | RESOLVED. `watch/rings.rs:33–70` resets affected history, marks loss and excludes future buckets. `sixty_buckets_and_clock_rollback` checks new additions and drain-once. | Same, resolved. |
| W5-A 4, foreground state exclusion (`w5a-state-exclusion`) | RESOLVED. `cli/src/cmd/watch.rs:38–48,137` adds canonical state path. `mount_realpath_and_state_exclusion` and non-self-PID processor fixture distinguish excluded prefix and sibling. | Same, resolved. |
| W5-A 5, foreground event stream (`w5a-foreground-stream`) | RESOLVED. `watch/mod.rs:302,564` opt-in 1,000-event oldest-drop buffer; CLI `watch.rs:74–183` shared 50/s limiter. Processor and `limiter_and_complete_event_envelopes` regressions cover drain, drops and omissions. | Code resolved; old plan lacks the new event interface. |
| W5-A 6, exact rename pairing (`w5a-rename-contract`) | RESOLVED by R2-8: independent directional events; no guessed pair or FAN_RENAME. Processor tests source/destination signed deltas. | PARTLY RESOLVED: fix handoff records accepted directional-only decision, but candidate plan/SCR still require pairs/pending decision. Later R2-8 text is absent. |
| W5-A 7, mount realpath (`w5a-mount-canonicalization`) | RESOLVED. CLI `watch.rs:38`; test covers relative path, alias and symlink-plus-parent before byte normalization. | Same, resolved. |
| W5-A 8, JSON envelope (`w5a-json-envelope`) | RESOLVED. CLI `watch.rs:188–219`; `tidemark.watch.v1`, complete attribution, exclusive path/path_b64 and newline tested. | Same, resolved. |
| W5-A 9, ineffective overflow/load fixtures (`w5a-runtime-test-coverage`) | RESOLVED in fixture implementation. `watch/mod.rs:881` pauses the real worker and requires recovery; fallback selection test; privileged load fixture paces 2,000/s, measures CPU and checks ongoing processing/fds. Runtime unrun; forced-fd raw fixture does not guarantee the separately negotiated watcher itself chooses fd mode. | Same fixtures; runtime pending. |
| W5-B 1, admission starvation (`w5b-writer-admission`) | RESOLVED. `procscan.rs:225–268` bounded rotating admission; `admission_reaches_initially_excluded_grower`. Independent eight-pass/20,000-fd probe admits inode 19,999. | NOT RESOLVED: same probe never admits it. |
| W5-B 2, covered mount device (`w5b-visible-mount`) | RESOLVED. CLI `phantom.rs:80,125` shared `mount_for_path`/exact-target lookup; `shared_setup_selects_visible_overmount`. Both commands use it. | NOT RESOLVED: first matching mountinfo row wins. |
| W5-B 3, wrong measurement duration (`w5b-window-duration`) | RESOLVED. `procscan.rs:362–377` actual baseline span and nonpositive-span exclusion. Short/irregular/repeated-second tests and independent 10-second-history probe. | NOT RESOLVED: reports requested 300 seconds for 10 seconds of history. |
| W5-B 4, bypassed realpath (`w5b-mount-realpath`) | RESOLVED. CLI `phantom.rs:119`; shared alias/relative/symlink-parent regression. | NOT RESOLVED: RawPath constructed before canonicalization. |
| W5-D missing pool data resolves (`w5d-unavailable-pool`) | RESOLVED. `alerts/rules.rs:161–176`, engine unknown branch preserves state/timers. `missing_pool_percentages_never_resolve` tests each missing percentage, both, NaN, unrelated row and later confirmed absence. | Same code/tests, resolved. |
| W5-D healthy subjects consume cap (`w5d-subject-cap`) | RESOLVED. `alerts/mod.rs:140` rejects unseen false/unknown without state; capacity diagnostics/healthy eviction bounded. `healthy_subjects_do_not_hide_breach`, `capacity_is_reported`, duplicate/eviction and unavailable-clearing tests. | Same, resolved. |
| W5-D unsupported windows/auto stale (`w5d-unsupported-inputs`) | RESOLVED in engine. Exact Duration source calls, ScanAge cadence and explicit diagnostics; `arbitrary_windows_and_auto_cadence`, all-nine known/unknown tests, now diagnostic test. Watcher-to-source mismatch remains G4. | Engine code resolved; candidate plan still describes old inputs. |
| W5-D reused hot histories (`w5d-hot-windows`) | RESOLVED in engine. `hot_dirs(mount, window)` per rule; `hot_windows_use_distinct_histories` excludes old growth from 5m while retaining it in 1h. | Same code/test, resolved. |
| W5-D clearing renotify (`w5d-clearing-renotify`) | RESOLVED. `alerts/mod.rs:341–378`; `clearing_false`, `resolution_precedes_renotify`. Unknown remains silent. | Same, resolved. |
| W5-D redirects counted delivered (`w5d-webhook-redirect`) | RESOLVED. `alerts/deliver.rs:81–100`, no redirect/proxy, only 2xx succeeds. 302/503 tests require four original-URL attempts and failed counter. | Same, resolved. |
| W5-D display collisions (`w5d-display-bytes`) | RESOLVED. `now.rs:314` and rule subjects use shared byte escaping. `byte_names_and_escaped_paths_are_distinct`, `raw_subject_bytes_preserve_identity`. | Same, resolved. |
| W5-D extra public cap/metric method (`w5d-public-surface`) | RESOLVED. Cap private, metric method removed; R2-10 documents new source/diagnostic surface. | Original exports removed; new interface documentation reconciliation still pending in this tree. |

## New gate findings

### G1 — BLOCKER: unsafe existing state/deploy directory permissions are accepted

`finding:w45-state-directory-permissions` — `crates/tidemarkd/src/firstboot.rs:108`, `crates/tidemark-cli/src/deploy/mod.rs:622,662`, `crates/tidemark-cli/src/deploy/files.rs:39`, `crates/tidemark-cli/src/deploy/real.rs:87`.

R2-7 requires existing unmarked production state directories already to be 0700 root:root, and ADR-018 requires private state directories. Only stage's RealEnv checks the state root. Firstboot and ordinary unstage do not enforce its mode/owner; existing deploy directories are reused unchecked.

The public-function probe, linked against actual libraries, observed `firstboot::write` return Ok in an unmarked 0777 directory, preserve 0777 and create the report. Ordinary unstage in an unmarked 0777 directory returned Ok, removed the record and called the fake service manager. Stage with a private state root but preexisting 0777 deploy directory preserved that mode and enabled the fake service. Environmental probes were faked only in the stage case; production RealEnv separately checks only the root. A writable accepted report parent lets its writer replace a 0600 report regardless of that file's permissions. No arbitrary external-file write is claimed.

**Change:** share descriptor-based state/deploy eligibility validation across all three entry points; refuse unsafe unmarked directories before files or services change, and retain validated handles. Only normalize new or valid marker-authorized directories; preserve unprivileged test support without adding a uid gate. Test owner, group/world-write, no-service/no-report and unchanged outside-content cases. **Files:** `deploy/files.rs`, `deploy/mod.rs`, `deploy/real.rs`, `deploy/tests.rs`, `tidemarkd/src/firstboot.rs` and its tests, CLI `tests/w4b/privileged.rs` for real ownership. No public type change is necessary. Evidence: `deploy-gate-probe.rs`, `deploy-gate-probe.log`. This precise permission-preservation defect is at the later gate checkout; the blind candidate has the older unconditional-chmod blocker instead.

### G2 — SHOULD-FIX: safe config inspection is followed by an unsafe pathname reopen

`finding:w45-config-parse-reopen` — `crates/tidemark-cli/src/deploy/mod.rs:121–127`, `deploy/files.rs:153–155`, `deploy/purge.rs:114–116`; `crates/tidemark-core/src/config.rs:687`.

Source staging reads safely, calls `Config::load(path)`, then compares a second safe read. Marker-root and purge validation similarly inspect then call Config::load. That loader uses blocking `File::open`, following symlinks. A replacement in this interval can follow a link or block opening a FIFO. An A→B→A replacement can parse configuration B but install/hash bytes A, passing the before/after comparison. The inspected call sequence establishes the race; no successful concurrent exploit or resulting outside write was demonstrated.

**Change:** parse and merge defaults from the exact bytes of one bounded, no-follow regular-file read; reuse that path for source, marker and purge config. Seek the required core SCR if adding a parser API. **Files:** `tidemark-core/src/config.rs`/tests, `deploy/mod.rs`, `deploy/files.rs`, `deploy/purge.rs`, `deploy/tests.rs`, firstboot regression (which shares files.rs). Add controlled replacement tests asserting parsed and installed bytes cannot differ and unsafe replacements cannot hang. Also present in the earlier candidate; original installed-backup R5 resolution does not cover this reopen.

### G3 — SHOULD-FIX: overflow loses dirty hints for nested scan roots

`finding:w45-nested-dirty-loss` — `crates/tidemark-live/src/watch/mod.rs:317–328,439–445`.

Loss replaces the dirty set with the watched mount root. `take_dirty(under)` returns only descendants of `under`. For watched `/data` and scan root `/data/reports`, an existing dirty `/data/reports` becomes `/data` after overflow; querying `/data/reports` returns empty. The independent regression appended to unchanged watcher modules fails with exactly this input. Source is identical in both trees.

**Change:** retain bounded coarse loss coverage/generations so every affected configured subroot remains invalidated until its own successful scan. One sibling's scan must not clear another's coverage. **Files:** `watch/mod.rs` and its tests; if a new acknowledgment/root registry is chosen, also Wave 6 watcher service and the declared watcher contract. Test nested/sibling roots, repeated loss and completion order. Evidence: `watch-probe/build.log` (0), `watch-probe/run.log` (expected assertion failure, 101). No current daemon caller exists yet.

### G4 — SHOULD-FIX: watcher output cannot provide the exact alert windows promised to Wave 6

`finding:w45-exact-watch-windows` — `crates/tidemark-live/src/alerts/mod.rs:18–20`, `watch/mod.rs:290,347`, `watch/rings.rs:124`; `docs/plan/13-contract-decisions.md:652`.

AlertSource requires complete inventories and exact arbitrary Duration windows. Watcher offers integer 1–60-minute aggregates, cumulative overflow totals and a 15-minute lossy-minute count. Completed buckets discard all but 1,000 keys per scope; drains preserve neither event timestamps nor per-minute overflow counts.

A compiled probe validates a 90-second rule and places the same 100-byte event/overflow at t=1 versus t=59. At t=120, all public minute-window aggregates, loss counts and completed drains are identical, but exact 90-second totals must be 0 versus 100 bytes and 0 versus 1 overflow. An adapter cannot recover discarded information. Current W6 stubs return None safely; no present false-firing claim is made.

**Change:** settle an SCR for bounded timestamped query/count data with explicit completeness and retention limits, or explicitly constrain/qualify accepted windows. Return unavailable plus diagnostic when completeness cannot be established; never silently round, clamp or call truncated top lists complete. **Files:** watcher `mod.rs`, `rings.rs`/tests; daemon `state.rs`, `services/watcher.rs` and W6 tests; plan `07`, `13`, Waves 5/6 and the SCR. A method-based seam need not change core payload constructors. Evidence: `watch-window-probe.rs`/`.log` (0). Both code trees have this gap; the earlier plan had not yet incorporated R2-10.

### G5 — SHOULD-FIX: an interrupted client I/O poisons a valid connection

`finding:w45-client-eintr` — `crates/tidemark-cli/src/client.rs:84,97`.

The initial CI run failed `deadline_and_poisoned_connection` with `Io(Interrupted)`. An independent socketpair probe installed a harmless SIGUSR1 handler, waited until the client blocked in `unix_stream_read_generic`, then signaled it. A call with a two-second deadline failed after about 100 ms and permanently poisoned the connection; the peer successfully sent its valid matching response at 700 ms. A handled signal is not a framing/protocol failure. Both commits have the same I/O logic.

**Change:** retry `ErrorKind::Interrupted` around write/fill_buf within the existing monotonic total deadline, without poisoning for the recovered interruption. Add a signal/injected-I/O regression that receives the valid response, keeps the next call usable, and still bounds the overall timeout. **Files:** `client.rs` and its tests only; no public signature changes. Evidence: `client-eintr-probe.rs`/`.log`, `gate-ci.log`.

### G6 — NIT: the operator example still promises change-free runtime failure

`finding:w45-stage-guarantee-doc` — `docs/plan/08-interfaces.md:309` says “non-zero = nothing changed.” R2-7 and the detailed section correctly promise change-free blocking preflight, then best-effort compensation with possible downtime/metadata/empty-directory residuals. **Change/file:** update this example in `08-interfaces.md` to the same distinction. The older candidate also has the misleading promise.

## Additional differences in the blind candidate

The 12 NOT RESOLVED original rows above remain findings against `c224`, with the exact changes recommended in their original reviews. Receipt references for W4-B R1–R6 are respectively `finding:w45-candidate-unstage-symlink`, `finding:w45-candidate-state-root`, `finding:w45-candidate-restage`, `finding:w45-candidate-config-destination`, `finding:w45-candidate-installed-symlink` and `finding:w45-candidate-public-constants`; other original rows retain their displayed references. Fix file sets are: W4-A `fs/tests/w4a/lvm.rs`, `cli/tests/w4a.rs`, `cli/src/render/lvm.rs`; W4-B deploy `mod.rs`, `files.rs` (new helper), `real.rs`, `tests.rs`, CLI `lib.rs`/`Context` and every Context constructor (`cli/tests/w5b/commands.rs` included), `cmd/deploy/{stage,unstage}.rs`, `client.rs`, `tidemarkd/src/firstboot.rs`, CLI W4-B tests; W5-B `live/src/procscan.rs`, CLI `cmd/phantom.rs` and its shared writers caller/tests. These later fixes were inspected but are not in that candidate.

- **SHOULD-FIX, `finding:w45-candidate-pid-identity`:** `c224:crates/tidemark-live/src/procscan.rs:158` attributes observations without matching enumeration starttime; its `fs/src/procfs.rs::OpenFile` lacks that field. Replacement process comm/unit can be assigned to the old inode. The later regression and independent algorithm comparison fail against candidate logic and pass at the checkout. Fix requires `fs/src/procfs.rs`, `live/src/procscan.rs`, `fs/tests/w1d/main.rs`, `live/tests/w5b/unit.rs` (all OpenFile constructors), plus the R2-9 contract. The comparison harness links the later compatible fs type to the older scanner; it proves aggregation ignores supplied identity, not a real kernel PID-reuse stress run.
- **SHOULD-FIX, `finding:w45-candidate-contract-reconciliation`:** `c224:docs/plan/waves/wave-5-live.md` still specifies fixed AlertInputs/NowInputs and lacks the new foreground stream/failed state; `13` lacks R2-7 through R2-10. W5-A/D fix notes explicitly call for interface reconciliation. The candidate cannot give W6 a consistent plan/code contract. Reconcile `07`, `08`, `13`, Waves 5/6 and SCR dispositions to the implemented/approved source, including AlertSource, ScanAge, diagnostics, optional now sections, two-argument webhook construction, and directional renames. This is resolved in the later checkout apart from G4 and the open items below; it is not evidence of unauthorized intent by the implementation.
- **NIT, `finding:w45-candidate-pg-annotation`:** `c224:crates/tidemark-scan/tests/w2a/main.rs:366` retains the generic annotation label `pg.db`. Replace that fixture label/assertion with a generic label in this file. Already corrected at the checkout; no PostgreSQL collector behavior is implicated or reviewed.

## Cross-task safety and contract assessment

LVM has one production spawn site, selecting absolute `/usr/sbin/lvs` and `/usr/sbin/vgs`, fixed report arguments, LC_ALL=C and suppressed fd warnings. No shell, `--readonly` or mutation command. Each output stream is capped at 16 MiB, parsing at 100,000 rows; nonblocking bounded polling checks a deadline, kills/reaps failures. Timeout is per child, so total collection can approach twice the configured value. String-valued JSON, inactive `-1`, empty optional fields, finite percentages, bracket-preserving matching, major/minor mapping, pool percentages, deterministic ordering and injected sampling time were inspected/tested. Fixtures are explicitly synthetic, not actual EL8 captures. CLI history queries are read-only, limited to 24 hours, and failures warn without discarding current data. Probe collects once and treats failure as advisory.

Deployment static symlink checks, relative no-follow operations, O_NOATIME/EPERM fallback, regular-file validation, exclusive temporary files, descriptor ownership, renameat/unlinkat and fsync are present. Stage retains state/deploy handles through service actions and compensation; unstage opens deploy before stopping. Purge streams entries with a 256-frame stack, checks mount IDs including bind mounts, and verifies stopped service/root identity. G1/G2 prevent a complete confinement claim. Concurrent privileged mount/rename changes after identity checks were not proven safe by a race stress test. Initial directory preparation occurs before the compensation closure; allowed empty-directory/metadata residuals can remain without its full error summary.

Blocking preflight tests assert original config, absent new state and no service mutations. Later-failure tests restore config/marker/staged bytes, remove new backups and independently attempt prior enabled/active state, preserving compensation errors. This is not a transaction. Systemctl uses fixed `/usr/bin/systemctl` operations, 10-second query/120-second action bounds and child reaping. Verify can overshoot its requested wait by an in-progress query/API call timeout. No uid gate or group-access check was found; firstboot's effective-uid branch selects file ownership, not authorization. No `doctor` command/interface is defined in this tree's frozen W4-B/CLI contract, so the task label alone is not treated as a missing implementation requirement.

Watcher uses notification-only nonblocking fanotify, never permission events. It reads the group fd, not event-file contents; event OwnedFds close on inspected success/error/ignored paths. Handle resolution uses O_PATH, named metadata no-follow flags. Unsafe blocks stay in sys with SAFETY comments. Bounds: 64 KiB read batch; 128-byte handles; 1,000 foreground events; 200,000 dirty entries; configured inode/path caches; 100,000 handle paths; 60 buckets with three current maps capped at 20,000, completed top 1,000, and a 200,000 identity set. Failed/recovering transport state and rollback loss are visible. Removed-mount kernel behavior, a hard one-second join under stalled metadata I/O, actual fd exhaustion and kernel FID resolution remain unverified.

Procfs has 1 MiB text, 1,048,576 ID/open-file bounds and returns cap errors rather than a partial result. It validates descriptor identity around readlink/fdinfo and process starttime around enumeration. The checkout compares later attribution starttime; the candidate does not. Writer history is 10,000×60, but full observation/phantom vectors can reach the much larger source cap and duplicate path storage: the plan's approximately 15 MB is not a whole-scanner memory guarantee. Hostile comm parsing, octal flags, byte paths and deterministic tie sorting were inspected. Existing disappearing-process tests exercise 20 children/five passes; no real PID-reuse stress or injected hostile procfs filesystem was run. R2-9 explicitly exempts procfs from scan Gate/O_NOATIME. Its required magic-link metadata dereference does not read monitored regular-file contents.

Alert evaluation uses injected time, private bounded state and deterministic subjects. Unknown input preserves state/value/timers, pauses continuity and never fires, resolves or renotifies; later known input restarts pending/clearing evidence. Confirmed empty differs from unavailable. The twelve named state/input cells plus boundaries, flaps, zero durations, backwards time, capacity and threshold tests provide an equivalent branch argument; no measured llvm-cov percentage is claimed. Webhook enqueue never performs network I/O; one worker has a 1,000-event oldest-drop queue, five-second requests and an initial attempt plus three retries with 1/2/4-second delays. Redirects are disabled and fail; no shell exists. Drop joins an in-flight request; daemon shutdown must budget for it. Journald delivery is a tracing event; actual subscriber integration remains W6. Now's missing phantom placeholder is accompanied by degradation and rendered “not measured.”

The shared `ComponentState` and `VgInfo` are reexports, not duplicated payloads. Units are bytes, seconds and explicitly labeled percentage-points/day. Scanned-file contents, metadata mutation, filesystem-depth recursion, async runtimes and production unsafe outside sys were not introduced in these W4/W5 paths. The reviewed reporting/event paths add no disk scan syscall bypassing Gate. Current production crates/manifests/packaging have no PostgreSQL tracking, pgbench, socket_group or tidemark-group creation; rejection tests and historical/deferred documents are intentional, not active features.

## Wave 6 readiness

The current checkout contains the documented callable library functions: LVM parse/collect/map/lvm_now/to_samples; Client connect/call; firstboot needs_report/build/write; Watcher start/mode/hot/events/dirty/stats/drain/stop; ProcScanner new/scan_once/phantom/writers/take_samples; AlertEngine new/evaluate/active/diagnostics and Deliver/WebhookDeliver; build_now/render_now_text. Both shared core payload reexports are correct. The following limitations prevent an unqualified handoff:

1. G1/G2 must be fixed before the daemon uses firstboot/state helpers; G5 affects every synchronous API client consumer.
2. G4 requires a decided data/availability contract. Cumulative overflow counts, truncated minute rows, and a 1–60-minute query cannot implement exact arbitrary rule windows. None/diagnostics is the safe temporary behavior.
3. `core/src/api.rs:106` LvmResult contains mount LvmNow values, VGs and timestamp, but lacks segment type, other LVs and 24-hour trends required by the direct renderer (`cli/src/render/lvm.rs:59–105`). Decide reduced daemon presentation, extra direct reads, or an SCR. Do not silently expand the wire payload.
4. `ProcScanResult` adds `writers_full`/`writer_evictions` in source but the Wave 5 code block lists only at/fds_seen/duration. Document the diagnostic fields; no current constructor mismatch was found. Permission-error counts remain unrepresented by both source return types and the published scanner interface, as the original review noted. Decide their contract rather than invent a field locally.
5. Watcher stop consumes self while LiveState stores Arc handles. W6 must release view/state clones before explicit stop or final Drop; retain no hidden owner that defeats shutdown. Add state_dir to both watcher and procscan exclusions, leave event buffering disabled, drain completed minutes/samples once per pass and expose Failed/recovering/stale collectors truthfully. G3 must not let a nested-root scan clear siblings' invalidation.
6. `tidemarkd/src/state.rs` and service files are W6-prep stubs, not implemented daemon behavior: AlertSource returns None and service start returns NotYetImplemented. This is expected future W6 work, not a W4/W5 stub-consumer regression. Startup report timing, effective cadence ≥24h for windowed roots, reload/persistence, metric gauge rendering from active(), subscriber delivery and orderly shutdown still require implementation/integration tests. The older candidate additionally lacks the reconciled interfaces and fixes enumerated above.

## Open requests and unfinished evidence

| Request/follow-up | Disposition at gate checkout |
|---|---|
| W4-A SCR directory | None exists. Real captures, root/data/thin oracles and <2-second measurement remain pending VM; direct default selection by roots versus distinct sampler mounts remains a specification ambiguity. |
| scr-W4-B/SCR-001 | Closed by R2-7; Context destination and independent source dispatch implemented. All inspected Context constructors updated. |
| scr-W4-B/SCR-002 | Closed by R2-7; best-effort compensation explicitly accepted. G6 contradicts it in one example. |
| scr-W4-B/SCR-003 | Closed by R2-7; exact local-filesystem allowlist implemented, unknown types block. |
| scr-W5-A/SCR-001 and SCR-002 | Closed by R2-8; bounded foreground stream and directional-only renames implemented. |
| scr-W5-B/SCR-001 | Closed by R2-9; starttime accepted, procfs Gate proposal rejected. Actual time span implemented. Permission diagnostics remain a separate unresolved dependency question. |
| scr-W5-D/SCR-1 | Closed by R2-10; source/availability accepted, no separate v1 webhook redaction. G4 is a remaining producer/consumer mismatch. |
| Deferred W5-C requests | Closed without a decision under ADR-019; no removed implementation reviewed. |
| Original Needs-from-coordinator lines | Above contract requests answered by `13`, superseding stale Pending labels. VM access/captures/performance, mounted-state tests, daemon subscriber/report/metrics wiring remain unfulfilled. |
| W4-B-fix / W5-B-fix / CI-fix-context build dependencies | Shared Context and WalkConfig constructors updated; full current workspace tests pass on the supplemental run, including the historical cancellation/publication and dir-series failures. Initial client EINTR failure is separately retained as G5. |
| CODE-drop-postgres follow-ups | Active removal and plan gate changes present; generic annotation leftover fixed only at later checkout. |
| Unreferenced TODOs | No TODO/FIXME/todo!/unimplemented! in inspected W4/W5 production paths. Legacy NotYetImplemented error variants are not returned by implemented components; W6 stubs retain explicit owners. |
| Known remaining gaps | top_unit is always None (optional but visible limitation); no dedicated real 20,000-fd benchmark; no measured now <50 ms/whole-process RSS/terminal-column width; no real persisted alert restart/live-disable or journal/metrics integration evidence. |

## Gate checklist and task acceptance

| Checklist item | Status | Evidence |
|---|---|---|
| Wave 4: lvm on root/data XFS/LVM plus loop thin pool | NEEDS-HUMAN | Fixtures/oracles implemented at checkout; no VM run or captured real JSON. |
| Wave 4: stage clean refusal on each injected failure and successful stage | FAIL | G1/G2 prevent confinement sign-off; fake state-machine coverage exists, real scenario matrix unrun. |
| Wave 4: tag wave-4-done | NEEDS-HUMAN | Not created; fixes and human approval required first. |
| Wave 5: privileged suite green on both VMs; capability report | NEEDS-HUMAN | Compilation only; no runtime claim or W5-gate capability evidence. |
| Wave 5: foreground watch attributes two systemd units | NEEDS-HUMAN | Stream/envelope/limiter tested synthetically; real unit attribution unrun. |
| Wave 5: tag wave-5-done | NEEDS-HUMAN | Not created; G3/G4 and runtime evidence outstanding. |
| W4-A: CLI matches lvs/vgs; collect <2 s | NEEDS-HUMAN | Correct fixtures now exist; synthetic parsing does not establish this. |
| W4-B: unit tests; privileged preflight/unstage on both VMs | NEEDS-HUMAN | Focused deployment/firstboot tests pass, but new probes fail required safety expectations and VM half is unrun. |
| W4-B: 08 §5 checks/exit codes match | FAIL | G1/G2, G6; static refusal 5, unhealthy 6 and absent daemon 4 paths otherwise checked. |
| W5-A: fd/FID behavior on both VMs | NEEDS-HUMAN | Probe/fallback tests compile; record actual selected modes and conditional FID coverage. |
| W5-A: stable fd count under ten-minute load | NEEDS-HUMAN | Scale fixture implemented and compiled, not run. |
| W5-B: approximately 20,000 fds in <500 ms | NEEDS-HUMAN | Synthetic algorithm timing excludes actual procfs work; real benchmark missing. |
| W5-D: state-machine branch coverage or equivalent argument | PASS | All twelve named cells plus timing/cap/threshold regressions inspected; equivalent argument, not measured coverage. |

## Pending VM tests and exact assertions

Nothing in this table was executed. Tests are feature-gated, with only the load test ignored as `scale`. Allocated-byte assertions use measured blocks/oracles rather than filesystem-dependent constants; nominal fixture write sizes are not allocation oracles.

| Test | Assertion to establish on both VMs |
|---|---|
| fs `lvm_maps_mounts_to_lvs` | Exact linear/thin/pool mapping, LV-size and VG-free oracle equality, collection <2 s. |
| CLI `lvm_thin_pool_usage_tracks_writes` | Synchronized 32 MiB fixture increases pool use, data/meta agree with reporting oracle within 0.01 percentage point, actual thin-pool alert fires after injected hold. |
| CLI `lvm_absent_is_na` | Non-LVM loop mount remains unmapped despite unrelated VG, enabled collection succeeds and text/JSON render n/a. |
| CLI `privileged::deploy_stage_preflight_blocks` | Missing-root preflight leaves installed bytes, absent state and service enable state unchanged. This single test does not inject every real preflight failure. |
| CLI `privileged::deploy_unstage` | From stopped daemon, real stage enables; purge stops/disables/removes marked temporary state and restores prior enable setting. |
| CLI `privileged::deploy_stage_reboot_verify` | Two separate stage/verify phases across actual reboot; healthy report/API and booted_at > staged_at. Deferred until W6 daemon exists. |
| sys `fanotify_probe_reports_capabilities` | Print available/fd/FID/DFID_NAME/filesystem/error and require fd support on each kernel. |
| live `privileged::fanotify_fd_mode_sees_close_write` | Child event within one second; correct path/PID/comm and measured allocation; record separately negotiated watcher mode. |
| live `privileged::exclude_prefixes_respected` | Excluded prefix absent, similarly named sibling present; stop <1 s under this fixture. |
| live `privileged::watcher_namespace_guard` | unshare -m changes namespace; start refuses Namespace. |
| live `privileged::fid_mode_create_delete_rename` | When supported, directory handle/path and raw CREATE/DELETE/MOVED_FROM/MOVED_TO names/directions correct. Record capability-based skips. |
| live library `watch::tests::kernel_overflow_worker_survives_and_resumes` | Locked worker receives 100,000-close burst; overflow/loss increases and a fresh later event is processed without Failed. Replaces the old ineffective overflow fixture. |
| live `privileged::watcher_sixty_second_load_and_ten_minute_fd_stability` | Ten minutes at 2,000 closes/s, first-minute generation ≥1,960/s, watcher-thread CPU <2%, processing advances each second and fd count returns unchanged. |
| live `privileged::phantom_detects_deleted_open` | Cross-user child holds unlinked 100 MiB fixture; holder allocation equals measured blocks, comm/path correct. |
| live `privileged::open_writer_growth` | Writable child's measured allocation growth reported; read-only holder excluded. |
| fs dependency `root_sees_other_user_open_file` | Actual cross-user descriptor PID/fd/inode visibility; attribution is not asserted by this primitive fixture. |

W5-A writer_child/paced_writer_child are helper test entry points, no-op without parent-supplied environment. W5-D specifies no privileged standalone test. Firstboot/journal/metrics and alert-source end-to-end behavior await Wave 6. Bind-mounted state/deploy refusal, mount disappearance, actual descriptor exhaustion and safe replacement races need additional VM fixtures/manual evidence; the existing named tests do not establish them. No claim rests on privileged execution.

## For the human gatekeeper

First resolve the blockers and exact-window/SCR decisions, choose one reviewed commit, and build/package that same commit. On separate disposable Rocky 8.8 and 8.10 VMs, provision the prerequisites in `xtask/README.md`: XFS root/data on LVM, loop space, xfsprogs/lvm2, Python 3 and util-linux (`setpriv`), tools, pinned Rust and the project RPM/unit for deployment tests. Do not run these commands on a production/development host. Use SSH aliases `rocky88` and `rocky810` or substitute the actual VM aliases.

From the reviewed checkout, these existing runner selectors exercise the core W4/W5 fixtures without accidentally running the two-phase reboot test:

```sh
for vm in rocky88 rocky810; do
  cargo xtask vm-test --host "$vm" --filter lvm_ --nocapture
  cargo xtask vm-test --host "$vm" --filter fanotify_probe_reports_capabilities --nocapture
  cargo xtask vm-test --host "$vm" --filter fanotify_fd_mode_sees_close_write --nocapture
  cargo xtask vm-test --host "$vm" --filter exclude_prefixes_respected --nocapture
  cargo xtask vm-test --host "$vm" --filter watcher_namespace_guard --nocapture
  cargo xtask vm-test --host "$vm" --filter fid_mode_create_delete_rename --nocapture
  cargo xtask vm-test --host "$vm" --filter kernel_overflow_worker_survives_and_resumes --nocapture
  cargo xtask vm-test --host "$vm" --filter phantom_detects_deleted_open --nocapture
  cargo xtask vm-test --host "$vm" --filter open_writer_growth --nocapture
  cargo xtask vm-test --host "$vm" --filter root_sees_other_user_open_file --nocapture
  cargo xtask vm-test --host "$vm" --filter deploy_stage_preflight_blocks --env TIDEMARK_DEPLOY_TEST_VM=1 --nocapture
done
```

Inside **each disposable VM**, from its synced checkout, with the pinned Cargo on PATH and the SSH user’s populated offline cache/toolchain under `$HOME/.cargo` and `$HOME/.rustup` (adjust these cache paths if customized), run the scale test and stopped-service unstage case:

```sh
sudo env "PATH=$PATH" "CARGO_HOME=$HOME/.cargo" "RUSTUP_HOME=$HOME/.rustup" cargo test --offline -p tidemark-live --features privileged --test w5a -- --ignored --exact privileged::watcher_sixty_second_load_and_ten_minute_fd_stability --test-threads=1 --nocapture
sudo systemctl stop tidemarkd
sudo env "PATH=$PATH" "CARGO_HOME=$HOME/.cargo" "RUSTUP_HOME=$HOME/.rustup" TIDEMARK_DEPLOY_TEST_VM=1 cargo test --offline -p tidemark-cli --features privileged --test w4b privileged::deploy_unstage -- --exact --nocapture --test-threads=1
cat /etc/rocky-release
uname -r
rpm -q lvm2 xfsprogs
sudo /usr/sbin/lvs --reportformat json --units b --nosuffix -o vg_name,lv_name,lv_attr,segtype,lv_size,data_percent,metadata_percent,pool_lv,lv_dm_path,lv_kernel_major,lv_kernel_minor
sudo /usr/sbin/vgs --reportformat json --units b --nosuffix -o vg_name,vg_size,vg_free
sudo target/debug/tidemark --json lvm
/usr/bin/time -f 'elapsed_seconds=%e' sudo target/debug/tidemark lvm
```

CLI time includes more than collector time; preserve the test's <2-second result and record a collector-only numeric timing if required. Capture actual JSON as fixtures. For manual attribution, in one VM terminal run `sudo target/debug/tidemark watch --mount /data`. In another use a dedicated disposable directory and two transient units:

```sh
watch_fixture=$(sudo mktemp -d /data/tidemark-watch.XXXXXX)
sudo systemd-run --wait --unit=fake-report /usr/bin/dd if=/dev/zero of="$watch_fixture/report" bs=1M count=8 conv=fsync
sudo systemd-run --wait --unit=fake-writer /usr/bin/dd if=/dev/zero of="$watch_fixture/writer" bs=1M count=8 conv=fsync
```

Record both paths and unit attribution, watcher mode, drops/loss and processing latency. If short-lived units exit before attribution, repeat with a fixture process that remains alive while the event is processed; record that condition rather than treating unknown attribution as a pass.

After W6 is implemented and installed, supply `/root/tidemark-vm.toml` for that VM's agreed mounts, then run the separate reboot phases inside that VM:

```sh
sudo env "PATH=$PATH" "CARGO_HOME=$HOME/.cargo" "RUSTUP_HOME=$HOME/.rustup" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_DEPLOY_VM_CONFIG=/root/tidemark-vm.toml TIDEMARK_DEPLOY_VM_PHASE=stage cargo test --offline -p tidemark-cli --features privileged --test w4b privileged::deploy_stage_reboot_verify -- --exact --nocapture --test-threads=1
sudo reboot
# Reconnect and return to the same checkout:
sudo env "PATH=$PATH" "CARGO_HOME=$HOME/.cargo" "RUSTUP_HOME=$HOME/.rustup" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_DEPLOY_VM_CONFIG=/root/tidemark-vm.toml TIDEMARK_DEPLOY_VM_PHASE=verify cargo test --offline -p tidemark-cli --features privileged --test w4b privileged::deploy_stage_reboot_verify -- --exact --nocapture --test-threads=1
```

Record exact source/RPM hash, OS/kernel/tools versions, complete logs, capability matrix, CPU/fd/timing measurements, config/state/deploy/report owner/modes, staged/firstboot JSON, btime and service/journal status. Redact sensitive paths only in shared copies; retain originals privately. Run the full privileged suite only after provisioning its additional Wave 1 MD fixture and arranging deployment phase selection; blindly invoking all tests will hit the reboot-phase precondition. The script install runner is `cargo xtask vm-test --host <vm> --suite install`, not `--filter install`; no install scripts exist yet in this tree.

Human decisions: resolve G4's window/completeness contract, the LvmResult presentation gap, permission diagnostics and undocumented ProcScanResult fields; select one exact candidate; require fixes plus real VM evidence before tags. Real 20,000-fd timing, removed-mount behavior and mounted-state/race fixtures still need a concrete harness. This review does not approve production operation.

## Build, test and probe results

Validation uses pinned Rust 1.98.0, host cache `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, `CARGO_NET_OFFLINE=true`, and `--offline` on direct Cargo invocations. No crates.io access was needed. All required commands completed on both trees; `checks.tsv` and the per-tree logs preserve exit statuses.

| Check | Gate checkout `f5aed2f` | Blind candidate `c224` |
|---|---|---|
| `tools/ci.sh` | Exit 1: source checks (139 owner tags), fmt and Clippy pass; tests stop on client EINTR (G5), before cargo-deny. | Exit 0: source checks, fmt, Clippy and tests pass; 385 passed, eight existing ignored across 49 test/doc-test targets. Cargo-deny explicitly skipped because not installed. |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0; compiled only. | Exit 0; compiled only. |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0. | Exit 0. |
| Supplemental `cargo test --offline --workspace --no-fail-fast` | Exit 0: 476 passed, 13 existing ignored across 49 test/doc-test targets; initial failure is not discarded. | Not an additional requested check. |

Focused built binaries: 30 deployment tests, nine client tests, five firstboot tests reported passing. The client listener test returns early after sandbox AF_UNIX EPERM, so this does not establish actual listener/connect success. Socketpair framing/deadline tests and the independent EINTR probe execute. Deployment permission probe succeeds at unsafe operations (G1); nested dirty assertion fails as expected (G3); the exact-window information-loss witness passes (G4); candidate scanner algorithms fail three regressions and checkout passes all three. Sources and complete transcripts are retained with these limitations. A blob-by-blob check of all 369 archived candidate files found no differences (`candidate-integrity.json`).

Tests using actual sockets/subprocesses have scheduling deadlines; webhook timeout/retry tests and direct-writer interval tests are not completely independent of machine timing. No filesystem-dependent allocation literal was found in the reviewed fixtures. No privileged suite, destructive host command, VM, real LVM mutation, real service action, dependency advisory audit, measured coverage, long soak or production performance was run. Build success cannot establish Rocky ABI/runtime behavior. Only the assigned report is committed; the report and receipt are evidence proposals.
