# Handoff — CLI-polish: operator-readable CLI output

## Needs from coordinator

- Decide the non-path writer-key contract. The CLI serializes `NowReport` unchanged; the frozen `HotEntryWire` in `tidemark-core/src/live.rs` emits `{"key":[117,115,101,114,45,49,48,48,48]}` for `user-1000`, while path keys use `{"path":"/data/hot"}` or `path_b64`. A possible operator-friendly non-path shape is `{"key":"user-1000"}` (with a separately specified raw form for invalid UTF-8). No new field or serializer was invented: item 4 explicitly requires leaving frozen core serialization unchanged. Interfaces §1.1 specifies path/path_b64 and contract decisions distinguish internal byte arrays from report paths; neither authorizes a CLI-only override of this frozen non-path key serializer.
- Investigate the out-of-scope daemon `blocked_mount_transition_log_does_not_hold_state_lock` failure seen in the first CI run (`w6a1.rs:1690`, waiting channel `Disconnected`), despite a passing standalone workspace run. Final-source CI is recorded below.
- Socket-dependent regressions and DAC-denial coverage need ordinary-user CI; new mock-daemon coverage uses the existing one-line permission detector and bounded accept/read deadlines. Privileged runtime is pending VM. Validation outcomes below are evidence candidates, not independent verification.

Base: `b2d9c72ee3b2aca104310cf704ab55885c7cc91d`. Canonical attempt branch retained; no branch creation, merge or push.

## Per-item changes and evidence

1. **Error context.** Previously `top /nonexistent` could emit only `No such file or directory (os error 2)`. It now emits `top: /nonexistent: resolve query path: /nonexistent: No such file or directory (os error 2)`. Every dispatched command has an operation/path context (explicit direct process queries name their mount filter or `/proc`, and deployment stage names its source configuration); additional contexts identify configuration loads/validation, catalog/state access, selected and older snapshot files, scan state, export creation/publication, watch/mount resolution, and daemon request methods/socket connection. The full anyhow cause chain is printed, while contextual CLI errors retain exit codes 1–6 and permission hints. `failures_name_operation_subject_and_preserve_status` covers missing top and scan roots (1), an absent socket (4), and a directory used as an unreadable config (2). `missing_snapshot_file_names_the_file_and_operation` removes a published fixture snapshot and checks its exact path, cause and exit 1. `unreadable_configuration_names_path_and_keeps_access_hint` covers real mode-000 denial and code 2, dropping DAC privileges for its child when the harness is root. `direct_and_deployment_contexts_name_the_input_subject` covers direct/source/default-installed config selection. Existing code-preservation tests remain.
2. **Hot sizes.** `+8388608` becomes `+8.00 MiB`, sharing now's signed-byte formatter. Numeric data columns are right-aligned by the shared wrapping table helper. JSON byte deltas remain integers. `signed_sizes_align_and_json_remains_integer` covers positive, negative, zero, i64::MIN, column ends at widths 80/100 and JSON integers.
3. **Enumeration case.** Human snapshot list/header, top provenance, scan outcome, diff provenance, status state/watch mode, now scan mode/phase, probe device kind, and TUI scan mode/pane/sort use lowercase labels. `Walk`/`Complete` becomes `walk`/`complete`; JSON is unchanged. `snapshot_header_uses_lowercase_enumerations`, `live_human_state_and_scan_labels_are_lowercase`, `human_header_and_pane_labels_are_lowercase`, existing catalog assertions and the updated scan/diff/TUI goldens cover the change. TUI and live tests inject widths 80/100.
4. **Writer JSON key.** Unchanged for the frozen-core reason above. `writer_unit_key_json_uses_the_frozen_core_serializer` compares the complete nested entry against core serialization and pins the current array. This is contract evidence, not a claimed regression fix.
5. **Hostile diagnostics.** Deployment check names/details and staged diagnostic values escape controls at the human boundary. Final errors use the same control helper, preserving already-escaped raw-byte paths. A single check detail with `/tmp/admin<ESC>[31m<LF>forged PASS` stays on one line as `/tmp/admin\x1B[31m\x0Aforged PASS`. Layout newlines are owned by the renderer: separate checks, validation issue lines, and the access hint retain their breaks; embedded message/path newlines become byte escapes. `hostile_path_in_check_is_one_escaped_diagnostic_line` checks deployment output and unchanged JSON detail. `human_diagnostics_escape_path_controls_and_keep_owned_line_breaks` covers final errors, multiple validation lines, hints and prior escapes. `final_error_boundary_escapes_hostile_configuration_path` sends ESC/newline/non-UTF-8 path bytes through the real binary.
6. **Long watcher detail.** Existing word wrapping already handles the appended explanation; no production layout change was needed. `appended_watcher_cap_detail_wraps_without_losing_reason` uses `truncated: distinct-file cap reached in the minute 10:42 UTC (timestamp 1791888120) (events complete, per-file counts capped)` at 80/100, reconstructs every word, and checks status cell/now prefix continuation indentation and line widths. This preservation regression also passes the old wrapping implementation; deliberately breaking correct wrapping to make it fail would be inappropriate.
7. **Unavailable process scan.** Neither writers/phantom renderer manufactures `measurement unavailable`; daemon `api.rs`'s generic helper is the source. The CLI keeps producer reasons and now adds `daemon request writers` / `daemon request phantom` plus command/socket context. `unavailable_process_measurements_keep_daemon_reason_and_name_method` injects the old daemon reason and checks subject, measurement, preserved reason and code 4. It requires sockets; no daemon changes were made.

