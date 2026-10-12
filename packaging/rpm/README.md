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
