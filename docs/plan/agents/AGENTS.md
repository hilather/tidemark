# AGENTS.md — conventions for coding agents working on Tidemark

> Copy this file to the repository root. Every implementer, reviewer and gate agent reads it first.

Tidemark is a disk-usage intelligence tool for Rocky Linux 8 / XFS servers (Rust workspace). The full plan lives in `docs/plan/`. **Your task is one section of one file in `docs/plan/waves/`.** Read that section and every document it lists under "Read first" before writing code.

## Golden rules

1. **Stay in your lane.** Edit only the paths your task owns. Shared files (workspace `Cargo.toml`, crate `Cargo.toml`, `lib.rs`/`main.rs` `mod` lines) may be touched only to add dependencies or module declarations, and you must list those edits in your handoff note.
2. **Contracts are frozen.** `tidemark-core` types, the snapshot schema (`05`), CLI flags and config keys (`08`), metric names and API methods are contracts. If one is wrong, write a spec-change request (`docs/handoff/SCR-<n>.md`, template in `docs/plan/agents/prompt-templates.md`) and keep working on what doesn't depend on it. **Never silently diverge.**
3. **The spec in `docs/plan/03-link-semantics.md` is normative.** If your numbers disagree with its worked example, your code is wrong.
4. **Never follow symlinks** when scanning (`O_NOFOLLOW`, `AT_SYMLINK_NOFOLLOW`). Never cross filesystems. Skip mount points, including same-device bind mounts.
5. **Never read regular-file contents.** Tidemark is metadata-only. fanotify event fds are only `fstat`-ed and closed.
6. **Open directories with `O_NOATIME`** (fall back without it on `EPERM`) so scans don't cause atime writes on RAID 5.
7. **Paths are bytes.** Use `RawPath`/`RawName`. Never `to_str().unwrap()` a path. Display with `display_lossy()`.
8. **`unsafe` only in `tidemark-sys`.** Every block needs a `// SAFETY:` comment. All other crates are `#![forbid(unsafe_code)]`.
9. **Strictly read-only.** No code may change file metadata, mounts, fstab, LVM or boot configuration (ADR-008). LVM is only read, via the `lvs`/`vgs` reporting commands (never `--readonly`, which hides thin-pool usage; see `07` §1). The only writes are to Tidemark's own state dir, and to its config and service enablement via `deploy stage`.
10. **Bounded everything.** Trees can be millions of directories wide and thousands deep. No recursion proportional to tree depth. No unbounded maps or queues. Channels are bounded. Caps come from config or documented constants.
11. **Be gentle with the disk.** Every metadata syscall in a scan path goes through the `Gate` (`gate.acquire(cost)`). Don't add I/O that bypasses it.
12. **Deterministic output.** Sort by name bytes where order matters. Inject clocks (`now: i64` parameters) instead of calling the system clock inside logic you want to test.

## Rust conventions

- Edition 2021; toolchain pinned in `rust-toolchain.toml`.
- `cargo fmt`; `cargo clippy --all-targets -- -D warnings` must be clean.
- Errors: `thiserror` in libraries, `anyhow` only in binaries and tests. No `unwrap()` / `expect()` outside tests, except for provably infallible cases with a comment.
- Logging: `tracing` macros. No `println!` in libraries. User-facing output happens only in `tidemark-cli` render code.
- Public items have rustdoc. Every source file starts with `//! Owned by: W<task-id>` and a one-line purpose.
- Hot loops (per directory entry): no per-entry heap allocation where avoidable; reuse buffers; prefer `SmallVec` and `rustc-hash` maps.
- Concurrency: `std::thread`, `crossbeam`, `parking_lot`. **No async runtime.**
- Dependencies come from `[workspace.dependencies]`. Adding a new crate requires a line of justification in the handoff note. Don't add a crate that pulls in OpenSSL.

## Tests

| Kind | Command | Notes |
|------|---------|-------|
| Everything unprivileged | `cargo test --workspace` | Must pass in your sandbox |
| Lints and format | `cargo xtask ci` | Runs fmt, clippy, tests, deny |
| Privileged (root, loop XFS/LVM, fanotify, deploy + reboot) | `cargo xtask vm-test --host <vm>` | Compiled only with `--features privileged` (no `#[ignore]`); scale tests use `#[ignore = "scale"]` |

- Use `test-support` fixtures and oracles (`du`, `find`, `lvs`, `df`) rather than hard-coding filesystem-dependent numbers.
- If you can't run privileged tests (no VM access), still write them, make sure they compile with `--features privileged`, and mark them `pending VM` in your handoff note.
- **Never** run destructive commands on the host outside temp directories. Never modify `/etc`, mount anything, touch LVM, or enable services on the machine you're running on. Those actions belong in VM tests only.

## Definition of Done

See `docs/plan/10-agent-playbook.md` §5. In short: owned paths only, fmt/clippy clean, tests pass, privileged tests written, acceptance criteria evidenced, handoff note written (`docs/plan/agents/handoff-template.md` → `docs/handoff/<task-id>.md`).

## Commits

- Branch `wN/<task-id>-<slug>`. Small, focused commits. Message format: `<task-id>: <imperative summary>`.
- Don't rewrite history on shared branches. Don't merge other task branches into yours. Rebase on `integration/wave-N` only if the coordinator asks.
