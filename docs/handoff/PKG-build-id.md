# Handoff — PKG-build-id: installed build identity

- Branch: precreated canonical attempt branch; agent: Codex; date: 2026-10-13.
- Status: implemented within owned paths; evidence candidate, independent verification pending.

## What was built

Both binaries keep `-V` as one line and expose package version, commit/date, and
target/profile through `--version`. Dependency-free build scripts prefer explicit
build environment values, otherwise use Git and mark tracked changes `-dirty`,
otherwise use `unknown`. Cargo watches identity variables, Git HEAD/index/ref,
and tracked source paths so incremental builds refresh identity.
The daemon logs the same identity before descriptor-limit logging.

The RPM task resolves identity once on the host and passes it through native
command environments or container `--env` arguments. `--require-clean` is native
only and rejects tracked changes before a release build. CI exports the first
12 characters of `github.sha`, exports the commit date, uses the clean guard,
and asserts both installed binaries' commit. RPM filename, Version-Release,
assets and manifests are unchanged. Man generation clears the long version so
manuals use the package version throughout.

## Shared-file edits

No manifests, dependencies, module declarations or frozen core types changed.
The only edits to CLI/daemon source are version attributes and startup logging.

## Deviations and remaining uncertainty

- `StatusResult.version` exists, but its producers are in
  `crates/tidemarkd/src/state.rs:893` and `:950`, outside this task's explicit
  ownership. It retains the package version. Coordinator must authorize a small
  follow-up in that file to expose build identity in status JSON; no wire change
  is needed. This requirement is not claimed complete.
- RPM VCS metadata remains unchanged. cargo-generate-rpm 0.20.0 is not cached;
  fetching its source from static.crates.io was rejected by the network allowlist.
  Setting `vcs` with `--set-metadata` could not be confirmed. Coordinator should
  check support on the equipped RPM builder before adding that metadata.
- No native RPM or privileged tests were executed on this host. EL8 RPM build,
  install assertions, and VM runtime checks remain pending.

## Operator checks

Run `tidemark --version` and `tidemarkd --version` on an installed server. The
first line remains `tidemark 0.1.0` or `tidemarkd 0.1.0`; line two is
`commit: <12-character SHA>[-dirty] (<commit date>)`; line three shows target and
profile. `-V` remains appropriate for consumers wanting the package version.
Use `journalctl -u tidemarkd` to inspect the startup identity.

Coordinator: inspect both Rocky 8.8 and 8.10 GitHub `rpm` matrix jobs for the
clean-tree build and installed commit assertions, retain normal RPM checks, and
verify that the first 12 characters match the workflow SHA. No GitHub contact
was made by this worker.

## Validation

All long commands are bounded by `timeout 1500`; Cargo uses the pinned 1.98.0
binary directory, a temporary copy of the host Cargo cache, and offline mode.
Detailed logs are in the canonical worker output `library/`.
Final results are recorded below after the checks finish.

Tests added: pure identity precedence/Git fallback/dirty/unknown cases for both
build scripts; CLI subprocess short/long output; daemon clap short/long contract;
host identity container arguments; native command environment keys; clean option
parsing and rejection before building; CI assertion contracts; deterministic
manuals excluding commit and target metadata.

## Definition of Done

Owned paths only; no new dependencies, unsafe blocks, privileged host actions,
branch creation, merges or pushes. Runtime VM/RPM checks and the status JSON
follow-up remain pending coordinator review.

Observed results: `cargo fmt --check` passed; workspace Clippy with warnings denied
passed; `cargo test --workspace --offline --no-fail-fast` passed (exit 0);
`cargo build --workspace --offline --features privileged --tests` passed.
The final focused serial run passed 3 identity/CLI tests, 13 RPM tests, and 4
manual/daemon-version tests. The initial focused parallel run had a transient
failure in the existing shellcheck-advisory subprocess harness; running that
test alone and the full focused suite serially both passed. The full workspace
run also passed it. The known socket-bind failures were not observed in this
workspace run. Actual binary output is captured in `library/operator-version.log`.
`tools/ci.sh` passed (exit 0): source checks counted 148 owner tags, formatting,
workspace Clippy, workspace tests, and privileged-feature compilation passed.
cargo-deny was not installed, so its audit was skipped by the existing wrapper;
that audit remains pending on an equipped host.
