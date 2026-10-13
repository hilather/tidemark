# Handoff — CI-deny: Run dependency policy on GitHub

- Branch: precreated hp-f6a9e791b4428793d26164fab736fb7e435da388311101aebe5da2f59e22cb47
- Agent/session: Codex, canonical attempt f6a9e791b4428793d26164fab736fb7e435da388311101aebe5da2f59e22cb47
- Date: 2026-10-13
- Status: implemented; evidence candidate pending independent review and first GitHub run

## What was built

Added an independent Ubuntu 24.04 deny job for pushes to main, PRs, manual dispatch and Monday 06:23 UTC advisory checks. It uses the same pinned-toolchain installation as ci, caches the cargo-deny binary with version 0.18.4 in the key, installs with `cargo install cargo-deny --locked --version 0.18.4` on cache misses, fetches the locked graph, logs the tool version, and streams complete `cargo deny check` output. The job has contents-read permissions, a 20-minute timeout, and initial continue-on-error with the required transition comment.

Added `cargo xtask ci --require-deny`: a missing tool now errors when requested, while default behavior still warns and skips. Pure unit tests cover absent/accepted/unknown/repeated arguments and all installed/required decision combinations without invoking cargo-deny. README documents the job and flag, including wrapper passthrough.

## Scope and deviations

Only .github/workflows/ci.yml, xtask/src/main.rs, xtask/README.md and this note changed. No shared manifest/module declarations, dependencies, contracts or deny.toml changes. The task is a follow-up to W0-A tooling; its historical read-first sections and handoff template were read. Used the provided branch, no branching/pushing/GitHub contact. Initial formatting/metadata commands hit the isolated Rustup home restriction; corrected RUSTUP_HOME to the existing host installation. Validation uses pinned toolchain 1.98.0, existing /home/brewerm/.cargo cache and CARGO_NET_OFFLINE=true.

The coordinator must confirm cargo-deny **0.18.4 exists** and installs with the pinned toolchain. Its availability was deliberately not checked over the network. Existing push triggers remain intact; the deny job filters pushes to main. The added schedule also triggers existing workflow jobs.

## Manual policy review

`cargo metadata --offline --format-version 1` succeeded for all 294 packages (284 registry and 10 workspace packages); no licenses were inaccessible. Cross-checked cached manifests against Cargo.lock. Full license/source inventory is retained in the attempt output library/licenses.json.

Expected license failures:

| Crate | License expression | Uncovered requirement |
|---|---|---|
| xxhash-rust 0.8.19 | BSL-1.0 | BSL-1.0 |
| terminfo 0.9.0 | WTFPL | WTFPL |
| finl_unicode 1.5.0 | (MIT OR Apache-2.0) AND Unicode-DFS-2016 | Unicode-DFS-2016 |
| wezterm-bidi 0.2.3 | MIT AND Unicode-DFS-2016 | Unicode-DFS-2016 |

Other uncovered alternatives need not fail: ryu offers Apache-2.0 OR BSL-1.0; termina offers MIT OR MPL-2.0; r-efi offers MIT OR Apache-2.0 OR LGPL-2.1-or-later; aho-corasick/globset/memchr offer Unlicense OR MIT. Several WASI/rustix crates offer Apache-2.0 WITH LLVM-exception OR Apache-2.0 OR MIT, allowing an already accepted branch. Legacy slash-separated MIT/Apache expressions occur; actual tool normalization/license-text confidence may produce additional findings.

Proposed policy decisions, not applied: review and explicitly approve BSL-1.0 and Unicode-DFS-2016 if acceptable; separately decide whether to allow WTFPL or replace/remove its dependency path. Keep deny.toml unchanged until real findings and owner policy review.

Expected duplicate-version warnings (bans has no explicit multiple-version policy): bit-set 0.5.3/0.11.1; bit-vec 0.6.3/0.10.1; bitflags 1.3.2/2.13.2; cpufeatures 0.2.17/0.3.1; getrandom 0.3.4/0.4.3; hashbrown 0.16.1/0.17.1; nix 0.29.0/0.31.3; r-efi 5.3.0/6.0.0; rand 0.8.8/0.10.3; rand_core 0.6.4/0.10.1; syn 1.0.109/2.0.119/3.0.7; thiserror 1.0.69/2.0.21; thiserror-impl 1.0.69/2.0.21. No proposed blanket exclusions: inspect the first dependency paths and deduplicate only where compatible.

All external lockfile sources are registry+https://github.com/rust-lang/crates.io-index. No Git/non-default registry sources or openssl-sys; source restrictions and the explicit OpenSSL ban should pass. Advisory findings cannot be predicted from cached manifests: current database access and actual cargo-deny execution remain pending GitHub. No policy-check success is claimed.

## Validation

Required checks: cargo fmt --check; cargo clippy -p xtask --all-targets -- -D warnings; cargo test -p xtask --offline. All three checks exited 0; tests passed 20 unit + 9 w6c + 6 w7b + 3 w7c + 3 w7d = 41 tests. Results are recorded in the attempt report/library. Tests include unchanged w6c workflow string assertions. No privileged behavior was added; VM tests are not applicable. No cargo-deny invocation was performed locally.

## Follow-ups

Coordinator: confirm the pinned release exists; run the workflow on GitHub, review complete findings, resolve approved dependency/policy changes, then remove continue-on-error and require the deny status check. Independent verification and integration remain pending submission review.
