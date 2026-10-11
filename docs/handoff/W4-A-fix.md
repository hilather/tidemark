# Handoff — W4-A-fix: W4-A review findings

- **Attempt:** attempt-448e3a9a4f6130189e2208a6c09d706ead1f6ccfa054d930a34f78bb832e73ce
- **Date:** 2026-10-11
- **Status:** implementation submitted as an evidence candidate; independent verification and VM execution pending.

## What changed

F1: `lvm_maps_mounts_to_lvs` now creates both linear and thin XFS LVs with `LoopVg`, checks the exact mounted LV names, and checks `thin` maps to `pool` while `linear` has no pool. Existing size, VG-free and collection-time assertions remain. Fixture sizes are increased to accommodate current xfsprogs' minimum filesystem size.

`lvm_thin_pool_usage_tracks_writes` moves to CLI integration tests, using its existing live dependency. It writes and synchronizes 32 MiB to a `LoopVg` thin LV, asserts pool data usage increases, compares data and metadata percentages with the separate plain-numeric `test_support::oracle::lvs_bytes` report for the exact pool, and evaluates the real alert engine across the test rule's one-second hold period. The rule's threshold is the measured initial data percentage; the firing event must identify `thin_pool_pct` and the exact VG/pool subject.

`lvm_absent_is_na` also lives in CLI integration tests. With collection explicitly enabled and installed reporting tools, it creates a non-LVM `LoopXfs` alongside an unrelated `LoopVg`, asserts the loop mount stays unmapped, and checks successful text and JSON CLI n/a output. The original missing-tool assertion remains covered by the unprivileged `command_environment_arguments_success_and_failure` and CLI missing-tool tests.

F2: `Report`, `TrackedLv`, `Trend`, their exposed fields, and `write` in the CLI renderer are now `pub(crate)`. Output formats are unchanged.

## Release override conditions

Source inspection: `crates/tidemark-fs/src/lvm.rs`, `executable`, guards both binary overrides with `cfg!(any(debug_assertions, feature = "privileged"))`. With `debug_assertions = false` and the privileged feature absent, the condition is false and execution returns the absolute default path regardless of either environment variable. Both collector executable selections call this helper. No release execution or host LVM command was needed for this confirmation.

## Scope and deviations

No shared-file edits, dependencies, manifests, lockfiles or contract changes. Tests involving alert evaluation and rendering moved to the owned CLI integration test file because tidemark-fs has no live/CLI dependency. All three privileged scenarios remain feature-gated without ignore attributes. VM scenarios explicitly select installed reporting tools; local unprivileged checks select missing executables to avoid host LVM commands.

## Validation

Results recorded after checks below. Privileged tests are compiled only and **pending VM**. No privileged tests, LVM reporting/mutation commands, mounts or service operations were executed on this host.

Cargo used pinned Rust 1.98.0 and `--offline` (or `CARGO_NET_OFFLINE=true` for the wrapper), against a copy of the host registry cache in `/tmp/w4a-cargo`. Logs are in this attempt's worker-output `library/`.

| Check | Observed result | Log |
|---|---|---|
| `cargo fmt --check` | exit 0 | fmt.log |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | completed successfully | clippy.log |
| `cargo test --offline --workspace` | failed: W2-C SIGINT cases at tests/w2c/main.rs:572 and :588 observed `running` instead of `failed`/`partial` | tests.log |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; privileged tests compiled, not run | privileged-build.log |
| `tools/ci.sh` | exit 1: source scan, formatting and Clippy passed; stopped on unrelated `client::tests::deadline_and_poisoned_connection` at client.rs:328 | ci.log |
| `cargo test --offline -p tidemark-fs -p tidemark-cli --lib` | exit 0: 34 FS and 60 CLI tests passed, including the client test that failed during CI | scoped-libs-tests.log |
| `cargo test --offline -p tidemark-cli --test w4a` | exit 0: 4 passed | scoped-w4a-tests.log |
| Extra privileged FS/CLI all-target Clippy | exit 101: existing unwrap lint at FS tests/w1a/main.rs:117, outside scope | privileged-clippy.log |
| Privileged FS/CLI library Clippy and CLI W4-A integration Clippy | both exit 0 | privileged-libs-clippy.log; privileged-w4a-clippy.log |

## Remaining uncertainty

All three mandatory privileged scenarios are **pending VM**, including runtime behavior of the reporting oracle and alert integration. No real collection latency measurement was performed. Full workspace tests and CI are not green: the unrelated failures above remain untouched under the task's ownership restriction. Cargo-deny was not reached by CI. Independent verification is not claimed.
