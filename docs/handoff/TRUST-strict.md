# Handoff — TRUST-strict: strict shipped ancestry and checked purge descendants

## Needs from coordinator

Run the namespace regressions in ordinary-user CI and the privileged second-uid regression in a disposable Rocky guest. Run the packaged-binary command below after installing the RPM. Local full-suite/CI outcomes are recorded below. Namespace creation is unavailable here, so namespace acceptance still needs CI. Independent verification remains pending.

## What changed

R3-01: `state_dir.rs` takes strictness explicitly in its pure policy. Release builds always enforce root/euid ancestry; development builds allow the existing overflow-uid and read-only-mount exceptions with one process warning. `TIDEMARK_STRICT_ANCESTRY=1` enables strict development behavior. Refusals name the ancestor and identify the overflow/unmapped owner or foreign read-only owner. Sticky ancestors require owned children without group/world write. Storage leaf ownership, descriptor `/proc/self/fd/<n>/.` handling and external call signatures remain unchanged. The API socket ancestry exception uses the same strictness helper and warns with the actual opened ancestor path.

R3-02: both purge walks validate every opened directory descriptor before constructing its entry reader: directory type, effective-uid ownership, no group/world write and the state mount id. This includes unknown and nested descendants. Preflight runs before service stop or unlink. Attachment identity is checked immediately after open and again before each walk iteration; the existing post-walk identity check remains. Directory paths and the existing depth cap bound retained traversal state. Leaf symlinks and other non-directories remain entries unlinked without following them.

## Acceptance evidence

- `state_dir::tests::explicit_integrity_policy_table`: ancestor/sticky-child/storage roles exercise strict and development modes, including both exception refusals. Without gating, strict exception rows accept the hostile ancestry.
- `state_dir::tests::readable_storage_warns_once`: separately counts the readability and development-exception warnings; repeated accepted foreign ancestry emits one exception warning.
- `strict_namespace_ancestry_refuses_before_creation` in CLI rules: namespace binary with strict variable must exit 1, report unmapped owner 65534, and leave state absent. It skips with the existing one-line namespace notice when unavailable. `namespace_mounts_are_skipped` retains development behavior.
- `strict_ancestry_refuses_second_uid_before_writes` in e2e lifecycle (privileged feature): a child executes mkdir as uid/gid 65533 without useradd; root CLI refuses its foreign parent in strict mode, leaves state absent, preserves sentinel bytes/inode/mode/uid/size. Pending guest execution.
- `purge_checks_unknown_and_nested_directories_before_stop`: unknown group-writable, nested world-writable and nested simulated foreign-owned directories all refuse with the path, no fake service calls and identical complete tree bytes/modes.
- `purge_after_open_rename_preserves_moved_contents`: a path-selected test hook moves the just-opened unknown directory outside state before first use; refusal precedes service calls/unlinks and preserves its sentinel and state marker. Without the immediate identity check, preflight follows the moved fd.

## Shared-file edits and dependencies

None. No manifests, module declarations, dependencies, frozen contracts, packaging scripts or files outside scope changed. `api.rs` changes only the ancestry exception block. Added `state_dir::strict_ancestry()` without changing existing signatures.

## Assumptions and deviations

- Coordinator decisions supersede the review's request to eliminate development exceptions. Development binaries remain explicitly unsuitable for hostile ancestry unless strict mode is enabled. The specified namespace/second-uid tests and dual-mode policy table are the regression set implemented here; an actual read-only-bind/writable-alias SQLite reopen race was not exercised or instrumented.
- The feature-gated second-uid test requires guest root, coreutils mkdir and an unused numeric uid 65533; no account creation is required. It forks/sets uid through `CommandExt`, matching the existing harness approach.
- No privileged-feature tests, host mounts, real service actions or install/fault scripts were executed. Namespace tests in the required unprivileged suite only attempted the existing sandbox-aware namespace probe.
- Toolchain binaries and cached dependencies are under `/home/brewerm/.rustup` and `/home/brewerm/.cargo`; validation sets PATH, RUSTUP_HOME and CARGO_HOME accordingly and `CARGO_NET_OFFLINE=true`.

## Pending guest commands

Compile/run only in a disposable guest as root:

```sh
CARGO_NET_OFFLINE=true timeout 1500 cargo test --offline -p tidemark-cli --features privileged --test e2e -- --exact scan_lifecycle::strict_ancestry_refuses_second_uid_before_writes --nocapture
```

