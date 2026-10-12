# Handoff — PKG-upgrade-restart

## Needs from coordinator

Rerun workspace tests and CI where Unix sockets are allowed: the workstation workspace run has five existing w6a1 readiness-deadline failures, matching the documented sandbox limitation.

Run the install/upgrade/erase/reboot gate in disposable Rocky 8.8 and 8.10 guests. No package installation or service operation was run on this workstation. Guest acceptance remains pending VM.

- **Branch:** hp-d28f7d17e6302115f3ad04d3b954db88483ca83caf77b6c3f0618795aafeedbd
- **Attempt:** attempt-d28f7d17e6302115f3ad04d3b954db88483ca83caf77b6c3f0618795aafeedbd
- **Date:** 2026-10-12
- **Status:** implemented; evidence candidate awaiting independent verification

## What changed

`postun.sh` reloads systemd, then runs `systemctl try-restart tidemarkd.service || :` only when `[ "$1" -ge 1 ]`. Erase never takes the restart branch. Install/post and erase/preun are unchanged. Enablement is unchanged by upgrades.

The install harness checks effective hardening and memory properties, an active upgrade's PID change and renewed API readiness, a stopped reinstall, and saved config bytes/mode after erase. Reboot verification now requires a write event from the restored watched filesystem. The optional restart is explicit and logged. Packaging/readme and rollout each document restart behavior; wave-6 install item 8 covers the upgrade/reinstall assertions. The scriptlet contract test compares the entire guarded postun script and explicitly forbids restart in post.

## New install-script assertions and exact commands

### Install steps 4–5

The helper `wait_for_status` retries `tidemark status` with `deadline=$((SECONDS + 60))`, checking `(( SECONDS < deadline ))` and sleeping one second after failures. It replaces the original readiness loop and is reused after upgrade, manual start, and the optional post-mount restart.

```bash
properties=$(systemctl show tidemarkd -p MemoryMax -p MemoryLimit -p NoNewPrivileges -p User -p Type -p CapabilityBoundingSet)
grep -Eq '^(MemoryMax|MemoryLimit)=2147483648$' <<< "$properties"
grep -Fxq 'NoNewPrivileges=yes' <<< "$properties"
grep -Fxq 'User=root' <<< "$properties"
grep -Fxq 'Type=notify' <<< "$properties"
expected_caps=$(awk -F= '/^CapabilityBoundingSet=/ {print tolower($2)}' /usr/lib/systemd/system/tidemarkd.service | tr ' ' '\n' | LC_ALL=C sort)
actual_caps=$(sed -n 's/^CapabilityBoundingSet=//p' <<< "$properties" | tr '[:upper:]' '[:lower:]' | tr ' ' '\n' | LC_ALL=C sort)
[[ -n $expected_caps && $actual_caps == "$expected_caps" ]]
systemd-analyze verify /usr/lib/systemd/system/tidemarkd.service > /var/lib/tidemark-install/unit-verify.txt 2>&1
cat /var/lib/tidemark-install/unit-verify.txt
if grep -Fi 'tidemarkd.service' /var/lib/tidemark-install/unit-verify.txt | grep -Ei 'ignoring|Invalid'; then
    exit 1
fi
```

Either memory property must equal exactly 2147483648 bytes (2 GiB), rather than infinity. Capabilities are compared as sorted lowercase names because systemd's display order/case can differ from the unit. `systemd-analyze verify` must also exit successfully; its installed-unit warnings are rejected case-insensitively.

### Upgrade step 8

```bash
before_pid=$(systemctl show tidemarkd -p MainPID --value)
[[ $before_pid =~ ^[0-9]+$ && $before_pid != 0 ]]
dnf upgrade -y "$TIDEMARK_INSTALL_UPGRADE_RPM"
systemctl is-active --quiet tidemarkd
after_pid=$(systemctl show tidemarkd -p MainPID --value)
[[ $after_pid =~ ^[0-9]+$ && $after_pid != 0 && $after_pid != "$before_pid" ]]
wait_for_status
systemctl stop tidemarkd
dnf reinstall -y "$TIDEMARK_INSTALL_UPGRADE_RPM"
[[ $(systemctl show tidemarkd -p ActiveState --value) == inactive ]]
systemctl start tidemarkd
wait_for_status
```

The existing version, config byte, ownership and mode checks remain after the upgrade/reinstall sequence.

### Erase step 9

```bash
cmp /etc/tidemark/tidemark.toml.rpmsave /var/lib/tidemark-install/edited.toml
[[ $(stat -c '%a' /etc/tidemark/tidemark.toml.rpmsave) == 600 ]]
```

The existing inactive/disabled and state-retention checks remain.

### Reboot verify phase

After `tidemark deploy verify --wait 5m`, verify requires a supplied mount and repeats the install phase's restricted path/exact loop-XFS mount assertions:

