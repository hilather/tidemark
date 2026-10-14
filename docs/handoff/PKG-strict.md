# Handoff — PKG-strict: packaged ancestry policy

## Needs from coordinator

Run the Rocky RPM matrix and guest ancestry refusal checks below. Full RPM build/install timing and payload execution are not observable on this workstation; independent verification remains pending.

## What changed (R4-01)

The store's additive `strict-ancestry` feature forces strict admission regardless of compiler assertions or runtime environment. CLI and daemon forward this feature without changing defaults. The truth table is feature OR assertions-disabled OR runtime-strict. Existing state policy tests retain strict/development cases; `policy_feature_table` adds all eight compiler/runtime combinations, and `feature_forces_strict_with_assertions` calls the compiled feature-enabled helper in an assertions-enabled test build. Removing the feature term fails these regressions.

Both native and container RPM build commands select both binaries and pass `--features strict-ancestry`. Native command-list and container-script tests fail if either build drops it. The API socket directory already uses the same helper (`api.rs:573`); it was read, not edited.

Both executable `--version` outputs gain the fourth ancestry line; short `-V` remains unchanged. CLI subprocess tests check the fourth line and runtime strict switch. Because daemon library version code is outside scope, its entry point extends the existing clap command only on invocations containing `--version`, retaining clap's parsing and validation. Library command metadata remains three lines. No normal daemon startup path changes.

The payload checker runs both extracted binaries with the runtime strict variable unset and requires the exact strict line. The installed-package workflow asserts it for both binaries, then Rocky 8.10 performs a second full native package build with release assertions enabled and checks the rebuilt payload and binaries. This chooses the full-package regression rather than the alternative small compile. Workflow/checker contract tests detect missing gates. Runtime and timing evidence for that second build is pending CI within the existing 45-minute limit.

## Shared-file edits

Only feature lines were added to the three owned crate manifests. No defaults, dependencies, module declarations, lockfile or frozen contracts changed. No plan/security files changed.

## Assumptions

- This assigned review repair supersedes the older wave task scope; W6-C packaging and its read-first sections supplied context.
- Executing `--version` from a trusted generated RPM is appropriate on its compatible EL8 builder, as requested; inspection never installs the payload.
- The second release build can reuse cached dependencies and fits the existing job limit; confirm this on the actual runner. The 8.10 artifact is the assertions-enabled strict build; the installed first package is verified before rebuilding.
- The existing unprivileged RPM guard test invoked the install script. Within the owned test file, it now inspects the VM marker text and retains bash syntax checks, respecting this attempt’s script-execution restriction. Guest install execution remains feature-gated.
- Development defaults continue to allow the documented warning-bearing exceptions. A feature-enabled debug build is intentionally strict and may reject developer namespace layouts.

## Proposed plan and security text

“RPM builds enable the store's `strict-ancestry` feature through both binaries. State and socket ancestry exceptions are disabled independently of debug assertion settings and runtime environment. `--version` reports the effective policy. Development builds without the feature retain the exceptions only when assertions are enabled and `TIDEMARK_STRICT_ANCESTRY` is not `1`.”

## Guest and builder checks (not run here)

On prepared Rocky 8.8 and 8.10 builders:

```sh
umask 022
export CARGO_NET_OFFLINE=true
timeout 1500 cargo xtask rpm --native --require-clean
timeout 1500 tools/check-rpm.sh target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm
# Disposable guest only: install the package through the existing rpm job.
env -u TIDEMARK_STRICT_ANCESTRY tidemark --version | grep -Fx 'ancestry checks: strict'
env -u TIDEMARK_STRICT_ANCESTRY tidemarkd --version | grep -Fx 'ancestry checks: strict'
CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true timeout 1500 cargo xtask rpm --native --require-clean
timeout 1500 tools/check-rpm.sh target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm
```

Run the installed-package unmapped-owner refusal fixture in `docs/reviews/W7-review4-security.md` under “For the VM run”, with runtime strict unset, against both packages. It must exit 1, name the unmapped owner, and create no state. Also run real read-only bind/writable-alias refusal and root-owned real/separate-mounted-state positive controls in the guest; these are not established by the pure table. Existing privileged trust tests compile here but execute only in a guest:

```sh
timeout 1500 cargo test --offline -p tidemark-store --features privileged,strict-ancestry --test state_trust -- --nocapture --test-threads=1
```

## Validation

Results and test counts are recorded below. Logs are in this attempt's worker-output `library/`. Commands use the pinned 1.98.0 toolchain, a temporary copied Cargo cache, offline mode, `umask 022`, and `timeout 1500` per long command. No privileged tests or host installation/service/mount commands were executed.

Observed results: workspace tests passed with summaries totaling 902 passed, 0 failed, 14 ignored (84 summaries, including subprocess helpers and capability-guard returns). The known readiness failures did not surface; this does not establish execution of socket/namespace bodies. Formatting, workspace Clippy, privileged-feature Clippy, and privileged test-target compilation passed. The explicit debug feature regression passed 2 tests; the focused serial suite passed 4 version tests and 14 RPM tests.

The actual store crate also passed both feature regressions under:

```sh
CARGO_TARGET_DIR=/tmp/pkg-strict-release CARGO_PROFILE_RELEASE_DEBUG_ASSERTIONS=true \
  timeout 1500 cargo test --offline --release -p tidemark-store \
  --features strict-ancestry --lib strict_feature_tests
```

This used the runtime strict variable absent, verified `cfg!(debug_assertions)` was true, and completed the separate cold release build in 4m35s. It is supplemental actual-library evidence, not a full RPM build or full-job timing measurement. The Rustflags override was not separately compiled; the policy table covers the same assertions dimension.

`tools/ci.sh` passed (exit 0): source policy (148 owner tags), formatting,
workspace Clippy, the explicit feature regression, workspace tests and privileged
compilation passed. Its summaries total 904 passed, 0 failed, 14 ignored,
including the two feature regressions. Cargo-deny was absent and explicitly
skipped; no dependency audit is claimed. Real ShellCheck was unavailable;
bash syntax and existing subprocess harness tests passed.

The final formatting check and privileged-feature Clippy repeat passed after
the install-guard test adjustment. A build selecting both binaries with
`--features strict-ancestry` passed; both actual executables reported the exact
strict line with the runtime variable unset. Development binaries were also
observed reporting relaxed by default and strict with the runtime switch;
both short versions remained one line. Version transcripts are retained in
`development-versions.log`, `versions.log` and `strict-versions-check.log`.

No VM, native/container RPM build, package installation, host service action,
mount operation or privileged test execution occurred. Full RPM checks,
installed foreign-ancestry refusal, real read-only alias layouts and pipeline
timing remain pending coordinator/guest validation. Source signatures used by
other tasks were retained. The result is an evidence candidate awaiting
independent verification. One task commit and build cleanup follow validation.
