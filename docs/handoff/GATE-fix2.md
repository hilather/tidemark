# Handoff — GATE-fix2: contextual CLI refusal assertions

**Product difference:** direct mode prints `scan: /: scan state: <state-dir>: NoSpace`, while the daemon path prints `scan: /: NoSpace`. Direct `run_scan` errors gain the state-directory context at `cmd/scan.rs`; `start_scan` maps the daemon's Unavailable/NoSpace refusal straight to a precondition, discarding request context. Both retain exit 5. This task asserts each actual line separately, as explicitly required for an inconsistency, rather than claiming their wording matches. Product changes remain outside scope; coordinator should decide whether to align the contexts.

- Branch: retained canonical `hp-8f86ea98853e321a33596dffcc55b6cd3992009e864abf69d13683e73af2acfa`; date: 2026-10-13.
- Status: implemented; evidence candidate, independent verification pending.

## What changed

`daemon_scan_space_refusal_matches_direct_exit_and_wording` now compares complete stderr including its final newline for all four responses:

| API response | Exit | Full error line (without newline) |
| --- | --- | --- |
| Unavailable, `scan: NoSpace` | 5 | `scan: /: NoSpace` |
| Unavailable, `scan: NoSpace: floor` | 5 | `scan: /: NoSpace` |
| Unavailable, `measurement unavailable` | 4 | `scan: /: daemon request scan.start: measurement unavailable` |
| Internal, `scan: NoSpace` | 1 | `scan: /: daemon request scan.start: daemon error: scan: NoSpace` |

Exit-5 cases explicitly reject `scan: scan:` and `daemon error`. The added `direct_scan_space_refusal_has_state_context` test uses the same `/` root, an absent socket and a private state directory, with `retention.min_free_pct=100.0` to refuse before walking the root. It asserts exit 5 and the entire actual direct error line. Quiet output suppresses unrelated throttle warnings. If the full stderr is exactly the contextual `root mount changed during setup` refusal, the test explicitly reports pending unrestricted-host coverage and returns: that sandbox preflight prevents reaching the space guard. No disk exhaustion or host configuration changes are needed.

## Reading and assertion audit

Read AGENTS.md, the explicit post-wave task (no wave section assigned), CLI-polish item 1, W0-A offline deviations, playbook §5 and the handoff template. Traced `scan_error`, direct `run_scan` wrapping, daemon `start_scan`, `now::call`, command `error_context`, `command_error`, `human_error` and `main_entry`. `now::call` adds `daemon request scan.start` for the exit-4 and exit-1 cases; the exit-5 remapping removes it.

Searched both test trees for command prefixes, `os error`, `contains("scan:")`, `starts_with("error:")`, exact stderr and related error assertions; external `library/assertion-search.txt`, `prefix-search.txt` and `bare-error-search.txt` retain results. No additional stale CLI assertion was found. The contextual `top:` assertion in w2c and exact `tui: /data:` stderr in w7a already match the printer. Surface's permission test checks configuration context, the permission cause and the separately printed access hint; these remain valid. The user-namespace test in `rules.rs` only recognizes `unshare` setup failures and checks scan success; it does not assert a bare Tidemark OS diagnostic. Socket-dependent w6a3 assertions retain cause fragments, forwarding behavior or request/subject context and do not forbid the newly added prefix.

Daemon findings (listed, not edited): `crates/tidemarkd/tests/w6a3.rs` asserts `error.message == "scan: NoSpace"` and `component.detail == "scan: NoSpace"` in its space-refusal test. These are API/status payloads, not CLI stderr, and remain correct under the frozen wire contract. No daemon CLI assertion requiring contextual-wording changes was found; operator tests inspect reports/component details.

## Validation and limitations

- `cargo fmt --check`: exit 0.
- `cargo clippy --offline -p tidemark-cli --all-targets -- -D warnings`: exit 0.
- `cargo test --offline -p tidemark-cli --no-fail-fast`: exit 0, 194 reported passed, 0 failed, 3 ignored (counts include early-return tests); external `library/tests-settled.log`; initial `library/tests.log` records the new direct test hitting the sandbox mount-identity precondition. An intermediate `--quiet` typo (the CLI accepts only `-q`) failed locally and was corrected before settled validation. `library/direct-coverage.log` confirms the precise pending mount-identity branch.
- Cached Rust 1.98.0 selected through `/home/brewerm/.cargo` and `/home/brewerm/.rustup`; offline flag added to Clippy as required by unavailable crates.io.
- Socket-dependent bodies return early in this sandbox; all four daemon error strings were checked by reading the printer, not claimed as executed socket coverage. The direct NoSpace body also needs unrestricted-host execution because `/` fails mount-identity setup here. Unrestricted coordinator CI remains necessary.
- No privileged command or VM runtime run; no new privileged tests needed for this assertion-only task.

## Scope and deviations

Only `crates/tidemark-cli/tests/w6a3.rs` and this handoff changed. No shared-file edits, dependencies, product code, frozen contracts, branches, merges or pushes. Existing ownership header retained. The additional direct-mode test is required evidence for the task's inconsistency exception. Task-specific CLI checks used rather than full workspace/xtask CI. No memory deliveries were pending at checkpoints; memory projections were not edited.
