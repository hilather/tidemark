# Handoff — W6-C: Packaging, systemd unit, RPM and install tests

## Needs from coordinator

Provision an equipped Rocky 8 RPM build host and disposable Rocky 8.8/8.10 VMs.
Real RPM generation, metadata inspection, service/security/SELinux behavior and
reboot verification remain pending; local results below are evidence candidates.

- Branch: canonical precreated attempt branch; no branching, merge or push.
- Attempt: attempt-213d778e4902e7f5c3dfdf1f084bfdbd3ebfada7162982f9c4d5b10de71a7f8f
- Date: 2026-10-11
- Status: implementation with local validation; pending build-host and VM gates.

## What was built

Shipped the exact normative systemd unit, runtime tmpfiles entry and commented
example configuration. Implemented container-only RPM construction with external
metadata, root-owned assets, private config, config(noreplace), EL8 gzip payload,
scriptlets and generated Bash completion. Added a guarded ten-step VM install
script, two-phase W4-B reboot integration, and five unprivileged packaging tests.
The unit was compared line by line with 08 §6; the exact-text and parsed-directive
test enforces this comparison, including comments and every directive.

## Public interfaces

All are in xtask/src/rpm.rs; they are binary-internal public functions, not new
core/API contracts:

- `pub const IMAGE: &str = "tidemark-rpm:rocky8"`
- `pub fn generate_rpm_args(version: &str) -> anyhow::Result<Vec<String>>`
- `pub fn container_args(root: &std::path::Path, version: &str) -> anyhow::Result<Vec<std::ffi::OsString>>`
- `pub fn run(root: &std::path::Path, args: impl Iterator<Item = String>) -> anyhow::Result<()>`

`rpm [--container] [--version 0.1.0|0.1.1]` always builds inside the cached Rocky 8
image, preferring Podman and falling back to Docker. No implicit image pull,
tool installation or dependency fetch occurs. Missing tools/image/generator
produce installation/provisioning guidance. See packaging/rpm/README.md for the
explicit build-host bootstrap and offline-cache steps. Pure builders validate
version before tool discovery; source mount paths preserve OS bytes.

## Shared-file edits and dependencies

None. No manifest, lockfile, runner, core contract or plan changed. All changes
are within owned paths. No new Rust dependency.

## Assumptions and choices

