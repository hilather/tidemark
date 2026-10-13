# Handoff — CI-shellcheck-fix: ShellCheck annotations and diagnostics

- **Branch:** hp-b7d4944263330c8e5ea9e8a75207bc72976e1dfdc4d50c9bad17c4293b10c1a8
- **Date:** 2026-10-12
- **Status:** implemented; independent verification pending submission review.

## What changed

Added the requested SC2016 suppression immediately above the reboot writer command: the inner sh must expand its own $1. Reviewed all six scripts by hand; the three RPM scriptlets needed no changes. Added focused annotations in check-rpm.sh for intentional splitting of fixed tag/name pairs (SC2086), and in el8-build-env.sh for guest-provided /etc/os-release (SC1091) and literal yum variables in sed patterns (SC2016). Script edits are comments only; behavior is unchanged.

Extended shell_syntax_and_vm_guard to both tools scripts. ShellCheck failures now report the file, exit status, stdout, and stderr using output().

## Shared-file edits and deviations

None. No dependencies, contracts, module declarations, or packaging behavior changed. The supplied attempt branch was retained as requested.

## Validation evidence

- command -v shellcheck found no executable. ShellCheck review was manual; actual default-severity execution remains pending on a host with ShellCheck.
- bash -n and sh -n passed on all six scripts.
- cargo fmt --check passed.
- cargo clippy --offline -p xtask --all-targets -- -D warnings passed.
- cargo test --offline -p xtask passed: 10 unit tests and 8 integration tests, including shell_syntax_and_vm_guard.
- git diff --check passed.
- Cargo initially was absent from PATH; checks used /home/brewerm/.cargo/bin with CARGO_HOME=/home/brewerm/.cargo and RUSTUP_HOME=/home/brewerm/.rustup, using the pinned toolchain and offline cache.

## Known gaps and reviewer notes

No privileged install, RPM, service, or build-environment script was executed directly. The existing unprivileged test checks the install script's VM guard with its marker removed. VM packaging acceptance remains pending VM, unrelated to these comment/test changes. Full workspace CI was not run; the task requested targeted xtask checks. Submission review and GitHub ShellCheck execution remain outstanding.

## Definition of Done

- [x] Only task-owned paths edited.
- [x] Requested syntax, format, Clippy, and xtask tests passed.
- [x] Handoff records all annotations and local ShellCheck limitation.
- [x] Script behavior unchanged.
- [ ] Independent verification and ShellCheck-enabled CI pending.
