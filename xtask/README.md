# Workspace tooling

Run `cargo xtask ci` to validate owner tags and read-only symbols, check formatting,
run Clippy and unprivileged tests, and run cargo-deny when installed. Missing
cargo-deny produces a warning. The checks do not rewrite tracked files.

The GitHub `deny` job installs cached cargo-deny 0.20.2 and checks the locked
dependency graph on pushes to `main`, pull requests, manual dispatch, and weekly
for new advisories. It logs the tool version, runs advisories, bans, licenses and
sources separately, and marks the job failed if any category fails while keeping
all category logs available. Its initial findings are non-blocking; make the job
required once those findings are resolved. Use `cargo xtask ci --require-deny` (or
`tools/ci.sh --require-deny`) to fail when cargo-deny is missing; without the flag,
the existing warning and skip behavior remains.

`tools/ci.sh` runs the same checks from the repository root. Set
`RUST_TOOLCHAIN_BIN` to the directory containing the pinned Cargo and Rust tools
when they are not on PATH.

`gen-fixture` and `bench` are reserved for W7-B, and `rpm` for W6-C.
These stubs exit with an error naming their owner.

The source scan parses Rust syntax, excludes `#[cfg(test)]` items and the
`test-support` crate and `tests` directories, and ignores comments and string
literals except command names passed directly to `Command::new`. It checks the
specified symbols, including macro arguments. It is a source-policy check,
not whole-program alias or data-flow analysis.

## Disposable VM tests

Use Rocky Linux **8.8 and 8.10**, x86_64, with XFS root and `/data` on LVM,
a spare disk and enough free space for loop images. Install `xfsprogs
xfsprogs-devel lvm2 bcc-tools lsof`, plus `mdadm`, `rsync`, SSH,
a C compiler/linker and rustup with the pinned toolchain from
`rust-toolchain.toml`. The local machine needs `ssh` and `rsync`. Configure
an SSH alias and a test user with passwordless sudo. These tests create mounts,
loop devices and test VGs; use disposable VMs only.

```sh
cargo xtask vm-test --host rocky810
cargo xtask vm-test --host tester@rocky88 --remote-dir /home/tester/tidemark --filter loop_xfs
cargo xtask vm-test --host rocky810 --suite install
cargo xtask vm-test --host rocky810 --suite fault
```

For a whole run that continues through failing test binaries:

```sh
cargo xtask vm-test --host <vm> --no-fail-fast --env TIDEMARK_DEPLOY_TEST_VM=1 --env TIDEMARK_W1A_MD_DEVICE=/dev/md127
```

`--no-fail-fast` is cargo-only, appears before Cargo's `--` separator, and
applies only to testing. The disposable VM marker is never set implicitly.
`deploy_stage_reboot_verify` prints a skip line unless `TIDEMARK_DEPLOY_VM_PHASE`
is set; stage/verify requires `TIDEMARK_DEPLOY_VM_CONFIG`. `install_vm` prints a
skip line unless `TIDEMARK_INSTALL_RPM` is set. When these variables are set,
the marker must be `1`; other deployment tests always require the marker.
Use `--nocapture` to expose printed lines for passing tests.

Loop devices and the md fixture do not survive a guest reboot and must be
recreated. The SSH client is whichever `ssh` is first on `PATH`; a wrapper
using its own configuration file works. On an SELinux-enforcing guest, tests
starting a binary through systemd must copy it out of the home directory to
an executable location such as `/usr/local/bin` and restore its context first.

The default remote directory is `tidemark-vm-test` relative to the SSH user's
home. The runner creates it and rsyncs the checkout, excluding `target/` and
`.git/`. Use a dedicated directory: rsync replaces matching files but does not
remove remote-only files. The cargo suite first builds privileged tests as the
SSH user, then runs them serially via `sudo env "PATH=$PATH" cargo test`.
Passing PATH explicitly handles EL8 sudo's `secure_path`. Scale tests remain
ignored. `--filter` selects a cargo test substring and is rejected for script
suites. Script suites run `tests/install/*.sh` or `tests/fault/*.sh` under sudo,
and fail clearly if the suite is absent. Script suites always run every script,
then print a PASS/FAIL summary with each exit status and elapsed seconds; any
failure makes the suite exit 1. Output streams directly; cargo-suite subprocess
exit codes propagate to the caller. The fault suite skips slow compaction by
default; opt in with `--env TIDEMARK_FAULT_SLOW=1`. Retain successful fault
evidence with `--env TIDEMARK_FAULT_KEEP=1`. No VM provisioning or host-side privileged
operations are performed by this runner.