## Golden files and workspace assertion search

Updated five owned goldens:

- `tests/w2c/snapshots/w2c__scan_summary.snap`
- `tests/w3a/snapshots/w3a__reports__diff_text.snap`
- `tests/w3a/snapshots/w3a__reports__diff_warning_text.snap`
- `tests/w7a/snapshots/tidemark_cli__tui__tests__main_80_24.snap`
- `tests/w7a/snapshots/tidemark_cli__tui__tests__main_160_48.snap`

Updated existing `tests/w3a/catalog.rs`'s human `Walk` assertion and `tests/w7a.rs`'s exact stderr assertion (now includes `tui: /data:`). No JSON golden changed. Whole-workspace searches are retained in external `library/assertion-search.txt`, `exact-output-assertions.txt`, and `cli-callers.txt`. The daemon operator tests run the CLI and assert component names/schema/report quantities, not the changed labels or full diagnostics. No old-text assertion requiring an out-of-scope change was found. Core/store tests construct enum variants; these are type assertions, not human text expectations. The w4b tests and symlinks command were inspected only, not edited.

## Assumptions and scope

- No plan wave section is assigned to this post-wave task; the explicit CLI-polish scope and requirements govern it. Read the interface/contract decisions, relevant wave CLI surface, audit F06, prior display/daemon handoffs and handoff/playbook conventions.
- Missing configuration still selects defaults; missing requested snapshot files remain runtime errors; exit code meanings are unchanged. Final context retains existing OS cause capitalization and `(os error N)` rather than replacing diagnostic causes with guessed messages.
- An absent endpoint client error abstracts missing/refused transport as `daemon unavailable`; the socket path and operation are now explicit. No extra endpoint probing, wire fields or client contract changes were added.
- Diagnostic controls are escaped without re-escaping literal backslashes already introduced by RawPath's byte display. Intended line breaks are emitted separately from message cells. JSON rendering is unchanged.
- Buffers remain over existing bounded reports; no scanner I/O, dependencies, manifests, unsafe code, metadata changes or host deployment actions added. No shared module declarations changed. `cmd/mod.rs` only adds internal contextual helpers; `lib.rs` is explicitly owned.
- Commands use `timeout 1500`, offline Cargo and cached toolchain 1.98.0 via `/home/brewerm/.cargo` and `/home/brewerm/.rustup`. The initial default PATH lacked Cargo; subsequent commands explicitly select the cache.
- Fourteen tests added (five library, nine integration). Item 4 and item 6 are preservation/contract checks; socket cases can return early. The unreadable-config subprocess uses a private executable copy and drops to UID/GID 65534 when its harness is root; all permission changes stay inside its temporary directory. When a root sandbox refuses this child identity change (InvalidInput/PermissionDenied/Unsupported), it prints one pending line and returns; this attempt observed InvalidInput, so real DAC denial remains pending ordinary-user CI.
- No memory deliveries were pending at checkpoints; projections were not edited. No questions or external messages sent.
- Local follow-ups are consolidated into one commit per item for submission. Only this isolated attempt's unpublished commits are rewritten; baseline and shared history are unchanged. The final tree is compared before/after consolidation, so tested source bytes are preserved.

