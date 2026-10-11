# Workspace tooling

Run `cargo xtask ci` to validate owner tags and read-only symbols, check formatting,
run Clippy and unprivileged tests, and run cargo-deny when installed. Missing
cargo-deny produces a warning. The checks do not rewrite tracked files.

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
xfsprogs-devel lvm2 bcc-tools postgresql-server lsof`, plus `mdadm`, `rsync`, SSH,
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

The default remote directory is `tidemark-vm-test` relative to the SSH user's
home. The runner creates it and rsyncs the checkout, excluding `target/` and
`.git/`. Use a dedicated directory: rsync replaces matching files but does not
remove remote-only files. The cargo suite first builds privileged tests as the
SSH user, then runs them serially via `sudo env "PATH=$PATH" cargo test`.
Passing PATH explicitly handles EL8 sudo's `secure_path`. Scale tests remain
ignored. `--filter` selects a cargo test substring and is rejected for script
suites. Script suites run `tests/install/*.sh` or `tests/fault/*.sh` under sudo,
and fail clearly if the suite is absent. Output streams directly; subprocess
exit codes propagate to the caller. No VM provisioning or host-side privileged
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
