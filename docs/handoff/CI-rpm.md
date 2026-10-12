# Handoff — CI-rpm: Rocky 8.8 and 8.10 package CI

## Needs from coordinator

Run the commands below in disposable Rocky 8.8 and 8.10 guests before the owner's
first push. RPM generation, real RPM query behavior, installation and privileged
runtime checks cannot be exercised on this workstation. Rerun workspace tests and tools/ci.sh outside this socket-restricted sandbox;
workspace tests stopped on the five known w6a1 readiness deadlines. Check the chosen official
action major tags before pushing; no GitHub contact or push occurred here.

## What was built

Added native RPM builds with pure option parsing, injected os-release validation,
and an ordered command list for locked release compilation, completion generation
and the existing single-source generator arguments. Default/container builds keep
the cached, no-pull behavior. Native mode refuses foreign hosts and missing generators,
installs nothing, and defaults to offline compilation.

Added a POSIX package inspector deriving the asset list, modes, ownership and flags
from rpm.toml, checking identity, exact file inventory, systemd dependency, scriptlet
bodies/interpreters and extracted ELF executables. Temporary payloads are removed.
Added an idempotent disposable EL8 preparation script with root/EL8 guards, Rocky 8.8
vault pinning before dnf, required build packages, pinned rustup installation if
Cargo is missing, and generator 0.20.0 installation if missing.

The workflow runs `ci` and a separate two-release `rpm` matrix, with read-only
repository permissions, ref concurrency cancellation, 45-minute job timeouts,
locked fetching, native builds, package checks, install/version/verification checks,
checksums and separate artifact names. Documentation gives the matching manual flow.

## Shared-file edits and dependencies

None. No manifests, CLI/API contracts, plan documents or crate sources changed.
The existing xtask dispatch needed no change. The Containerfile remains unchanged.

## Reproduce in guests (pending VM)

Use a checkout of this candidate in each disposable guest. Run as root, from the
workspace root. Preparation changes the guest's repositories and installed packages;
do not run this on a developer workstation. For the **8.8 guest**, run exactly:

```sh
tools/el8-build-env.sh 8.8
export PATH="$HOME/.cargo/bin:$PATH"
cargo fetch --locked
cargo xtask rpm --native
tools/check-rpm.sh target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm
dnf install -y ./target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm
tidemark --version
tidemarkd --version
rpm -V tidemark
sha256sum target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm > target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm.sha256
```

For the **8.10 guest**, run exactly:

```sh
tools/el8-build-env.sh 8.10
export PATH="$HOME/.cargo/bin:$PATH"
cargo fetch --locked
cargo xtask rpm --native
tools/check-rpm.sh target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm
dnf install -y ./target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm
tidemark --version
tidemarkd --version
rpm -V tidemark
sha256sum target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm > target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm.sha256
```

Repeat preparation to confirm idempotence. Preserve the printed release, glibc,
gcc, rustc and generator versions. For upgrade construction use
`cargo xtask rpm --native --version 0.1.1` and pass `0.1.1` as the inspector's second
argument. The daemon supports `--version` (its exported clap parser enables it).

## Assumptions and choices

- Explicit task ownership and the precreated attempt branch supersede the general
  wave branch convention. No branches, merges, pushes or GitHub calls were made.
- Official major tags chosen without lookup: **checkout v6, cache v5,
  upload-artifact v6**. Coordinator must confirm current tags and runner/container
  runtime compatibility. No dependency was added, including no YAML parser.
- `rpm` is the job ID specified by this task, implementing the plan's `build-el8`
  role. Workflow triggers all pushes for `ci`, with a job condition limiting RPM
  push builds to main and v tags; both jobs also run on manual dispatch.
- Known EL8 IDs are rocky, rhel, centos, almalinux and ol, with major version 8.
  EL8 preparation arguments should match the disposable guest's release.
- Cargo installed already is assumed equipped with the pinned toolchain; an
  existing generator is retained as requested and its version is printed.
- Cache restore follows preparation in the explicitly requested order, so first
  provisioning can still install the generator before a cached binary is restored.
- Inspector uses EL8 coreutils plus rpm/rpm2cpio/cpio. Script bodies preserve their
  shebang; comparison normalizes terminal newlines only, while checking /bin/sh
  separately. Actual generator/RPM behavior needs guest confirmation.
