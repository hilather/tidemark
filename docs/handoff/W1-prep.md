# Handoff — W1-prep: Wave 1 shared-file preparation

- **Branch:** existing hp-0f38eb4acd26a51d5ca7ebfdc28fb72df35c94d1d9abfc8f13f67527f404145c
- **Agent/session:** canonical attempt attempt-0f38eb4acd26a51d5ca7ebfdc28fb72df35c94d1d9abfc8f13f67527f404145c
- **Date:** 2026-10-11
- **Status:** implemented; evidence candidate awaiting independent verification.

## What was built

Prepared all requested dependency attachments without implementing Wave 1 behavior. StoreError now lives in W1-E-owned error.rs and remains re-exported at tidemark_store::StoreError with NotYetImplemented unchanged. Added the W1-B walk benchmark entry and empty main, and the private W1-C gen module stub. No unsafe code or benchmarking crate was added.

## Shared-file edits

| File | Change | Why |
|------|--------|-----|
| Cargo.toml | Seven internal workspace path entries; rustix system and insta json features | Central dependency inheritance and probe support |
| Cargo.lock | Offline resolution of newly attached dependencies | Reproducible Wave 1 builds |
| Four owned crate Cargo.toml files | Runtime/dev attachments and privileged forwarding; fs walk bench | Avoid shared manifest changes during parallel implementation |
| tidemark-store/src/lib.rs | Private error module and public re-export | Resolve gate S3 without changing the public error path |
| tidemark-store/src/error.rs | Relocated unchanged enum, W1-E ownership | W1-E can extend errors in its owned file |
| tidemark-sys/src/lib.rs and src/gen/mod.rs | Private gen module declaration and W1-C stub | Generated binding namespace |
| tidemark-fs/benches/walk.rs | Empty benchmark entry, W1-B ownership | Compilable harness-free target |

## Dependency inventory

All entries below use workspace inheritance. Existing entries were retained, including store's dev serde_json; CLI core was normalized from a direct path to workspace inheritance. No requested attachments were omitted and no additional dependency beyond the requested lists was needed. Reasons justify the newly attached crates; rows also document retained dependencies.

### tidemark-sys

| Kind | Dependency | Purpose |
|------|------------|---------|
| Runtime | `libc` | makedev and syscall/FFI constants |
| Dev | `test-support` | Fixture DSL and privileged loop helpers |
| Dev | `tempfile` | Isolated test state |

### tidemark-fs

| Kind | Dependency | Purpose |
|------|------------|---------|
| Runtime | `tidemark-core` | Frozen shared types and contracts |
| Runtime | `thiserror` | Typed library/CLI errors |
| Runtime | `globset` | Walker exclusions |
| Runtime | `crossbeam-channel` | Bounded walker output |
| Runtime | `tidemark-sys` | XFS, fanotify and ioprio calls |
| Runtime | `libc` | makedev and syscall/FFI constants |
| Runtime | `rustix` | Safe filesystem, proc metadata, statvfs, directory and host wrappers |
| Runtime | `crossbeam-deque` | Walker work stealing |
| Runtime | `parking_lot` | Throttle synchronization |
| Runtime | `smallvec` | Avoid hot-loop allocations |
| Runtime | `rustc-hash` | Fast bounded metadata maps |
| Runtime | `serde` | Report/storage serialization derives |
| Runtime | `serde_json` | Probe JSON and snapshot metadata |
| Runtime | `tracing` | Diagnostic logging |
| Dev | `test-support` | Fixture DSL and privileged loop helpers |
| Dev | `tempfile` | Isolated test state |
| Dev | `proptest` | Generated parser/path/storage cases |
| Dev | `insta` | Probe JSON snapshots |

### tidemark-store

| Kind | Dependency | Purpose |
|------|------------|---------|
| Runtime | `tidemark-core` | Frozen shared types and contracts |
| Runtime | `thiserror` | Typed library/CLI errors |
| Runtime | `rusqlite` | Bundled SQLite persistence |
| Runtime | `serde` | Report/storage serialization derives |
| Runtime | `serde_json` | Probe JSON and snapshot metadata |
| Runtime | `rustix` | Safe filesystem, proc metadata, statvfs, directory and host wrappers |
| Runtime | `chrono` | ISO-week retention arithmetic |
| Runtime | `tracing` | Diagnostic logging |
| Dev | `serde_json` | Probe JSON and snapshot metadata |
| Dev | `test-support` | Fixture DSL and privileged loop helpers |
| Dev | `tempfile` | Isolated test state |
| Dev | `proptest` | Generated parser/path/storage cases |