## Validation

Logs are in this attempt's external `library/`. Final `tools/ci.sh` executes format, workspace Clippy, workspace tests and privileged test compilation on the settled source. No privileged runtime or VM test was run.

| Required check | Observed outcome | Evidence |
| --- | --- | --- |
| `cargo fmt --check` | exit 0, also passed inside final CI | `settled-fmt.log`, `final-ci.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0, also passed on final source inside CI | `settled-clippy.log`, `final-ci.log` |
| `cargo test --offline --workspace --no-fail-fast` | exit 0: 825 passed, 0 failed, 14 ignored; final CI: 826 passed, 0 failed, 14 ignored | `workspace-tests.log`, `final-ci.log` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0, repeated successfully by final CI | `privileged-build.log`, `final-ci.log` |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | final exit 0; 148 owner tags and forbidden-symbol scan passed; cargo-deny absent and skipped with warning | `final-ci.log` |

Source `#[test]` declarations: CLI 185 → 199; workspace crates plus xtask 872 → 886. Fourteen new tests were added. Final CI CLI harness totals are 188 passed, 0 failed, 3 ignored. Final focused library/surface/w6a3 run reports 134 passed, 0 failed, 1 ignored (`settled-cli.log`). Reported runtime counts include nested subprocess harnesses and early-return socket/identity detectors; they are not unique executed-test or unrestricted coverage counts.

The standalone workspace run preceded the last direct/deployment/completions context and permission-detector follow-ups; final CI covers the settled implementation and all fourteen new test declarations. First CI reported 824 passed, one failed, 14 ignored, stopping before its privileged/deny stages because the unowned daemon mount-transition logging test's waiting channel disconnected. Final CI passed that same test. Its cause remains uninvestigated in this scope; both logs are retained. Intermediate owned failures (initial cause-chain display, old TUI goldens, three renderer lints, octal-permission lint, restricted child identity and the fallback/direct primary-subject mismatch) were fixed and their later focused/CI results passed. Earlier CLI-only run reported 185 passed, 0 failed, 3 ignored before final follow-ups.

No advisory/license audit success is claimed: cargo-deny is not installed. Socket tests returned early where the sandbox refused binds; the mode-000 permission subprocess reported the restricted UID/GID change explicitly (`settled-surface.log`). Ordinary-user CI and privileged VM runtime remain pending.

## Definition of Done evidence candidate

- Owned source/test/handoff paths only; no excluded symlinks/deploy implementation/w4b edits, manifests, dependencies or frozen types changed.
- Final source fmt/Clippy/workspace tests/feature-enabled compilation/available CI checks passed locally.
- All seven items are implemented, preserved with regression evidence, or explicitly deferred under the task's frozen-contract exception for item 4.
- Public helpers documented; existing ownership headers retained; no new unsafe or production unwrap/expect.
- Five human goldens updated; JSON goldens unchanged; whole-workspace assertion searches retained.
- Local validation does not verify unrestricted socket/DAC/VM behavior or resolve the earlier unowned daemon test failure. Submission/cleanup evidence is in the attempt report.
