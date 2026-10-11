# Handoff — W0-A: Workspace scaffold and tooling

- **Branch:** hp-568aef71df25a7ff075396cbc79eb77eaae58d79d889c3e0f3004e150c565f95
- **Agent/session:** Codex, attempt-568aef71df25a7ff075396cbc79eb77eaae58d79d889c3e0f3004e150c565f95
- **Date:** 2026-10-10
- **Status:** implemented; evidence candidate awaiting independent verification

## What was built

Created all ten member crates and xtask with edition 2021, MIT licensing, a committed lockfile, and Rust 1.98.0 pinned in both the toolchain and workspace package metadata. Every crate declares `privileged`; all except tidemark-sys inherit workspace lints and explicitly forbid unsafe code. tidemark-sys uses its own deny-level unsafe policy so future modules can opt in locally.

Created all W0-A stub-map files with their future owner tags and purpose comments, leaving core contracts, command parsing, public stub signatures, and implementations to W0-B and later owners. CLI and daemon are lib + bin skeletons; the CLI binary is named `tidemark`. Added release settings, dependency declarations, cargo-deny policy, the Cargo alias, executable POSIX `tools/ci.sh`, and future-owner command stubs.

CI runs the owner-tag and syntax-aware read-only scans, formatting, Clippy, tests, and cargo-deny when available. Three scanner tests cover forbidden constants/syscalls/LVM commands including macro arguments, comment/string/test exclusions and allowed reporting commands, and external test-module path identification.

## Shared-file edits

| File | Change | Why |
|------|--------|-----|
| Cargo.toml | Created workspace, package defaults, required third-party dependency declarations, lints, release profile | W0-A scaffold |
| crates/*/Cargo.toml | Created package skeletons, privileged features, lint inheritance (sys exception), CLI binary declaration | W0-A scaffold |
| crates/*/src/lib.rs | Created crate documentation, unsafe policy, and module declarations | Compile empty owned modules without introducing signatures |
| xtask/Cargo.toml | Added anyhow, syn, proc-macro2 from workspace | Errors and syntax-aware source checks |

New dependencies beyond the required list:

- `syn`: parse Rust syntax so comments, literals and test items do not cause source-policy false positives.
- `proc-macro2`: inspect macro token trees that are not visited as ordinary Rust AST nodes.

`anyhow` was already required by the plan and is used only in the xtask binary. All other planned dependency entries are retained; member skeletons do not attach unused runtime dependencies yet. Cargo.lock consequently resolves only xtask's five transitive/direct third-party packages. No OpenSSL dependency is present in that resolved graph.

## Deviations from the spec

| Spec reference | Decision | Reason | SCR? |
|----------------|----------|--------|------|
| Toolchain pin | Exact 1.98.0; `cargo 1.98.0 (797e8a9bc 2026-08-05)` | Explicit attempt instruction; Cargo initially absent from PATH | No |
| Dependency fetch | Online fetch blocked by sandbox allowlist; default offline cache lacked anyhow; used temporary copy of host cache and `cargo fetch --offline` | Explicit offline fallback; did not alter the host cache | No |
| Stub module contents | Nested child files compile through private `#[path]` declarations in crate roots; parent mod.rs files remain two doc-comment lines | Attempt requires header-only stub files; W0-B/later owners can relocate declarations into parent modules | No |
| License | MIT | Team choice delegated by W0-A | No |
| Branch/worktree | Used precreated attempt branch without branching, merging or pushing | Explicit attempt instruction | No |

## Acceptance criteria evidence

All commands below used the pinned toolchain directory on PATH, `CARGO_HOME=/tmp/tidemark-w0-cargo`, and offline mode for final validation. Full transcripts are in this attempt's output `library/validation.log`.

| Criterion | Evidence | Status |
|-----------|----------|--------|
| `cargo build --workspace` passes | Exit 0; `Finished dev profile` | Observed locally |
| `cargo xtask ci` passes | Exit 0; `source checks: 72 owner tags; forbidden-symbol scan passed`; `ci: all available checks passed` | Observed locally; clean-tree repeat recorded in output report |
| Rocky Linux 8 container build | Exact command below; no container runtime available per attempt environment | **pending host run** |
| Every stub file names its owner | CI counted 72 source owner tags; all stub-map files have assigned W-task tags | Observed locally |
| Formatting | `cargo fmt --check`, exit 0 | Observed locally |
| Clippy | `cargo clippy --workspace --all-targets -- -D warnings`, exit 0 | Observed locally |
| Unprivileged tests | `cargo test --workspace`, exit 0; scanner `3 passed; 0 failed` | Observed locally |
| Privileged feature compiles | `cargo build --workspace --features privileged --tests`, exit 0 | Observed locally |
| Tooling wrapper | `tools/ci.sh`, exit 0 with `RUST_TOOLCHAIN_BIN` set; executable bit committed | Observed locally |
| Future subcommands | vm-test/W0-C, gen-fixture and bench/W7-B, rpm/W6-C each exit nonzero naming owner | Observed locally |
| Dependency fetch | `cargo fetch --offline`, exit 0 using copied populated cache | Observed locally |
| Deny behavior | `warning: cargo-deny is not installed; skipping cargo deny check` | Allowed skip; advisories/license audit pending equipped host |

Exact container build command, to run on a Docker-equipped host from the checkout root:

```sh
docker run --rm -v "$PWD:/src" -w /src rockylinux:8 /bin/bash -lc 'dnf install -y gcc gcc-c++ make curl ca-certificates && curl --proto "=https" --tlsv1.2 -sSf https://sh.rustup.rs | sh -s -- -y --profile minimal --default-toolchain 1.98.0 && source "$HOME/.cargo/env" && cargo build --workspace --locked'
```

## Tests

- Unprivileged: workspace build, fmt, Clippy, workspace tests, CI, wrapper, and all four stub command owner/error checks passed locally.
- Privileged: feature-enabled test build passed. W0-A defines no privileged tests; VM helpers and smoke tests belong to W0-C — **pending VM**.
- Source scanner tests: `detects_forbidden_symbols_and_commands`, `excludes_tests_comments_and_literals_but_allows_reports`, `identifies_external_test_modules`.

## Measurements

Not a performance task. CI observed 72 crate source files with owner tags and three xtask unit tests.

## Known gaps and follow-ups

- Rocky 8 / glibc 2.28 container compatibility — pending host run of command above.
- cargo-deny actual license/advisory check — pending a host with cargo-deny and advisory database access.
- Full runtime dependency graph is intentionally not resolved by empty members; later owners attach the centrally declared dependencies as needed.
- Core contracts, CLI surface and signatures — W0-B.
- VM runner, fixtures and privileged tests — W0-C.

## Notes for the reviewer

The source-policy scanner is syntactic, not alias/data-flow analysis: literal command names are inspected, comments and ordinary literals are ignored, and cfg(test) items plus external test-module paths are excluded. It also checks macro token arguments. No unsafe blocks, filesystem scanning, runtime metadata/content I/O, or host configuration operations were introduced. The packaging placeholder reflects the retained root-only access decision.

## Definition of Done

- [x] Only owned paths changed; shared-file edits listed above
- [x] `cargo xtask ci` passed locally
- [x] Privileged feature compiles; privileged tests themselves correctly deferred to W0-C
- [x] Public items documented; owner headers present
- [x] Every acceptance criterion has evidence or explicit pending status above
- [x] No unexplained TODOs, unwraps or expects (unwraps only in tests)
- [ ] Rocky Linux 8 container acceptance pending host run
- [ ] Independent task verification pending submission review
