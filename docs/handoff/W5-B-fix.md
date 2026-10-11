# Handoff — W5-B-fix: Process scanner review corrections

- Branch: canonical attempt branch hp-d544bd2d478d3e01d917824360a4afeac1e030baa86d231ae671eab02ec369e0
- Date: 2026-10-11
- Status: implementation and regression evidence candidate; independent verification and VM runtime pending.

## Changes and acceptance evidence

All four findings were present in the baseline; none was already fixed.

| Finding / decision | Change | Regression evidence |
|---|---|---|
| 1: starvation at capacity | Every other successful pass admits up to half the table from previously untracked keys, in cyclic key order. Evict the smallest observed growth first, ties by device/inode; the intervening pass gives admitted entries a second observation. Larger established growers retain priority. `ProcScanResult.writers_full` reports excess candidates and `writer_evictions` counts capacity evictions on that pass. | `admission_reaches_initially_excluded_grower`: inode 19,999 grows across eight 20,000-fd passes and becomes visible; both bounds checked every pass. Existing `ring_cap_eviction_and_twenty_thousand_fds` checks the 60-sample ring. |
| 2: covered mount device | Shared direct setup calls `mount_for_path`, retaining exact mount-point validation; later overmount wins. Both commands already call this setup. | `shared_setup_selects_visible_overmount`: duplicate target with different devices selects the later record; rejects a child path. |
| 3: incorrect measured window | Report the timestamp difference between the selected baseline and latest observation. Short history uses its oldest observation and reports that actual span. Subsecond requested windows round the selection boundary to seconds; positive growth with a zero timestamp span is omitted. No new core representation or contract is introduced. | `short_history_and_repeated_seconds`, `irregular_window_baseline_sort_limits_and_subsecond`; executable `direct_writer_reports_observed_interval` grows a temporary file during two passes, requests 1h and verifies a positive measured interval under 60s. |
| 4: missing realpath | Canonicalize original mount paths before constructing `RawPath`, propagating resolution failures. | `shared_setup_canonicalizes_alias_relative_and_symlink_parent`: symlink, relative path, symlink/parent resolution, missing-path error. |
| Accepted identity change | `OpenFile.starttime` carries the start time validated around enumeration. Scanner compares each descriptor with later attribution; mismatch emits `?` with no unit. | `enumeration_identity_mismatch_is_unknown` covers both phantom and writer persistence; W1-D `finds_deleted_open_file_without_reading_contents` compares returned identity with the current process's starttime. |
| Rejected procfs Gate | Added required rustdoc sentences on `scan_open_files` and `ProcScanner`: procfs does no disk I/O, so Gate charging and O_NOATIME do not apply. Existing caps remain. | Source inspection; no Gate or dependency added. |

## Shared-file edits and dependencies

Only explicitly owned paths changed. No manifests, lockfile, core types, shared module declarations, plan documents or dependencies changed. All workspace `OpenFile` constructors are in scope and updated; no external constructor needs coordinator work. The task's explicit SCR decision takes precedence over the unchanged SCR document's older Pending label.

## Validation

Offline Rust 1.98.0 using the host Cargo cache. Command logs and exit statuses are in the canonical attempt output's `library/`.

Validation results are recorded below after the final runs. No privileged tests were executed; existing W5-B deleted-open and writable-growth VM fixtures remain pending VM. The direct executable fixture writes only a bounded temporary file (at most 32 MiB), never monitored host data.

## Needs from coordinator / limits

Unowned filesystem/scanner tests have missing `WalkConfig.max_record_bytes` initializers and obsolete walk-result expectations. These prevent full workspace checks and require the concurrent walk/bulk owner. No out-of-scope repair attempted. Real XFS/cross-user runtime and performance remain pending VM. Attribution still cannot identify inode reuse between independent observations using the existing device/inode contract.

Timestamp precision remains the frozen whole-second precision of `scan_once`; repeated-second observations alone cannot yield a rate and are omitted. Direct mode supplies two observations, so it cannot supply an hour of history, and now honestly reports its shorter measured span.

## Final command results

| Command | Result |
|---|---|
| `cargo fmt --check` | Passed, including final rerun. |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 101: unowned `crates/tidemark-fs/tests/w1b/main.rs` has a missing `max_record_bytes` initializer and two obsolete walk return/error expectations. |
| `cargo test --offline --workspace` | Exit 101: missing `max_record_bytes` in unowned `crates/tidemark-scan/tests/w2a/main.rs:553` (and W1-B compile errors). |
| `cargo build --offline --workspace --features privileged --tests` | Exit 101: same unowned W1-B/W2-A test compilation errors. |
| `tools/ci.sh` with `CARGO_NET_OFFLINE=true` | Exit 1 at workspace Clippy; source policy and format passed. |
| `cargo clippy --offline -p tidemark-live -p tidemark-fs -p tidemark-cli --lib --test w5b --test w1d -- -D warnings` | Passed. |
| `cargo test --offline -p tidemark-live -p tidemark-fs -p tidemark-cli --lib --test w5b --test w1d` | Passed: CLI 47 library + 2 executable tests; FS 34 library + 10 W1-D tests; live 51 library tests. |
| `cargo build --offline -p tidemark-live -p tidemark-fs -p tidemark-cli --features privileged --lib --test w5b --test w1d` | Passed; compiled privileged fixtures without executing them. |

All requested source corrections and regressions implemented. Full workspace validation is not complete due to the recorded out-of-scope compile failures. No independent verified-success claim is made.
