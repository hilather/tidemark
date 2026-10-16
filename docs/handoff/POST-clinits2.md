## Needs from coordinator

- Independently review this evidence candidate and run the socket-capable uid 1000 / umask 022 and GitHub ubuntu-24.04 gates. Local execution is uid 0; no socket server is needed by the new tests.
- Privileged runtime remains pending VM; only compile privileged tests here. Run cargo-deny on an equipped host if the CI wrapper skips it.

# Handoff — POST-clinits2

## What changed

Daemon-mode phantom/writers now select the configured socket as their outer error context, matching lvm/now/hot even when `--mount` is present. Explicit `--direct` still selects the mount, or `/proc` without a mount. The daemon-absent branches add `direct fallback: <command>: <mount or /proc>` around errors from their direct invocation. Keeping the command name preserves the explicit-direct diagnostic as the final suffix, preserving a useful subject and the underlying error chain and exit classification.

Only the four owned Rust files and this handoff changed. `now.rs` changes are tests only. No dependency, manifest, module declaration, contract, success output, direct scanner, timing, or scan behavior changed.

## Reproduction and regression evidence

Before the production change, `cargo test --offline -p tidemark-cli --lib context_tests -- --nocapture` reported 1 passed / 2 failed: `daemon_mount_contexts_are_consistent` observed `phantom: /B` instead of `phantom: /run/tidemark/api.sock`; `fallback_errors_name_the_direct_subject` lacked the `direct fallback: phantom: /proc` context. The original focused prefix run separately reported 0 passed / 1 failed. Logs: `library/baseline.log`, `baseline-contexts.log`.

After the change, all three context tests pass. The first full workspace run exposed the existing `phantom_and_writers_fallback_notice_and_direct_setup_seam` suffix assertion; the fallback wrapper was refined to include the command name so the complete explicit-direct error remains the final suffix. No existing test was edited. They cover five commands with and without `--mount`, explicit direct contexts with and without a mount, and actual daemon-absent fallback calls for phantom/writers with and without a mount. Phantom fails resolving a missing watched mount; writers receives a zero interval through its internal Args to fail before any scan or sleep. These paths never reach the clock. The diagnostic test composes the real outer prefix with the real daemon-error mapper or missing-path resolver for all five commands; 1 passed before and after. Existing tests continue checking JSON and exit codes. Logs: `after-contexts.log`, `baseline-lines.log`, `after-lines.log`.

## Before/after output

These are socket-free composed diagnostics from `daemon_mount_diagnostic_lines`, not a claim of a live-daemon VM reproduction. Normalize the temporary missing path as `M = /tmp/<fixture>/missing`, and use `S = /run/tidemark/api.sock`. Let `D` be `is not watched (watch.mounts = ["/A"]); add it to [watch].mounts and run: tidemark deploy stage --from <file>` and `E` be `No such file or directory (os error 2)`. Full unabridged lines are in the logs above.

| Command | Unwatched before | Unwatched after |
|---|---|---|
| phantom | `phantom: /B: /B D` | `phantom: S: /B D` |
| writers | `writers: /B: /B D` | `writers: S: /B D` |
| lvm | `lvm: S: /B D` | `lvm: S: /B D` |
| now | `now: S: /B D` | `now: S: /B D` |
| hot | `hot: S: /B D` | `hot: S: /B D` |

| Command | Missing mount before | Missing mount after |
|---|---|---|
| phantom | `phantom: M: resolve mount: M: E` | `phantom: S: resolve mount: M: E` |
| writers | `writers: M: resolve mount: M: E` | `writers: S: resolve mount: M: E` |
| lvm | `lvm: S: resolve mount: M: E` | `lvm: S: resolve mount: M: E` |
| now | `now: S: resolve mount: M: E` | `now: S: resolve mount: M: E` |
| hot | `hot: S: resolve mount: M: E` | `hot: S: resolve mount: M: E` |

