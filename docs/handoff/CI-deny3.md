# Handoff — CI-deny3: Allow the Boost licence

- **Branch:** hp-10a09d2bd9212799176dfbdc2843366c59ddedbf37ef82126d436ca13a57951c
- **Date:** 2026-10-13
- **Status:** implemented; evidence candidate awaiting independent verification

## What changed

Added `BSL-1.0` to `[licenses].allow` in `deny.toml`, with a comment immediately above the list naming `xxhash-rust`. Retained all existing allowances, including BSD-2-Clause, BSD-3-Clause, ISC and CC0-1.0. No dependencies or shared files changed. `.github/workflows/ci.yml` is unchanged; the deny job remains `continue-on-error: true`.

## First results (provided task evidence)

The GitHub run used cargo-deny 0.20.2: advisories passed; bans passed with duplicate-version warnings for `hashbrown` and `syn`; sources passed. Licences rejected only `xxhash-rust` 0.8.19 (through `tidemark-core` 0.1.0), because BSL-1.0 was not explicitly allowed. BSD-2-Clause, BSD-3-Clause, ISC and CC0-1.0 produced `license-not-encountered` warnings.

## Validation and limitations

- Searched `xtask/tests/` and `crates/*/tests/` for `deny.toml`: no matches. Per the task, no Cargo tests or builds were run; no build cleanup was needed.
- The requested cache search (`find ~/.cargo/registry/src -maxdepth 2 -name 'cargo-deny-*'`) failed because that registry source directory does not exist. The warning-suppression key could not be confirmed from cached cargo-deny 0.20 source or documentation, so the non-failing unused-allowance warnings are intentionally retained.
- Local TOML parsing and a scoped diff check confirm only the comment and BSL-1.0 allowance changed in `deny.toml`; all other policy sections are unchanged.
- No local cargo-deny rerun or GitHub contact. The resulting licence audit remains pending an equipped runner. No privileged tests apply to this policy-only change.
