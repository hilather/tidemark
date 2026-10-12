# W6-C RPM builds

`cargo xtask rpm --container` (or `cargo xtask rpm`) always builds in the cached
`tidemark-rpm:rocky8` image derived from `rockylinux:8`. Podman is preferred to
Docker. Missing tools/images cause errors; the command never pulls an image,
installs tools or fetches Cargo dependencies. Host Cargo is not used to generate
an EL8 package. Output is `target/generate-rpm/`.

On an equipped build host, explicitly provision the image:

```sh
podman build -t tidemark-rpm:rocky8 -f packaging/rpm/Containerfile .
# Explicitly fetch the locked workspace dependencies into the cached image.
podman run --name tidemark-rpm-cache -v "$PWD:/src" -w /src tidemark-rpm:rocky8 cargo fetch --locked
podman commit tidemark-rpm-cache tidemark-rpm:rocky8
podman rm tidemark-rpm-cache
cargo xtask rpm --container
cargo xtask rpm --container --version 0.1.1
```

Use Docker in place of Podman if necessary. Bootstrap downloads are explicit
operator actions; they are never invoked by xtask. The toolchain and generator
are cached in the image. The generator is pinned to 0.20.0; compatibility and
actual package metadata remain build-host gates. Gzip payload supports EL8.
The CLI manifest carries only the empty table required by cargo-generate-rpm;
package metadata comes from `packaging/rpm/rpm.toml`. Asset and script paths
are relative to the workspace root, where the generator command must run. See the
[generator metadata documentation](https://github.com/cat-in-136/cargo-generate-rpm#configuration).
Bash completion is generated from the release CLI before packaging.

Only config/state/runtime are private (0600/0700 root:root); binaries are 0755
and public unit/docs/completion files 0644 root:root. No account is created.

On a fresh disposable Rocky 8.8 or 8.10 VM with auditd, python3, loop-XFS fixture,
Cargo cache and built RPMs, run `cargo xtask vm-test --host VM --filter install`
with `--env TIDEMARK_DEPLOY_TEST_VM=1`, and absolute VM paths via
`TIDEMARK_INSTALL_RPM`, `TIDEMARK_INSTALL_UPGRADE_RPM`, `TIDEMARK_INSTALL_MOUNT`.
Alternatively use `--suite install`. The loop mount must survive the harness
reboot; this test never edits fstab. The install phase runs steps 1–9 and the
existing W4-B stage test. Reboot externally, then invoke the same suite/filter
with `--env TIDEMARK_INSTALL_PHASE=verify` and the VM marker. That phase runs
W4-B's boot-time/hash/core-health/API assertions and `deploy verify --wait 5m`.
Archive `/var/lib/tidemark-install/security.txt` and the harness logs.

## CI

GitHub Actions runs `ci` on every push, pull request and manual dispatch, using
Ubuntu 24.04 and `tools/ci.sh`. The `rpm` job builds on Rocky 8.8 and 8.10 for
pushes to main, `v*` tags, pull requests and manual dispatch. Rocky 8.8 repositories
are pinned to the vault before the first installation. Each build checks the
package metadata and payload, installs it, checks binary versions and `rpm -V`,
and records its SHA-256 checksum.

Run the same preparation, build and inspection by hand from the workspace root
inside a **disposable Rocky 8 container or guest** (preparation runs as root):

```sh
# Choose 8.8 or 8.10 to match the guest. This changes its installed packages.
tools/el8-build-env.sh 8.8
export PATH="$HOME/.cargo/bin:$PATH"
cargo fetch --locked
cargo xtask rpm --native
tools/check-rpm.sh target/generate-rpm/tidemark-0.1.0-1.el8.x86_64.rpm
```

Native builds install nothing and default to locked, offline Cargo builds;
`--online` permits fetching during the build. `--version 0.1.1` builds the upgrade
package. Native mode rejects foreign hosts unless `--allow-foreign-host` is given.

Download the RPM and `.sha256` from the workflow run's artifacts:
`tidemark-el8.8-x86_64` and `tidemark-el8.10-x86_64`. Their packaging version,
release and layout are identical; the two artifacts differ only in the Rocky
release they were built on (binary bytes and checksums may differ). Both use the
same RPM filename, so keep the downloads in separate directories.