## Test parameters and Wave 1 evidence


Repeatable `--env NAME=VALUE` accepts only names matching `TIDEMARK_[A-Z0-9_]+`; reject other names before making any connection. Transport the pairs through SSH and sudo next to PATH: `sudo env "PATH=$PATH" NAME=VALUE ... cargo test ...`. Quote values so spaces and shell metacharacters cannot escape the command. `--nocapture` appends `--nocapture` to the test binary arguments after `--`, exposing passing-test diagnostics. Unit-test accepted/rejected names, a value containing a space, quote and semicolon, and argument positions.

`xtask/README.md` must document these commands, run **inside a disposable VM only**, never on a development host. Install `mdadm` as a VM prerequisite. Choose an unused md device (example `/dev/md127`), and retain the returned loop paths for cleanup:

```sh
fixture_dir=$(mktemp -d /var/tmp/tidemark-md.XXXXXX)
truncate -s 1G "$fixture_dir/member0" "$fixture_dir/member1"
member0=$(sudo losetup --find --show "$fixture_dir/member0")
member1=$(sudo losetup --find --show "$fixture_dir/member1")
sudo mdadm --create /dev/md127 --metadata=1.2 --level=raid1 --raid-devices=2 "$member0" "$member1"
sudo mdadm --detail /dev/md127
```

Invoke the suite from the checkout using the VM’s SSH alias:

```sh
cargo xtask vm-test --host rocky88 --suite cargo --env TIDEMARK_W1A_MD_DEVICE=/dev/md127 --nocapture
cargo xtask vm-test --host rocky810 --suite cargo --env TIDEMARK_W1A_MD_DEVICE=/dev/md127 --nocapture
```

Provision the fixture separately in each VM. After testing, in that VM only, stop the array and remove only these created resources:

```sh
sudo mdadm --stop /dev/md127
sudo mdadm --zero-superblock "$member0" "$member1"
sudo losetup --detach "$member0" "$member1"
rm -f "$fixture_dir/member0" "$fixture_dir/member1"
rmdir "$fixture_dir"
```

Record these Wave 1 findings on both VMs in `docs/handoff/W1-gate.md`; preserve the diagnostic output:

| Finding | Command / evidence |
|---------|--------------------|
| Bulkstat v5 at runtime | `cargo xtask vm-test --host <vm> --filter bulkstat_probe_records_kernel_support --nocapture`; record the printed kernel/v1/v5 result (module inspection is insufficient) |
| `bs_blocks` unit | `cargo xtask vm-test --host <vm> --filter bulkstat_blocks_match_statx --nocapture`; record the measured conversion and all 10,000 `disk_bytes == statx.blocks * 512` comparisons |
| Default I/O scheduler | In the VM, `sudo tidemark probe --json` for resolved leaves and `cat /sys/block/<leaf>/queue/scheduler` for each leaf; the bracketed scheduler is active |


Environment pairs also reach script suites. Empty values are allowed; repeated names
are passed in order (the last value wins). `--nocapture` is cargo-only.

`cargo xtask rpm --native` builds the locked release binaries, bash completion
and RPM directly in a prepared EL8 guest or container, without podman or docker.
It requires cargo-generate-rpm already installed and defaults to `--offline`;
use `--online` to permit Cargo network access, `--version 0.1.1` for an upgrade
package, or `--allow-foreign-host` to explicitly override the EL8 host guard.
`--native` conflicts with `--container`; the default remains the cached container
build. See `packaging/rpm/README.md` for CI and guest preparation commands.

For the install gate, build `TIDEMARK_INSTALL_UPGRADE_RPM` with `cargo xtask rpm --native --version 0.1.1 --upgrade-fixture` inside the prepared EL8 guest. `--upgrade-fixture` requires native mode and version 0.1.1; it selects the test metadata and changed default config under `tests/install/`, whose exact relationship to the production files is checked by xtask tests. The install harness rejects an upgrade package missing the fixture marker and checks config preservation plus `.rpmnew`; use ordinary builds for release packages.