Ordinary-user CI (namespace and socket support):

```sh
umask 022
CARGO_NET_OFFLINE=true timeout 1500 cargo test --offline -p tidemark-cli --test rules -- --nocapture
```

Exact packaged-binary proof (disposable guest root, installed RPM; strict variable deliberately absent):

```sh
set -eu
fixture=$(mktemp -d /tmp/tidemark-strict-XXXXXX)
trap 'rm -rf -- "$fixture"' EXIT
chmod 1777 "$fixture"
setpriv --reuid 65533 --regid 65533 --clear-groups mkdir -m 755 "$fixture/foreign"
mkdir -m 755 "$fixture/root"
printf '[general]\nsocket = "%s/absent-api.sock"\n' "$fixture" > "$fixture/config.toml"
printf 'sentinel\n' > "$fixture/sentinel"
cp "$fixture/sentinel" "$fixture/sentinel.before"
set +e
env -u TIDEMARK_STRICT_ANCESTRY unshare --user --map-root-user /usr/bin/tidemark \
  --config "$fixture/config.toml" --state-dir "$fixture/foreign/state" \
  scan "$fixture/root" > "$fixture/stdout" 2> "$fixture/stderr"
code=$?
set -e
cat "$fixture/stderr"
test "$code" = 1
grep -F 'owner 65534 is not mapped in this user namespace' "$fixture/stderr"
test ! -e "$fixture/foreign/state"
cmp "$fixture/sentinel.before" "$fixture/sentinel"
```

The same command is an artifact `library/packaged-strict-guest.sh`, syntax checked only. Packaging already invokes `cargo build --release` (`xtask/src/rpm.rs`) and selects `target/release` binaries (`packaging/rpm/rpm.toml`); no packaging edit was needed.

## Proposed plan and security text

Plan: Shipped binaries enforce root/euid-owned state ancestry irrespective of user-namespace mappings or read-only mount flags. Writable ancestors must be sticky with an owned child without group/world write. Development exceptions emit a one-time warning and can be disabled with `TIDEMARK_STRICT_ANCESTRY=1`. A retained descriptor caller vouches for ancestry. Purge preflights every opened directory's owner, mode, type and mount before service stop, and repeats checks on descent during deletion.

Security: Unmapped owners and read-only mounts do not prove immutable ancestry; another host identity or writable alias can rename entries before SQLite pathname reopens. Release builds refuse both exceptions. Purge accepts no foreign-owned or group/world-writable descendants, even unknown names. Immediate attachment checks stop deterministic after-open replacement before traversal; this does not claim protection against a malicious effective uid/root concurrently changing its own trusted private directories.

## Validation

All commands use offline Cargo and `timeout 1500`; the workspace test/CI shell explicitly sets `umask 022`.

| Command | Observed result |
|---|---|
| `cargo fmt --check` | Passed on final sources |
| `cargo clippy --workspace --all-targets -- -D warnings` | Passed; CI also checks final sources |
| `cargo clippy --workspace --all-targets --features privileged -- -D warnings` | Passed on final sources |
| `cargo test --workspace --no-fail-fast` | Exit 0: 848 passed, 0 failed, 14 ignored across 79 result summaries (includes nested subprocess summaries) |
| `cargo build --workspace --features privileged --tests` | Passed on final sources; tests were not executed |
| `tools/ci.sh` | Exit 0: all available checks passed; 848 passed, 0 failed, 14 ignored across 79 test summaries; cargo-deny unavailable/skipped |
| Guest script `bash -n` | Passed; script not executed |

Focused final checks: purge 4 passed (including both new regressions); state policy 6 passed; CLI rules 3 passed statuses with both namespace bodies skipped. The rules `--nocapture` log records `namespace cannot be created: unshare: write failed /proc/self/uid_map: Operation not permitted` twice. A direct local binary check with strict mode returned exit 1, named the unmapped owner 65534 at `/`, and created no state directory.

The task forecast five sandbox failures in w6a1; this execution instead reported all 30 w6a1 tests as passed. This is the observed harness outcome, not guest or namespace acceptance. Evidence logs are retained under the attempt's output `library/`.

Final CI source-policy scan passed (148 owner tags), formatting and unprivileged Clippy passed on final sources. Dependency advisory/license/source denial checks remain pending a host with cargo-deny installed. No privileged or packaged-binary runtime acceptance is claimed.