- A pre-provisioned image derived from `rockylinux:8` satisfies EL8 build isolation
  while honoring the prohibition on silent downloads. The optional --container
  spelling is accepted; both invocations use the same container build. Explicit
  Containerfile bootstrap uses rustup 1.98.0 and cargo-generate-rpm 0.20.0, cached
  in the image. Version 0.20.0 includes the noreplace/scriptlet fix according to
  [upstream releases](https://github.com/cat-in-136/cargo-generate-rpm/releases).
  No bootstrap was executed here.
- Metadata lives in packaging/rpm/rpm.toml and is supplied through documented
  generator metadata overrides; no Cargo manifest changes are needed. The upgrade
  switch changes RPM version to 0.1.1 without changing frozen workspace manifests.
- The baseline contains no root README.md. An owned operator README is shipped as
  /usr/share/doc/tidemark/README.md, matching unit Documentation. The three plan
  documents ship with their existing filenames.
- Binaries are 0755; unit/tmpfiles/docs/completion are 0644. Every asset is root:root;
  private configuration is 0600 and runtime/state are 0700. No account/group is
  created. This follows ADR-018's sensitive-data modes rather than preventing
  administrators from reading public docs or executing the CLI.
- The existing VM runner sends --filter install to Cargo, and --suite install to
  scripts. The privileged `install_vm` Cargo test invokes the same guarded script,
  supporting both entry points without editing the runner.
- Reboots belong to the external VM harness. The install phase runs 1–9, reinstalls
  the upgrade package and calls W4-B's exact stage test. After a harness reboot,
  the verify phase calls W4-B's exact boot-time/hash/core/API test and the CLI
  `deploy verify --wait 5m`. The harness supplies a persistent disposable loop-XFS
  mount and RPM files already present on the VM; rsync excludes target/.
- The daemon exposes no state-directory override. Its --check-config path only
  loads/validates and exits, so the test invokes the built daemon with the exact
  packaged config; it performs no state writes. xtask has no direct core dependency.

## Acceptance evidence and tests

- `exact_normative_unit_and_config`: exact plan blocks, parsed section/directive
  equality, prohibited group directives absent, disabled HTTP/webhook, tmpfiles.
- `metadata_and_scriptlet_contract`: all ten asset destinations, modes, root:root,
  private config(noreplace), version, exact post/postun and erase-only preun guard.
- `pure_commands_and_invalid_arguments`: generator arguments, safe container mount,
  cached image/no-pull, offline locked release build, generated completion and
  rejected unsupported/malicious version/arguments without container execution.
- `example_config_validates_with_built_daemon`: builds tidemarkd offline and runs
  --check-config against the packaged config; exit 0 locally.
- `shell_syntax_and_vm_guard`: bash -n on install and all scriptlets, optional
  shellcheck, absent VM marker refuses before any filesystem/service action.

## Pending VM or build host

- `cargo xtask rpm --container` and `--version 0.1.1`: actual glibc 2.28 binaries,
  generated completion, generator integration and installable EL8 RPMs.
- `rpm -qlp`: exact asset layout comparison in VM script, and actual owner/mode/
  config flags (`rpm -qplv`, `rpm -qp --configfiles`) on the equipped host.
- Privileged `install_vm` / tests/install/install.sh on Rocky 8.8 and 8.10: fresh
  install stays stopped/disabled; default config validates; minimal loop-XFS
  config starts and exposes API status; separate-unit 50 MiB write appears in hot;
  no tidemarkd AVCs; 0.1.1 retains edited config bytes/mode/owner; erase stops and
  disables while retaining a state sentinel; stage/reboot/verify returns healthy
  firstboot and confirms newer boot time. Polls use state checks and deadlines.
- `systemd-analyze security tidemarkd`: script captures output under
  /var/lib/tidemark-install/security.txt for attachment; no score claimed locally.
- shellcheck is absent on this host; bash syntax checks passed. The test runs
  shellcheck automatically if available.

## Validation

Required check results are recorded below after final execution. All commands
use Rust 1.98.0, --offline or CARGO_NET_OFFLINE=true, and the existing host Cargo
cache. Full logs and exit codes are under this attempt's output library/.
No privileged commands or script body, mounts, LVM mutation, service changes,
installation, host /etc writes, image build/pull or dependency downloads ran.

## Remaining uncertainty

No real RPM or VM acceptance is claimed. The baseline daemon lifecycle remains
owned by W6-A; install runtime tests intentionally require its completed behavior.
Independent verification and integration belong to the coordinator. CI's optional
cargo-deny check does not establish auditing when cargo-deny is absent.

## Definition of Done evidence

- Owned paths only; no shared-file edits or dependencies; public items documented.
- Exact unit/config and root-only metadata evidenced by unprivileged tests.
- Privileged install entry point feature-gated and compiled; execution pending VM.
- No unexplained TODOs, unsafe code or production unwrap/expect added.
- Acceptance requiring RPM/build host, VM/security output and reboot remains pending.

## Final local check results

| Check | Exit | Evidence |
|---|---|---|
| cargo fmt --check | 0 | library/fmt.log; final repeat in fmt-final.log |
| cargo clippy --offline --workspace --all-targets -- -D warnings | 0 | library/clippy-final.log; initial test-helper lint failure preserved in clippy.log |
| cargo test --offline --workspace | 0 | library/test.log: 477 passed, zero failed, 13 existing scale ignores |
| cargo build --offline --workspace --features privileged --tests | 0 | library/privileged.log; compilation only |
| RUSTDOCFLAGS='-D warnings' cargo doc --offline --workspace --no-deps | 0 | library/doc.log |
| tools/ci.sh, offline inherited | 0 | library/ci.log: 139 owner tags, source checks passed, all available checks passed |
| cargo test --offline -p xtask --test w6c | 0 | library/focused.log: five passed, none ignored |
| cargo clippy --offline -p xtask --all-targets -- -D warnings | 0 | library/focused-clippy.log |
| cargo xtask rpm --container | 1, expected provisioning failure | library/rpm-missing.log; no image built or tool installed |

Cargo-deny was unavailable and CI explicitly skipped it; dependency/advisory
inspection remains pending an equipped host. No new test is ignored. Final source
changes were also compiled for xtask's privileged feature (privileged-final.log)
and formatting rechecked (fmt-final.log). Real RPM and VM success is not claimed.
