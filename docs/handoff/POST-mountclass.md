# POST-mountclass — exact unwatched-mount classification

## Needs from coordinator

Independent review, uid-1000 umask-022/socket-capable execution and GitHub ubuntu-24.04 validation remain required. Privileged runtime checks remain pending VM; this worker only builds feature-enabled tests. Cargo-deny is not installed, so CI skipped its dependency-policy check; coordinator should run that gate.

## What changed

The shared CLI mapper now receives the requested mount label from the same params object sent to the daemon. All five callers (now, hot, lvm, writers, phantom) pass `params.mount.as_deref()`. An unavailable response is classified as an unconfigured mount only when its message starts with `unavailable: `, followed by that exact requested label and ` is not watched (watch.mounts = `. An omitted mount cannot take that branch. Other errors use the existing `call_error` path.

No daemon text, DTO, schema, exit-code contract, dependency, manifest or shared module declaration changed. Mount resolution and connection ordering remain the same.

## Regression evidence and counts

Before changing production code, updated both existing five-command mapper loops with the reviewer's mount name `/data is not watched (watch.mounts = x`. The absent regression failed on the original mapper: expected ordinary daemon-request context, received the precondition detail instead. The unconfigured marker-name regression passed. The reproduction run had 5 passing tests and 1 failing test (`library/before.log`).

After the fix, retained ordinary `/A` absent and `/B` unconfigured coverage alongside marker-name cases. The absent loop checks 20 combinations (two labels × five commands × both output modes): exit 4, unchanged error context and empty stdout. The unconfigured loop checks 20 combinations: exit 5, unchanged precondition detail, command schema and unavailable error document in JSON mode, empty stdout in human mode. A new negative loop checks 50 combinations: omitted mount, different mount, missing unavailable prefix, leading detail and a subdirectory label must all retain exit 4 and empty stdout.

The focused run passes all 7 mapper tests, with 158 unrelated tests filtered out (`library/after.log`). Tests are socket-free, use no clocks, and make no real-time assertions.

## Before / after output

Let M be `/data is not watched (watch.mounts = x`, METHOD one of the five commands and S its configured socket. The daemon message for configured absence is `unavailable: mount not present: M`.

| Case | Before | After |
| --- | --- | --- |
| Configured absent M, human | exit 5; empty stdout; stderr `METHOD: S: mount not present: M` | exit 4; empty stdout; stderr `METHOD: S: daemon request METHOD: unavailable: mount not present: M` |
| Configured absent M, JSON | exit 5; unavailable error document; same stderr as human | exit 4; empty stdout; same stderr as human |
| Unconfigured M, human | exit 5; empty stdout; original config advice | unchanged |
| Unconfigured M, JSON | exit 5; unavailable error document and original config advice | unchanged |

The unchanged JSON schemas are `tidemark.hot.v1`, `tidemark.now.v1`, `tidemark.phantom.v1`, `tidemark.writers.v1`, and `tidemark.lvm.daemon.v1`. The unconfigured document retains `data.error.code = "unavailable"` and the complete prefixed daemon message. Stderr descriptions include outer CLI context by source inspection; regression tests directly exercise the shared mapper, not a live daemon or unmount.

## Assumptions

- The task's explicit POST scope replaces a wave section assignment; its referenced review and prior handoff are the relevant reading.
- Only an exact diagnostic for an explicitly requested mount is an unconfigured-mount precondition.
- Existing unavailable handling intentionally emits no JSON document.
- Socket-free mapper tests adequately cover this change; no new privileged test is needed.

## Validation

Commands use Rust 1.98.0, host cargo cache `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, offline mode and `timeout 1500` for each long command. Worker uid is 0. The initial Cargo lookup required adding the pinned toolchain to PATH; Cargo fmt additionally required the host RUSTUP_HOME. Neither setup failure changed sources.

Logs live in the attempt output `library/`.

| Check | Result | Log |
| --- | --- | --- |
| `cargo fmt --check` | exit 0; repeated by final CI | `fmt.log`, `ci.log` |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | exit 0; repeated by final CI | `clippy.log`, `ci.log` |
| `cargo test --offline --workspace --no-fail-fast` | first run exit 101 from new assertion debug formatting; final rerun by CI exit 0, no failed targets | `test.log`, `ci.log` |
| `cargo build --offline --workspace --features privileged --tests` | exit 0; repeated by final CI; build only | `privileged-build.log`, `ci.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | exit 0 | `doc.log` |
| `tools/ci.sh` | exit 0; all available checks passed; cargo-deny unavailable and skipped | `ci.log` |
| `cargo test --offline -p tidemark-cli --test format_scan` | exit 0; 1 passed after removing debug formatting from assertion diagnostic | `format-scan-recheck.log` |
| `git diff --check` | exit 0 | worker check |

The first full-workspace run exposed the repository's source-format check rejecting `mount={mount:?}` in the new test assertion. Removed that unnecessary diagnostic formatting and confirmed the target passes. Final CI reran fmt, workspace Clippy, strict-ancestry checks (2 passed), the entire workspace suite and privileged-feature compilation successfully. CLI unit totals: 164 passed, 1 scale ignored; daemon unit totals: 135 passed. The focused mapper total remains 7 passed (90 combinations in the three relevant loops). Nested child-process summaries are not added to these unit totals.

Final cleanup uses `cargo clean --offline`; the commit and clean-status evidence are recorded in the external attempt report.

## Scope and remaining uncertainty

Only the five owned CLI files and this handoff changed. No branch creation, merge, push, host configuration operation or memory projection edit. No shared-file edits or new dependencies. Socket-capable and privileged runtime behavior remain for coordinator gates.