```bash
: "${TIDEMARK_INSTALL_MOUNT:?Supply restored disposable loop-XFS mount}"
[[ $TIDEMARK_INSTALL_MOUNT =~ ^/[a-zA-Z0-9_/-]+$ && $TIDEMARK_INSTALL_MOUNT != / ]]
[[ $(findmnt -n -o FSTYPE --target "$TIDEMARK_INSTALL_MOUNT") == xfs ]]
[[ $(findmnt -n -o TARGET --target "$TIDEMARK_INSTALL_MOUNT") == "$TIDEMARK_INSTALL_MOUNT" ]]
[[ $(findmnt -n -o SOURCE --target "$TIDEMARK_INSTALL_MOUNT") == /dev/loop* ]]
```

Only when `[[ ${TIDEMARK_INSTALL_RESTART_AFTER_MOUNT:-0} == 1 ]]`, the harness prints its restart explanation, runs `systemctl restart tidemarkd`, and calls `wait_for_status`. Otherwise there is no restart in verify.

```bash
mkdir -p "$TIDEMARK_INSTALL_MOUNT/tidemark-install-reboot"
systemd-run --wait --unit=tidemark-install-reboot-writer /bin/sh -c 'dd if=/dev/zero of="$1/payload" bs=1M count=8 && sleep 5' sh "$TIDEMARK_INSTALL_MOUNT/tidemark-install-reboot"
deadline=$((SECONDS + 60))
until tidemark hot --window 5m | grep -F "$TIDEMARK_INSTALL_MOUNT/tidemark-install-reboot"; do
    (( SECONDS < deadline )) || { journalctl -u tidemarkd; exit 1; }
    sleep 1
done
```

The writer produces 8 MiB and stays alive five seconds after dd so process attribution can observe it. The hot check requires the full directory path.

## Coordinator guest checks

Run both phases with `TIDEMARK_DEPLOY_TEST_VM=1` and `TIDEMARK_INSTALL_MOUNT` supplied in both. Restore the persistent loop mount before daemon startup, or restore it before verify and explicitly set `TIDEMARK_INSTALL_RESTART_AFTER_MOUNT=1`; the comment records this temporary harness requirement until W6-mount-presence handles late mounts. Exercise the ordinary path without that variable when boot ordering permits it; also exercise the explicit restart path. Confirm all new assertions above, both cgroup memory-property variants where available, unchanged upgrade enablement, no AVCs, and retained state. Archive harness logs, `/var/lib/tidemark-install/security.txt`, and `/var/lib/tidemark-install/unit-verify.txt`.

## Assumptions

- RPM passes postun argument >=1 on upgrade/reinstall and 0 on final erase, matching the coordinator's systemd-macro decision.
- The disposable VM provides systemd's numeric memory properties and named capability list; equality ignores capability order and case, but rejects missing or extra entries.
- Each VM gate uses a fresh fixture; the new reboot directory has no earlier write events in the daemon's hot window.
- The 60-second polling deadline is measured with Bash SECONDS; individual CLI calls use their existing client timeout.
- Existing stale PostgreSQL mentions elsewhere in the rollout are outside this task's sentence-only scope and were not edited.

## Shared-file edits and deviations

No dependencies, manifests, module declarations, or out-of-scope files changed. Used the precreated attempt branch. Cargo uses the pinned toolchain on PATH and a temporary copy of the host cache with offline mode. No privileged host commands were run.

## Validation

Full logs are in the attempt output `library/`. Cargo commands used `PATH=/home/brewerm/.rustup/toolchains/1.98.0-x86_64-unknown-linux-gnu/bin:$PATH`, `CARGO_HOME=/tmp/pkg-upgrade-cargo`, and `CARGO_NET_OFFLINE=true`. Every long command ran under `timeout 1500`.

| Command | Observed result |
|---|---|
| `cargo fmt --check` | Exit 0 |
| `cargo clippy --offline --workspace --all-targets -- -D warnings` | Exit 0 |
| `cargo test --offline -p xtask` | Exit 0; all tests passed, including all eight w6c tests |
| `cargo test --offline --workspace --no-fail-fast` | Exit 101; only failing target `-p tidemarkd --test w6a1`, 14 passed / 5 failed |
| `tools/ci.sh` (offline environment) | Exit 1; source checks/fmt/Clippy passed, stopped at the same w6a1 failures |
| `cargo test --offline -p xtask --features privileged --no-run` | Exit 0; packaging privileged tests compile |
| `bash -n tests/install/install.sh` | Exit 0 |
| `sh -n packaging/rpm/post.sh packaging/rpm/preun.sh packaging/rpm/postun.sh` | Exit 0 |
| `git diff --check` | Exit 0 |
| shellcheck | Not installed; optional check unavailable |

Both workspace and CI fail in `binary_ready_and_stopping`, `lifecycle_reload_and_shutdown`, `corrupt_catalog_is_preserved_before_recovery`, `private_and_missing_state_directories_start`, and `startup_reads_last_complete_from_catalog`, all at `crates/tidemarkd/tests/w6a1.rs:27` with `deadline exceeded`. This matches the task's known socket-restricted sandbox limitation; independent CI rerun remains required. CI did not reach its later full-workspace privileged build or cargo-deny stage. The install/upgrade/erase/reboot assertions are **pending VM**.

Only the seven owned paths changed. No dependency was added. Formatting and lint checks passed; all requested commands were run, with the failed checks and remaining guest uncertainty recorded rather than treated as verified acceptance.
