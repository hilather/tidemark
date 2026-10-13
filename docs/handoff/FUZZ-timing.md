# Handoff — FUZZ-timing: generous parser timing guards

- **Branch:** hp-3d7e50d211860c8e705924f891110f31c2b6dbbcf9a26ee8227080c49ee02a84
- **Agent/session:** Codex / attempt-3d7e50d211860c8e705924f891110f31c2b6dbbcf9a26ee8227080c49ee02a84
- **Date:** 2026-10-13
- **Status:** implemented; evidence candidate awaiting independent verification

## What was built

Replaced all eight two-second wall-clock assertions across the four existing fuzz smoke suites with one 30-second helper per file. Each helper explains that the generous bound catches quadratic blowups while allowing loaded CI hosts. Renamed the three tests whose names specified two seconds. Existing inputs, property case counts, and product code are unchanged.

Added `repeated_seed_scales_without_quadratic_blowup` to each file. It constructs exactly 64 KiB and 1 MiB from the same repeated seed, parses each three times, and compares minimum durations. The larger minimum must be strictly less than 200 times the smaller only when the smaller is at least one millisecond. Each timed run also uses the 30-second helper. Input construction is outside the timed region.

The fs check parses repeated mountinfo fixture bytes; CLI parses repeated whitespace through config TOML; daemon directly decodes whitespace as a firstboot report, avoiding fixture disk I/O. Store uses repeated `a/` path components through the existing public catalog seam, which includes disposable SQLite setup because the resolver is not independently exposed. Core, sys, and live have no `tests/fuzz_smoke.rs`; no files were created for them.

## Shared-file edits

None. No dependencies added.

## Deviations from the spec

None in implementation. Used the installed pinned Rust 1.98.0 binaries directly with `CARGO_HOME=/home/brewerm/.cargo` and `RUSTUP_HOME=/home/brewerm/.rustup`; the default rustup home was read-only. Builds used offline mode as requested.

## Acceptance criteria evidence

- All old two-second assertions and names removed from the four owned suites; each file has one documented 30-second helper and one scaling test.
- `cargo fmt --check`: exit 0.
- `cargo clippy --offline --workspace --all-targets -- -D warnings`: exit 0.
- `cargo test --offline --workspace --test fuzz_smoke`: exit 0; CLI 11, fs 5, store 4, daemon 2 tests passed (22 total), including all four new scaling tests.
- `git diff --check`: exit 0.

## Tests

Only unprivileged fuzz smoke tests were requested. No privileged tests or VM operations were added or run; this change only adjusts tests. Full workspace runtime tests and `cargo xtask ci` were not run; workspace/all-target Clippy was run.

## Known gaps and follow-ups

`docs/handoff/W7-C3-fuzz.md:24` still names `large_and_deep_inputs_finish_within_two_seconds_each`, as explicitly required by scope. `fuzz/README.md` mentions the independent libFuzzer two-second execution limit, not the renamed Rust test; it remains unchanged.

Scaling checks sample one parser seam per suite and skip the ratio comparison for sub-millisecond small samples. Store measurements include fixture setup. These are coarse regression guards, not benchmarks or proof of complexity. Independent review and integration remain pending.

## Definition of Done

- [x] Only owned paths changed; no shared-file edits
- [x] Formatting and workspace/all-target Clippy passed offline
- [x] Existing owner headers retained; no unsafe code or product changes
- [x] No new dependencies, TODOs, unwraps, or expects
- [ ] Independent verification pending submission review
