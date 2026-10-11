# W0-B2-fix — CLI frozen-contract adaptation and help

Implementation evidence candidate for attempt `attempt-0664a485da714b4f7c36d92a14653966d7d43b22c33cffbbaecc7df12af63547`; independent verification and integration remain external to this report.

## Changes and evidence

| Assigned item | Change | Evidence |
|---|---|---|
| Build against frozen core | Render helper converts the static schema to `Cow<'static, str>` using `Into`. Inspection found no `ScanId::new`, scan request DTO or writer API call sites in CLI/daemon production code: command implementations and daemon services remain their existing stubs. No daemon change was needed. | Workspace checks below; `rendered_json_deserializes_from_owned_string` exercises the actual helper and drops the owned input before using the parsed document. |
| Top growth sort | Replaced the existing string argument with CLI `Sort`, containing the eight frozen snapshot metric variants plus `Growth`. Accepted values and their order remain the same. Core is unchanged. | `top_sort_includes_snapshot_metrics_and_cli_growth` parses every value and rejects an unknown one. |
| Writer window | `Option<Duration>` parsed with existing humantime dependency; rejects zero and malformed durations, imposes no extra maximum. Hot retains its fixed window parser. | `writers_accepts_positive_durations_and_hot_keeps_fixed_windows` checks nanoseconds, arbitrary minute/hour windows, a large duration, invalid/zero values, all hot windows, and round-trips parsed writer durations through frozen core `WritersParams`. |
| Help | Replaced placeholder descriptions with command purposes and concise option explanations from interfaces §1.1–1.2, including snapshot/export/deploy children and positional arguments. | `all_commands_and_arguments_have_real_help` traverses the built clap tree, including inherited/generated options, rejecting absent, empty and `<name> command` descriptions. Existing complete-tree/default/short-option tests still cover the surface. |
| Regression tests | Added the two requested regressions plus focused duration and sort tests; existing six surface tests retained. | Ten surface tests passed locally; existing JSON completion test still checks the schema and native Bash script payload. |

No flag spelling, value name, default, exit-code mapping, completion generator or completion output envelope changed. No dependencies, manifests, lockfile, module declarations, core, other crates, plans or handoff files changed. Only CLI source/tests and this review note changed. No branches, merges or pushes were created.

## Interpretation and remaining work

The task-specific scope and explicit submit instructions take precedence over the general handoff/branch conventions in AGENTS.md. This note therefore lives in `docs/reviews/`, and the precreated attempt branch is retained. Current interfaces §1–2 agree with the decisions file on the implemented contracts; the old W0-B2 handoff's fixed writer-window interpretation is superseded by SCR-002/S1. Growth comparison/query execution remains W2-C's existing stub responsibility; this repair implements the prescribed parser surface only.

No privileged runtime behavior was introduced, so no new VM test is required. Privileged feature compilation is checked; runtime VM and Rocky Linux platform validation remain pending. Root-only access behavior is untouched. No host filesystem metadata, mounts, LVM, service enablement or installed config were changed.

## Validation

Validation uses Rust 1.98.0 on PATH, `CARGO_HOME=/tmp/tidemark-b2-fix-cargo` containing a copy of the host registry, and `CARGO_NET_OFFLINE=true`. Cargo commands use `--offline` where accepted; CI inherits offline mode. Transcripts are saved in the attempt output `library/` directory. Final results are recorded below after the checks finish.

| Required command | Local observation | Transcript |
|---|---|---|
| `cargo fmt --check` | Exit 0; also repeated by final CI | `cargo-fmt.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0; also repeated by final CI | `cargo-clippy.log` |
| `cargo test --offline --workspace` | Exit 0, including all ten CLI surface tests and daemon validation tests | `cargo-test.log` |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0 | `cargo-privileged.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0 | `cargo-doc.log` |
| `tools/ci.sh` (invoked with `sh`) | Exit 0; source checks and all available checks passed | `sh-ci.log` |
| `git diff --check` | Exit 0 | Observed before commit |

CI skipped cargo-deny because it is not installed, as permitted by the existing CI wrapper. Advisory/license auditing remains pending an equipped host. The rendered executable help is also saved as `library/help.txt`. No assigned repair item remains unimplemented; independent verification is not claimed.
