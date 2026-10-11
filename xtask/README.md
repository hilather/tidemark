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
xfsprogs-devel lvm2 bcc-tools postgresql-server lsof`, plus `rsync`, SSH,
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
