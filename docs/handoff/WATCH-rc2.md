# Handoff — WATCH-rc2

## Needs from coordinator

- Run ordinary-user CI at `umask 022` with Unix sockets and user namespaces available; this worker sandbox runs as uid 0 and denies those facilities.
- Run the existing privileged watcher lifecycle and daemon tests on both Rocky guests. No privileged tests, fault scripts, install scripts, mounts, service changes or LVM operations were executed here.

Status: implementation and evidence candidate; independent verification pending. Attempt: `attempt-e8b3d989877803921a8c308037e7c3a37df29b68e6f73d5c00b2eb7b39db28cd`. Base: `67847fb2afb3ed8510a29e6bc8594e002b5e4d87`. No branch creation, merge, push or release action.

## Per-finding changes and discriminating tests

### F1 — retired views

Watcher state retains an explicit retirement timestamp. `query_at` decides retirement, startup coverage and rows under the watcher mutex; `LiveStateView::hot` uses this one decision. Any selected retired watcher makes the answer unavailable, including combined-mount requests. Its detail is `watcher for <mount> was replaced at HH:MM UTC`, prioritized over startup warnings from other mounts. `now` omits retired directory/unit rows and lists the retirement line in `degraded`. Existing vector-returning watcher APIs remain compatible; their retired query path cannot recreate hot rows.

Service retirement records its injected reconciliation timestamp before invoking the existing shutdown path. Direct shutdown also records retirement before joining. The worker stop/join, event descriptor close, mount descriptor release and `clear_retained` ordering are unchanged. No retained pathname payload is preserved to implement this state.

`held_mature_view_is_unavailable_after_retirement` uses a two-party barrier and joined thread: a held mature view first has lossless 82-byte rows in all three scopes and all four API windows; another thread retires and replaces the watcher; the held view must return None and the retirement text for all twelve queries, and its now report must carry the line. A fresh view must return the replacement's 82-byte rows. Without retirement-aware querying the held view returns measured empty and fails. `combined_mount_window_returns_known_rows_and_names_the_incomplete_mount` also requires a retired second mount to take diagnostic precedence over an uncovered first mount. Existing concurrent shutdown/resource-release tests remain in the live-library and daemon-service suites.

### F2 — dispatch explanation

Hot dispatch parses parameters once and maps None to `DaemonError::Unavailable(view.hot_unavailable_detail(&p))`. Zero available minutes says `watch history for <mount> starts at HH:MM UTC; no complete window yet`; positive coverage retains the requested maximum-window guidance. No wire types or CLI changes are needed: the existing CLI error mapper preserves daemon text and the now renderer prints degraded lines.

`hot_dispatch_reports_startup_and_replacement_boundaries` calls the public dispatcher without sockets, with an empty startup watcher and an empty replacement after explicit retirement/removal. It asserts unavailable codes and exact zero-minute and one-minute guidance, and retirement text through dispatch. Generic `available(...)` fails these assertions. Existing view-only expectations were updated for the zero-minute wording.

### F3 — raw reload identity

Watcher membership uses its retained `RawPath` converted to a borrowed Path; relevant exclusions compare raw Path/PathBuf prefixes. Immutable-window retention resolves its existing presentation key to an old configured raw path, then compares raw membership, raw exclusion settings and raw rule mounts. Requested alert-window matching uses the same mapping. Escaping only generates presentation labels, including component removal labels. No new identity map or unbounded collection was added.

`reload_preserves_raw_mount_history_and_windows` runs for `/data\\archive` and `/data` followed by a newline and `archive`. It seeds actual watcher rows, completed-minute history and a published immutable alert window. Reorder and extension with an unrelated exclusion must retain Arc identity, exact hot/history rows and the window timestamp; a relevant exclusion must retire the watcher, clear its minute history/window and invalidate a held hot view. Display-string membership or exclusion comparisons fail these checks.

## Shared files, contracts and assumptions

- Only assigned paths changed. No manifest, dependency, module declaration, core type, CLI flag, config key, API method or metric name changed.
- New watcher methods are additive. The crate-private settings helper is called only from the two owned files and now accepts a raw Path.
- Existing display-keyed service/state maps remain compatible; raw identity is recovered at their label boundary, then used for filesystem comparisons.
- Retirement is a linearization point: a query that acquires the watcher lock before retirement may return its measurement; one after retirement is unavailable. Retirement is permanent for that watcher instance.
- Combined requests with any selected retired watcher are unavailable, even if another watcher has rows. Existing startup-only partial answers retain lossy semantics.
- Synthetic barrier/event tests establish query and service logic, not kernel fanotify behavior or end-to-end socket transport. Boundaries and retirement clocks in regressions are injected.

## Proposed plan and security text

Plan proposal for live monitoring/reload: “Watcher retirement is an explicit unavailable lifecycle state. Coverage, lifecycle and hot rows are read atomically under the watcher lock. Retained views disclose replacement time; replacement watchers have independent history. Empty uncovered windows explain history start and the admissible window, or report that no complete window exists. Membership and relevant exclusion comparisons use raw mount paths; escaping is for labels.”

Security proposal: “Retained API views cannot keep watcher transport alive or present reclaimed retired history as complete empty data. Retirement preserves bounded shutdown reclamation. Raw mount identity avoids history resets caused by presentation escaping. These changes add no filesystem mutation, content read, capability, transport or dependency.”

These are proposals only; `docs/plan/**` and `docs/security.md` were not edited.

## Validation

Final command outcomes and test counts are recorded below after completion of the offline validation run. All long commands use `timeout 1500`, the pinned Rust 1.98.0 binaries, `CARGO_NET_OFFLINE=true`, and `umask 022`.

Privileged execution is pending VM. From an equipped coordinator run `cargo xtask vm-test --host <rocky-8.8-guest>` and `cargo xtask vm-test --host <rocky-8.10-guest>`. On each isolated root guest, the focused existing teardown check is `cargo test --offline -p tidemarkd --features privileged --test w6a2s daemon_sigterm_releases_loop_xfs_for_ordinary_unmount -- --exact --nocapture`. Full guest gates establish real fanotify, ordinary unmount, socket and reload behavior; worker tests do not.