- No shellcheck executable was available. The only multi-line YAML shell logic
  is the required pre-checkout 8.8 vault recipe; remaining commands are script
  invocations or individual steps.

## Validation

| Command | Result |
|---------|--------|
| timeout 1500 cargo fmt --check | Exit 0 |
| timeout 1500 cargo clippy --offline --workspace --all-targets -- -D warnings | Exit 0 |
| timeout 1500 cargo test --offline --workspace | Exit 101: five known w6a1 readiness deadlines |
| timeout 1500 cargo build --offline --workspace --features privileged --tests | Exit 0: compile only |
| timeout 1500 tools/ci.sh | Exit 1: same five w6a1 readiness deadlines; source checks, fmt and Clippy passed |
| timeout 1500 cargo test --offline -p xtask --test w6c | Exit 0: 7 passed in final run |
| sh -n tools/check-rpm.sh and sh -n tools/el8-build-env.sh | Both exit 0 |
| git diff --check | Exit 0 |

The workspace test run completed 44 test-result blocks: **492 passed, 5 failed,
13 ignored**, before stopping in w6a1. These are partial-run counts, not a full
workspace pass. Failed tests: binary_ready_and_stopping,
corrupt_catalog_is_preserved_before_recovery, lifecycle_reload_and_shutdown,
private_and_missing_state_directories_start and startup_reads_last_complete_from_catalog.
Each reported deadline exceeded at w6a1.rs:27, consistent with the documented
sandbox limitation; no attempt was made to change daemon code. Full workspace
baseline counts were not measured; the owned test suite before/after counts are
reported below. Full logs are in the attempt output library.
Rust 1.98.0 was placed on PATH. Commands used the host registry/git cache copied
into /tmp/tidemark-ci-cargo, with CARGO_NET_OFFLINE=true and --offline where
applicable. Every long command ran under timeout 1500. No environment preparation,
native build, container runtime, sudo, dnf, mounts or services ran on the host.

Baseline w6c source defines four unprivileged tests (the prior PKG handoff also
records four passing); the changed suite defines seven, all seven observed passing.
The additional tests cover native parsing/conflicts/version validation, command
order/locked/offline behavior, injected EL8/foreign os-release text and workflow
contracts plus sh -n for both new scripts. Privileged w6c adds its existing install
VM test; no new privileged test is needed for pure command construction. All real
RPM execution and install tests remain pending VM.

A temporary mock RPM-query harness ran the inspector end to end with a cpio
payload holding real ELF binaries: valid metadata passed and an unexpected file
was rejected. This proves shell flow, not real package/generator compatibility.
Its transcript is check-rpm-mock.log in the attempt output library.

## Only a real GitHub run can confirm

- Trigger/job filtering for branch pushes, v tags, pull requests and dispatch.
- Superseded-ref concurrency cancellation and 45-minute timeout enforcement.
- Docker Hub matrix image pulls and GitHub's container default user/HOME/PATH.
- Official action tag availability, Node runtime compatibility with EL8 glibc,
  checkout after git/tar bootstrap, and GITHUB_PATH propagation.
- Cache save/restore permissions and hit behavior for registry/git/generator/target.
- Network availability for vault, dnf, rustup and crates.io on the hosted runners.
- Artifact upload/download, separate matrix names, retained checksums and package bytes.
- The complete hosted build/install/verification sequence on both matrix entries.

This handoff is an evidence candidate, not independent verification or release acceptance.

## Final evidence and scope

The CI wrapper's source scan found 140 owner tags and passed the forbidden-symbol
check. Its workspace run again completed 492 passing tests, five failing daemon
readiness tests and 13 ignored tests before stopping. cargo-deny and the wrapper's
later privileged stage were not reached; privileged compilation passed separately.
No test failure was repaired outside task scope. Final w6c run: **7 passed, 0 failed,
0 ignored**, versus the four baseline unprivileged tests; existing privileged install
coverage remains pending VM. No full baseline workspace run was performed.

Only the eight task-owned deliverable files changed. No shared-file edit or new
dependency was necessary. Submission records the candidate for coordinator review;
workspace/CI green status and actual RPM runtime acceptance are not claimed.
