# Handoff — INSTALL-assert: Discriminating install assertions

## Needs from coordinator

Rerun workspace tests and CI on a host permitting daemon Unix sockets: the workstation workspace run has the five known w6a1 readiness-deadline failures.

Run native package builds and both install phases in disposable Rocky 8.8 and 8.10 guests. Package generation, extraction, systemd lifecycle, RPM conflict handling and SELinux acceptance are pending VM; none was executed on this workstation. Run ShellCheck in equipped CI; it is absent locally.

- **Attempt:** attempt-c4993215e23ae63352bd0ffbad5c206fc03ee8873b075da420cbd0155c97135c
- **Branch:** hp-c4993215e23ae63352bd0ffbad5c206fc03ee8873b075da420cbd0155c97135c
- **Date:** 2026-10-12
- **Status:** implemented; evidence candidate awaiting independent verification

## What changed per part

1. Before erase, the harness enables tidemarkd and requires it to be enabled and active. After erase, it requires inactive/disabled and checks both existence and symlink existence of the multi-user.target wants entry, detecting dangling enablement links.
2. Native RPM parsing accepts `--upgrade-fixture` only with `--native --version 0.1.1`. The generator selects `tests/install/rpm-upgrade.toml`; this metadata differs from the production metadata only in the config source. The fixture config appends exactly `# upgrade fixture 0.1.1`. The new `upgrade_fixture_options_and_drift` test compares entire file bytes after the one permitted replacement/addition, and tests option rejections plus the complete native command list in offline/online modes. The harness extracts only the upgrade config with rpm2cpio/cpio in a private temporary directory and rejects an absent marker before installation. Immediately after upgrade it compares edited live bytes and requires `.rpmnew`, 0600 root:root, with the marker. Script header and both READMEs document the required build option.
3. Before the AVC query, Enforcing is mandatory unless `TIDEMARK_INSTALL_ALLOW_PERMISSIVE=1`; that explicit override prints that SELinux acceptance is NOT tested. Audit error/denial checks continue in either mode.
4. Retained the existing upgrade assertions requiring active service and a nonzero PID different from the pre-upgrade PID. Retained `wait_for_status` with its Bash SECONDS 60-second deadline after upgrade and other starts.
5. `bash -n` and the existing shell syntax/VM guard test pass. Expansions in new commands are quoted; the existing SC2016 annotation explains the inner shell's literal `$1`. ShellCheck is absent, so the script was also reviewed by hand.
6. Extended `xtask/tests/w6c.rs`; the optional workflow change was omitted. No dependencies or shared manifests/module declarations changed.

## Exact coordinator commands

Inside each prepared disposable EL8 guest, with cargo-generate-rpm 0.20.0, the pinned toolchain and locked Cargo cache available:

```sh
timeout 1500 cargo xtask rpm --native --version 0.1.0
tools/check-rpm.sh target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm 0.1.0
timeout 1500 cargo xtask rpm --native --version 0.1.1 --upgrade-fixture
tools/check-rpm.sh target/generate-rpm/tidemark-0.1.1-1.el8.x86_64.rpm 0.1.1
export TIDEMARK_DEPLOY_TEST_VM=1
export TIDEMARK_INSTALL_RPM="$PWD/target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm"
export TIDEMARK_INSTALL_UPGRADE_RPM="$PWD/target/generate-rpm/tidemark-0.1.1-1.el8.x86_64.rpm"
export TIDEMARK_INSTALL_MOUNT=/mnt/tidemark-install-xfs
TIDEMARK_INSTALL_PHASE=install timeout 1500 bash tests/install/install.sh
```

The guest must initially have no tidemark installed and have auditd, python3, rpm2cpio and cpio. Provision the persistent disposable loop-XFS fixture separately. After the external harness reboots and restores the same mount before daemon startup, run from the same checkout, restoring the environment above if needed:

```sh
TIDEMARK_INSTALL_PHASE=verify timeout 1500 bash tests/install/install.sh
```

If restoring the mount after daemon startup, explicitly set `TIDEMARK_INSTALL_RESTART_AFTER_MOUNT=1` for verify as already documented by PKG-upgrade-restart. Do not set `TIDEMARK_INSTALL_ALLOW_PERMISSIVE` for SELinux acceptance. Archive harness output, `/var/lib/tidemark-install/security.txt` and `unit-verify.txt`. A negative guest check should supply an ordinary 0.1.1 RPM and observe the clear fixture-marker rejection before installation. An ordinary and fixture 0.1.1 build share an output filename; keep copies separately when exercising that rejection.

## Assumptions and deviations

- Chose checked-in fixtures with strict drift tests, one of the two explicitly authorized approaches. Production metadata remains untouched. Byte comparison intentionally detects comment/format/asset drift too.
- RPM config payload names use `./etc/tidemark/tidemark.toml`, matching the cpio extraction pattern; actual package inspection remains pending guest validation.
- The readiness deadline retains the existing CLI timeout per call; its wall-clock duration can exceed 60 seconds by the final bounded CLI call.
- Used the precreated attempt branch; no branch creation, merge or push. No privileged host operation, install script execution with VM marker, native RPM build or host configuration write occurred.
- Cargo uses the pinned toolchain directly on PATH and the existing host cache in offline mode. No dependency added. Optional fixture CI job build omitted.

## Validation

Commands use `PATH=/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:$PATH`, `CARGO_HOME=/home/brewerm/.cargo`, `RUSTUP_HOME=/home/brewerm/.rustup`, and `CARGO_NET_OFFLINE=true`. Each long command runs under `timeout 1500`. Full transcripts are in the attempt output `library/`.

| Command | Observed result |
|---|---|
| `cargo fmt --check` | Exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo test --offline -p xtask` | Exit 0; 10 unit tests and 9 w6c tests passed |
| `cargo test --offline --workspace --no-fail-fast` | Exit 101; only failing target `tidemarkd --test w6a1`, 15 passed / 5 failed |
| `tools/ci.sh` | Exit 1; source checks, fmt and Clippy passed; stopped at the same five w6a1 failures |
| `cargo test --offline -p xtask --features privileged --no-run` | Exit 0; install_vm compiles |
| `bash -n tests/install/install.sh` | Exit 0 |
| `git diff --check` | Exit 0 |
| ShellCheck | Not installed; manual review only |

The five workspace failures are `binary_ready_and_stopping`, `lifecycle_reload_and_shutdown`, `corrupt_catalog_is_preserved_before_recovery`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`, at `crates/tidemarkd/tests/w6a1.rs:27` with `deadline exceeded`. They match the task's documented sandbox socket restriction. Native build and install/erase/reboot acceptance remain pending VM; this handoff does not declare release or independent task verification.

CI stopped before its later full-workspace privileged compilation and cargo-deny stages. All task-specific checks passed locally; actual ShellCheck execution and guest acceptance remain pending. Only eight owned files changed; no shared-file edits.