Direct fallback failure lines below use the tested inner errors and dispatcher prefixes; baseline outer lines are reconstructed. `library/binary-fallback-after.log` additionally records actual binary stderr for phantom and writers with an absent socket and explicit missing mount, plus unchanged explicit-direct stderr (all exit 1). `T` denotes the fixture's absent socket path. The standard `daemon unavailable; falling back to direct <method> mode` notice remains unchanged.

| Command / injected failure | Before | After |
|---|---|---|
| phantom, no mount, missing configured watch path | `phantom: T: resolve path: M: E` | `phantom: T: direct fallback: phantom: /proc: resolve path: M: E` |
| phantom, explicit missing mount | `phantom: M: resolve path: M: E` | `phantom: T: direct fallback: phantom: M: resolve path: M: E` |
| writers, explicit missing mount | `writers: M: resolve path: M: E` | `writers: T: direct fallback: writers: M: resolve path: M: E` |
| writers, no mount, internal zero interval | `writers: T: interval must be positive` | `writers: T: direct fallback: writers: /proc: interval must be positive` |
| writers, mount M, internal zero interval | `writers: M: interval must be positive` | `writers: T: direct fallback: writers: M: interval must be positive` |
| lvm / now / hot | No direct fallback in this task | Unchanged |

The zero interval is an internal failure injection; normal CLI `--interval` requires `--direct`. Explicit direct errors keep their existing outer context and gain no fallback wrapper.

## Assumptions

- Use the precreated attempt branch, offline host Cargo cache, and pinned Rust 1.98.0. No branching, merging, pushing, memory-projection changes, or privileged runtime.
- This follow-up task supersedes the prior handoff's dispatcher scope restriction. Existing frozen APIs, schemas, flags and exit codes remain unchanged.
- Missing-path resolution and synthetic remote errors provide deterministic socket-free presentation evidence. Coordinator gates cover a live daemon and other execution environments.
- No new privileged test is needed for this presentation change. Existing privileged suites are compiled only, pending VM. No fake clock is needed because the new failure injections stop before any timestamp or sleep.

## Validation

All long commands used `timeout 1500`, offline host cache (`CARGO_HOME=/home/brewerm/.cargo`, `CARGO_NET_OFFLINE=true`) and Rust 1.98.0. `RUSTUP_HOME=/home/brewerm/.rustup` was set so cargo-fmt could locate the installed toolchain. Logs are in this attempt's `library/` output directory.

| Command | Final local result | Log |
|---|---|---|
| `cargo fmt --check` | Exit 0 | `fmt.log` |
| `cargo clippy --workspace --all-targets -- -D warnings` | Exit 0 | `clippy.log` |
| `cargo test --workspace --no-fail-fast` | Exit 0; 87 harness summaries, 1,031 passed / 0 failed / 14 ignored | `tests.log` |
| `cargo build --workspace --features privileged --tests` | Exit 0; compilation only | `privileged-build.log` |
| `RUSTDOCFLAGS='-D warnings' cargo doc --workspace --no-deps` | Exit 0 | `doc.log` |
| `tools/ci.sh` | Exit 0; 150 owner tags; forbidden-symbol scan passed; all available checks passed | `ci.log` |

Counts above sum harness summaries, including nested child summaries, rather than claiming unique tests. The CI log has 88 summaries, 1,033 passed / 0 failed / 14 ignored. CLI unit suite: 163 passed. Focused final context run: 3 passed; diagnostic lines: 1 passed; existing fallback integration: 1 passed. Initial full run: 1,030 passed / 1 failed / 14 ignored; its sole failure is preserved in `initial-workspace-tests.log` and passes after the suffix refinement. Formatting and Clippy were repeated after that refinement.

Cargo-deny is not installed; the wrapper explicitly skipped its audit. No dependency was added. Privileged runtime is pending VM, and uid 1000 / umask 022, GitHub runner and independent review remain coordinator work. This is an implementation/evidence candidate, not independent verification or integration approval.

Only owned paths changed; no shared manifest or module-declaration edits. `git diff --check` passed. The deliverable is committed with the requested task prefix, followed by `cargo clean` and a clean-tree check before submission.
