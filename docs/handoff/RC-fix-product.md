# Handoff — RC-fix-product: Operator polish from the guest pass

- **Branch:** precreated canonical attempt branch hp-dad47cce8e4d1c20d544a5c471a51b6e3700b6346975a506236f6fe366f79f3f
- **Agent/session:** Codex / attempt-dad47cce8e4d1c20d544a5c471a51b6e3700b6346975a506236f6fe366f79f3f
- **Date:** 2026-10-13
- **Status:** evidence candidate; item 3 daemon rendering blocked by explicit owned-path scope.

## What changed

1. `api.rs`: old refusal `API socket: Resource temporarily unavailable (os error 11)` becomes `API socket: another tidemarkd is already running with this configuration (API socket lock <path> is held)`. Only a lock-acquisition WouldBlock is translated; other I/O errors and daemon exit handling retain their behavior. Path display uses byte escaping.
2. `catalog.rs`: recovery used to eagerly create `timeseries-recovery-0/` even with no source artifacts. Under existing recovery ownership it now checks the eight fixed database/sidecar names before allocating the private recovery namespace. An empty state gets a fresh rebuilt catalog without an artifact directory; real artifacts retain the existing rename/sync preservation path. Added clean startup/restart test in daemon `lib.rs` and empty/real-recovery test in `crash_recovery.rs`.
3. `render/lvm.rs`: direct text formerly used `size=1073741824 bytes free=1048576 bytes`; now uses `size=1.00 GiB free=1.00 MiB`, through the shared binary formatter. Tracked and other LV sizes use the same formatter. JSON still contains integer bytes. Updated the existing inline `direct_report_fits_width_without_changing_json` test with present VG values; no external snapshot or integration expectation pinned the old direct text in the allowed test paths.
4. `vm_test.rs`, `rpm.rs`, both binary `build.rs`, `.gitignore`, `w8.rs`: vm-test computes the host identity with the RPM resolver, passes both TIDEMARK_BUILD variables to the remote cargo build/test and script invocations, and writes a two-line `.tidemark-build-id` in the synced source tree. Git-free native RPM and build scripts use env → git → file → unknown precedence per field. The file carries the existing tracked-dirty suffix and is ignored. Rsync excludes worktree `.git` files as well as ordinary `.git/` directories. Old guest identity `unknown (unknown)` becomes the host commit/date when supplied. New tests live in `xtask/tests/w8.rs`, including build script pure-function tests compiled as modules.
5. `vm_test.rs`, `w8.rs`: old zero-exit script summaries always said `PASS ... seconds=0`; a successful script emitting a line starting `SKIP:` now gets `SKIP ...`, its own reason line, and increments skipped rather than passed. Nonzero exit always wins over SKIP text. Output still streams through tee and pipefail preserves failures; summaries report passed/skipped counts and all scripts continue after failures.

## Scope conflict / remaining required work

The task requires human sizes for both direct and daemon LVM output, but explicitly owns only `render/lvm.rs` and listed tests. The daemon renderer actually lives in `crates/tidemark-cli/src/render/now.rs`, `lvm_width`: LV size and VG free use `.to_string()`, and the VG line formats `VG {}: {} bytes, {} free`. That path is outside both the textual scope and submission check_scope allowlist. It was not edited. A coordinator-authorized follow-up must replace those cells with `super::size::bytes(...)` (or the already imported shared `fmt_bytes`) and add present-value daemon assertions in `w6a3.rs`. Item 3 is therefore partial; no frozen contract change is needed.

## Shared files and dependencies

No manifest, dependency or module declaration changes. `.gitignore` adds only `.tidemark-build-id`. Binary build scripts are explicitly owned; no new dependencies.

## Focused acceptance evidence

- `held_socket_lock_explains_refusal_without_binding`: passed; pure mapping, no socket bind.
- `clean_first_start_and_restart_do_not_allocate_recovery_directory`: passed; startup catalog seam only.
- `recovery_allocates_one_directory_only_when_preserving_artifacts`: passed; exactly one directory for preserved DB, none when empty.
- `direct_report_fits_width_without_changing_json`: passed; present GiB/MiB values, integer JSON.
- `cargo test --offline -p xtask --test w8`: 12 passed; both build fallback functions, remote identity commands, rootless fake-sudo classification and existing runner tests.

## Validation

All Cargo validation uses the pinned 1.98.0 toolchain, offline mode, and a temporary copy of the host Cargo registry (`CARGO_HOME=/tmp/rc-cargo`), following W0-A's cache approach. Long commands use `timeout 1500`. Full transcripts are in the attempt output `library/`; final results will be recorded below before submission. No privileged operations or VM commands were run; VM behavior remains pending guest confirmation.

### Final standalone checks observed

| Command (offline environment, timeout 1500) | Observed result |
| --- | --- |
| `cargo fmt --check` | exit 0 |
| `cargo clippy --workspace --all-targets -- -D warnings` | final rerun exit 0; initial run found an unused test seam, fixed by exercising it in the unit target |
| `cargo test --workspace --no-fail-fast` | exit 101; sole failing target `tidemarkd --test w6a2s`, sole failing test `retired_watcher_components_and_errors_release_capacity` at line 447 (`left: 2`, `right: 1`), the pre-existing failure identified by the task |
| `cargo build --workspace --features privileged --tests` | exit 0; compile only, no privileged tests executed |

All added regression cases also passed in the full workspace run. This attempt leaves the pre-existing watcher test and all source paths owned by other tasks untouched. CI completion and clean-tree confirmation are recorded in the final addendum below.

### CI addendum

`tools/ci.sh` exited 1 after the same sole pre-existing watcher failure (its inner `cargo test --workspace --no-fail-fast` exited 101). Source checks passed with 150 owner tags and a clean forbidden-symbol scan; formatting, Clippy and strict-ancestry feature checks passed. Cargo-deny was not reached because the test stage failed. No new failures were observed. Guest package identity, second-daemon refusal and real fault-suite behavior remain pending VM confirmation; rootless regressions and privileged-feature compilation are evidenced above.

Only the twelve paths listed in this handoff's change descriptions differ from the attempt baseline. No frozen type, schema, flag, config key, exit-code mapping or metric changed. Final cleanup and clean git status are recorded in the attempt report.