### tidemark-cli

| Kind | Dependency | Purpose |
|------|------------|---------|
| Runtime | `tidemark-core` | Frozen shared types and contracts |
| Runtime | `clap` | CLI argument parsing |
| Runtime | `clap_complete` | Existing completion support |
| Runtime | `anyhow` | CLI error context |
| Runtime | `thiserror` | Typed library/CLI errors |
| Runtime | `tracing-subscriber` | Existing CLI logging initialization |
| Runtime | `serde` | Report/storage serialization derives |
| Runtime | `serde_json` | Probe JSON and snapshot metadata |
| Runtime | `humantime` | Existing CLI duration rendering |
| Runtime | `tidemark-fs` | Filesystem probe composition |
| Runtime | `tidemark-pg` | CLI Postgres probe composition |
| Dev | `assert_cmd` | CLI command tests |
| Dev | `tempfile` | Isolated test state |
| Dev | `test-support` | Fixture DSL and privileged loop helpers |
| Dev | `insta` | Probe JSON snapshots |

## Features and privileged arrangement

Each of the four crates defines `privileged = ["test-support/privileged"]`. Cargo accepts this forwarding to the non-optional dev dependency, so test-support remains dev-only; no production dependency edge or fallback was necessary. The three requested test-build commands validate workspace ordinary/privileged and isolated fs privileged compilation.

Workspace rustix retains fs/process and adds system for safe uname host facts used by W1-A. Its fs feature already covers statvfs and directory reads, so no extra filesystem feature is needed. Workspace insta adds json for W1-A JSON snapshots. Existing rusqlite bundled, serde derive and chrono std-only feature sets are unchanged. The empty private gen module needs no allow attributes today; W1-C can place narrow generated-code lint attributes inside its owned module when bindings arrive.

## Deviations from the spec

No contract or dependency substitutions. Builds use pinned Rust 1.98.0, a temporary copy of the host registry cache at /tmp/tidemark-w1-prep-cargo, and CARGO_NET_OFFLINE=true (direct commands additionally use --offline), consistent with W0-A. Host cache is unchanged. No branches, merges, pushes, mounts, LVM mutations or service changes were performed.

## Acceptance criteria evidence

Validation transcripts and exit files are in this attempt's worker-output/library directory. An initial fmt check caught one trailing blank line in relocated error.rs; it was removed and fmt rerun. Final outcomes are recorded below.

| Check | Local result | Evidence |
|-------|--------------|----------|
| `cargo fmt --check` | Exit 0 | library/fmt-final.log |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 | library/clippy.log |
| `cargo test --offline --workspace` | Exit 0 | library/test.log |
| `cargo build --offline --workspace --tests` | Exit 0 | library/build-tests.log |
| `cargo build --offline --workspace --features privileged --tests` | Exit 0 | library/build-privileged.log |
| `cargo build --offline -p tidemark-fs --features privileged --tests` | Exit 0 | library/build-fs-privileged.log |
| `RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps` | Exit 0 | library/doc.log |
| `tools/ci.sh (offline environment)` | Exit 0 | library/ci.log |


The bundled SQLite C compilation succeeded. The walk bench is included in all-target Clippy. Existing storage contract tests compile and pass against the unchanged re-export.

## Tests and known gaps

Only unprivileged tests were executed. Privileged tests were compiled, not run: pending VM. No new privileged runtime behavior was introduced by this preparation task. CI may skip cargo-deny when the tool is unavailable; that skip is disclosed below and is not an advisory/license audit result. Wave 1 runtime acceptance, generated binding layouts and benchmark measurements belong to their respective implementation tasks.

Workspace tests: 80 passed, 0 failed, 1 existing scale test ignored. tools/ci.sh exited 0, but explicitly skipped cargo-deny because it is not installed. Advisory/license auditing remains pending an equipped host.

## Definition of Done

- Owned paths only; shared-file edits listed above.
- Dependency attachments, feature forwarding, error ownership and both stubs prepared.
- Local format, lint, tests, build and documentation evidence recorded above.
- New public error rustdoc and source owner headers retained; no new unwrap/expect or unsafe blocks.
- Independent verification and integration remain coordinator work.
