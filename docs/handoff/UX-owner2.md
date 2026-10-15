# Handoff — UX-owner2: Owner first-server CLI rough edges

- Branch: precreated attempt branch `hp-e760cb7ee6a0ffaf5c1f355104fee616e5cb259d2c6e8b595b446dbd0a3b6d4e`
- Agent: Codex; date: 2026-10-14
- Status: implemented; evidence candidate awaiting independent verification

## Needs from coordinator

- Run cargo-deny on an equipped host; local CI used its documented unavailable-tool skip.

- Independently gate this candidate as uid 1000 with umask 022 and sockets, and on Ubuntu 24.04. Socket-dependent tests print the existing pending-host line and return when binding is forbidden.
- Privileged runtime acceptance remains pending VM; this attempt only builds those tests.
- Adopt the proposed interface plan text below; normative plan files are outside this task's scope.

## What changed and acceptance evidence

1. **Unwatched `hot --mount`:** the daemon's consistent state view checks the effective watch list and recorded mount presence before querying activity. It returns the existing Unavailable API error with the requested path, effective list and deployment hint. The CLI recognizes this diagnostic and maps it to Precondition (5), with one stderr line. JSON retains `tidemark.hot.v1` and the existing API `error` shape (`code`, `message`) under `data`. Other hot errors keep their existing mappings. `hot_unwatched_mount_is_distinct_from_watched_idle_mount` exercises daemon dispatch without sockets, including an unwatched subtree and an idle watched mount. `hot_unwatched_mount_is_precondition_in_text_and_json` uses the fake daemon for exit status, stderr and JSON.
2. **Singular count wording:** the watcher history hint now says `1 minute`; other counts still use `minutes`. Count-following nouns in scan progress, live hot/phantom/writer displays, link reports, catalog rebuild, foreground watch, and TUI link/file panes now select singular for 1. Zero uses plural. Column and field labels remain fixed labels. `progress_unit_words_follow_displayed_counts` covers 0, 1 and 2 through the renderer; `progress_rate_unit_matches_rounded_display` covers displayed fractional-rate boundaries; `hot_dispatch_reports_startup_and_replacement_boundaries` pins the one-minute daemon hint. Existing live-output and link golden assertions were updated.
3. **Absent purge state:** `unstage` skips the purge-specific root/marker walk only when the trusted descriptor-relative state lookup returned no state handle. All initial path/ancestor checks remain; present roots still run the original purge checks. Stop/disable and still-active refusal remain unchanged, including compensation. The CLI reports `state directory <path> already absent; nothing to purge` after success. `purge_absent_state_still_stops_service_and_preserves_stop_refusal` uses the existing fake Systemctl for success and stop refusal; `absent_purge_message_names_directory` pins the requested wording. Existing symlink, unsafe-owner/mode, missing-marker, mount and busy-writer tests remain in the suite.

Operator guidance documents the behavior and fixes its `1 files` example.

## Golden files updated

- `crates/tidemark-cli/tests/w3b/snapshots/w3b__links_text.snap`: `1 set` in the incomplete total and pinned-directory row.
- Inline golden in `crates/tidemark-cli/src/render/scan_progress.rs`, `log_cadence`: `1 dir`.
- `crates/tidemark-cli/tests/w6a3.rs`: existing rendered live assertion now expects `1 file`.

## Shared-file edits and dependencies

No manifests, module declarations, frozen core types, JSON field names, schema names, config keys, unit files or store files changed. No dependencies added. The shared CLI API error conversion in `cmd/now.rs` was extracted into `call_error` so hot can specialize only its unwatched diagnostic.

## Proposed plan text

For `08-interfaces.md`, hot behavior: “An explicit mount outside the daemon's effective watch.mounts, or recorded as not mounted, returns an Unavailable API error naming the mount and watch list with a deploy-stage hint. The CLI maps this diagnostic to Precondition (5). JSON uses tidemark.hot.v1 with data.error containing the existing ApiError code/message. Idle watched mounts with complete history continue to return an empty successful result.”

For deployment: “Unstage --purge-state --yes treats an already absent trusted state path as nothing to purge; it still stops/disables the service and retains all service failure behavior. Existing directories retain every purge guard.”

## Assumptions

- The frozen API has no Precondition code. Unavailable describes the absent watched measurement; only the CLI exit changes to 5 for the explicit diagnostic. No new wire code is introduced.
- Existing mount canonicalization remains unchanged; local path-resolution failures retain their existing behavior. An existing non-mount path is diagnosed by daemon watch-list membership, and a configured path recorded absent is diagnosed by mount presence.
- The existing ApiError code/message object is the requested error shape. Successful hot data remains an array; the unwatched error data contains `error` and uses the same schema name.
- Diagnostic labels preceding counts (column names, `Files:`, `Link sets:`) are labels, not count-following unit nouns, and remain stable.
- No new privileged operation is introduced; existing privileged coverage is compiled, never executed in this attempt.

## Validation and test counts

All commands ran offline with the host cargo cache and pinned Rust 1.98.0. Each long command used `timeout 1500`; local uid was 0 and umask 022.

| Check | Observed result |
|---|---|
| `cargo fmt --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | exit 0; repeated against final code by CI |
| `cargo test --workspace --no-fail-fast` | exit 0; repeated against final code by CI |
| `cargo build --workspace --features privileged --tests` | exit 0; build only |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | exit 0 |
| `tools/ci.sh` | exit 0; 150 owner tags and forbidden-symbol scan passed |

The first workspace run's harness-summary aggregate was 975 passed, 0 failed, 14 ignored. Final CI's aggregate, including its strict-ancestry check and child helper harnesses, was 978 passed, 0 failed, 14 ignored. These totals include subprocess helper summaries rather than counting unique test names. CLI unit tests: 125 passed, 1 scale test ignored. The two progress regressions also passed in a targeted final-code run. The fake-daemon regression printed `pending unrestricted host: Unix socket bind denied by sandbox (EPERM)` and returned; its socket body remains pending coordinator gate. Daemon dispatch regression ran and passed without sockets.

Logs: attempt output `library/fmt.log`, `clippy.log`, `tests.log`, `privileged-build.log`, `docs.log`, `ci.log`, `socket-regression.log`, and `rate-regression.log`. Privileged runtime tests remain **pending VM**.

CI reported cargo-deny unavailable and skipped its advisory/license audit under the existing tooling policy; an equipped host should run that audit.

## Items not done / remaining uncertainty

No requested implementation item is intentionally deferred. Independent uid-1000/socket, GitHub runner and VM runtime checks remain coordinator work. This is an evidence candidate, not a verified integration result.
