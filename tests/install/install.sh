#!/bin/bash
# Owned by: W6-C. Disposable VM only; install/upgrade/erase and two-phase reboot gate.
# TIDEMARK_INSTALL_UPGRADE_RPM must be built with:
# cargo xtask rpm --native --version 0.1.1 --upgrade-fixture
set -euo pipefail
[[ ${TIDEMARK_DEPLOY_TEST_VM:-} == 1 ]] || { echo 'Requires disposable VM marker TIDEMARK_DEPLOY_TEST_VM=1' >&2; exit 1; }
cd "$(dirname "$0")/../.."
phase=${TIDEMARK_INSTALL_PHASE:-install}
config=/var/lib/tidemark-install/config.toml
wait_for_status() {
    local deadline=$((SECONDS + 60))
    until tidemark status; do
        (( SECONDS < deadline )) || return 1
        sleep 1
    done
}
if [[ $phase == verify ]]; then
    export TIDEMARK_DEPLOY_VM_PHASE=verify TIDEMARK_DEPLOY_VM_CONFIG=$config
    cargo test --offline -p tidemark-cli --features privileged --test w4b w4b::privileged::deploy_stage_reboot_verify -- --exact --nocapture
    tidemark deploy verify --wait 5m
    : "${TIDEMARK_INSTALL_MOUNT:?Supply restored disposable loop-XFS mount}"
    [[ $TIDEMARK_INSTALL_MOUNT =~ ^/[a-zA-Z0-9_/-]+$ && $TIDEMARK_INSTALL_MOUNT != / ]]
    [[ $(findmnt -n -o FSTYPE --target "$TIDEMARK_INSTALL_MOUNT") == xfs ]]
    [[ $(findmnt -n -o TARGET --target "$TIDEMARK_INSTALL_MOUNT") == "$TIDEMARK_INSTALL_MOUNT" ]]
    [[ $(findmnt -n -o SOURCE --target "$TIDEMARK_INSTALL_MOUNT") == /dev/loop* ]]
    # Until the daemon handles late mounts itself, the harness must restore the
    # mount before daemon startup, or restart the daemon after restoring it.
    if [[ ${TIDEMARK_INSTALL_RESTART_AFTER_MOUNT:-0} == 1 ]]; then
        echo 'Restarting tidemarkd after mount restoration (TIDEMARK_INSTALL_RESTART_AFTER_MOUNT=1)'
        systemctl restart tidemarkd
        wait_for_status
    fi
    mkdir -p "$TIDEMARK_INSTALL_MOUNT/tidemark-install-reboot"
    # shellcheck disable=SC2016 # $1 is expanded by the inner sh, using its directory argument.
    systemd-run --wait --unit=tidemark-install-reboot-writer /bin/sh -c 'dd if=/dev/zero of="$1/payload" bs=1M count=8 && sleep 5' sh "$TIDEMARK_INSTALL_MOUNT/tidemark-install-reboot"
    deadline=$((SECONDS + 60))
    until tidemark hot --window 5m | grep -F "$TIDEMARK_INSTALL_MOUNT/tidemark-install-reboot"; do
        (( SECONDS < deadline )) || { journalctl -u tidemarkd; exit 1; }
        sleep 1
    done
    exit 0
fi
[[ $phase == install ]] || { echo 'phase must be install or verify' >&2; exit 1; }
: "${TIDEMARK_INSTALL_RPM:?Supply absolute 0.1.0 RPM path}"
: "${TIDEMARK_INSTALL_UPGRADE_RPM:?Supply absolute 0.1.1 RPM path built with --upgrade-fixture}"
: "${TIDEMARK_INSTALL_MOUNT:?Supply persistent disposable loop-XFS mount (survives reboot)}"
[[ $(findmnt -n -o FSTYPE --target "$TIDEMARK_INSTALL_MOUNT") == xfs ]]
[[ $(findmnt -n -o TARGET --target "$TIDEMARK_INSTALL_MOUNT") == "$TIDEMARK_INSTALL_MOUNT" ]]
[[ $(findmnt -n -o SOURCE --target "$TIDEMARK_INSTALL_MOUNT") == /dev/loop* ]]
[[ $TIDEMARK_INSTALL_MOUNT =~ ^/[a-zA-Z0-9_/-]+$ && $TIDEMARK_INSTALL_MOUNT != / ]]
[[ $(rpm -qp --qf '%{VERSION}' "$TIDEMARK_INSTALL_RPM") == 0.1.0 ]]
[[ $(rpm -qp --qf '%{VERSION}' "$TIDEMARK_INSTALL_UPGRADE_RPM") == 0.1.1 ]]
# Reject an ordinary upgrade package before installing anything. Extract only
# the configuration asset into a private temporary directory.
upgrade_extract=$(mktemp -d)
trap 'rm -rf -- "$upgrade_extract"' EXIT
if ! rpm2cpio "$TIDEMARK_INSTALL_UPGRADE_RPM" | (cd "$upgrade_extract" && cpio -id --quiet './etc/tidemark/tidemark.toml'); then
    echo 'Cannot inspect upgrade RPM configuration (requires rpm2cpio and cpio)' >&2
    exit 1
fi
if ! grep -Fxq '# upgrade fixture 0.1.1' "$upgrade_extract/etc/tidemark/tidemark.toml"; then
    echo 'Upgrade RPM must carry the marker: build with --native --version 0.1.1 --upgrade-fixture' >&2
    exit 1
fi
rm -rf -- "$upgrade_extract"
trap - EXIT
if rpm -q tidemark >/dev/null 2>&1; then
    echo 'Requires a fresh VM without tidemark installed' >&2
    exit 1
fi
# 1. Install, assert post does not start/enable, and compare actual RPM layout.
rpm -qlp "$TIDEMARK_INSTALL_RPM" | sort > /tmp/tidemark-install-layout.actual
python3 - <<'PY' > /tmp/tidemark-install-layout.expected
import re
from pathlib import Path
for dest in sorted(re.findall(r'dest = "([^"]+)"', Path('packaging/rpm/rpm.toml').read_text())):
    print(dest)
PY
diff -u /tmp/tidemark-install-layout.expected /tmp/tidemark-install-layout.actual
dnf install -y "$TIDEMARK_INSTALL_RPM"
if systemctl is-active --quiet tidemarkd; then
    echo 'tidemarkd unexpectedly active' >&2
    exit 1
fi
if systemctl is-enabled --quiet tidemarkd; then
    echo 'tidemarkd unexpectedly enabled' >&2
    exit 1
fi
[[ $(stat -c '%a:%U:%G' /etc/tidemark/tidemark.toml) == 600:root:root ]]
# 2. Packaged example config validates.
tidemarkd --check-config
# 3. Minimal config with the real loop mount watched; no unrelated collectors.
install -d -m 0700 /var/lib/tidemark-install
cat > "$config" <<CONFIG
[general]
state_dir = "/var/lib/tidemark"
[[roots]]
path = "$TIDEMARK_INSTALL_MOUNT"
mode = "walk"
schedule = "off"
[sampler]
mounts = ["$TIDEMARK_INSTALL_MOUNT"]
interval = "1s"
[watch]
mounts = ["$TIDEMARK_INSTALL_MOUNT"]
[procscan]
enabled = false
[lvm]
enabled = false
[metrics]
textfile = ""
CONFIG
install -m 0600 "$config" /etc/tidemark/tidemark.toml
# 4–5. Start and wait for API readiness with a deadline.
systemctl start tidemarkd
wait_for_status
# Assert parsed, effective settings; EL8 cgroup v1 may expose only MemoryLimit.
properties=$(systemctl show tidemarkd -p MemoryMax -p MemoryLimit -p NoNewPrivileges -p User -p Type -p CapabilityBoundingSet)
printf '%s\n' "$properties"
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
# 6. Host namespace write from a separate unit must appear in hot results.
mkdir -p "$TIDEMARK_INSTALL_MOUNT/tidemark-install-namespace"
systemd-run --wait --unit=tidemark-install-writer /bin/dd if=/dev/zero of="$TIDEMARK_INSTALL_MOUNT/tidemark-install-namespace/payload" bs=1M count=50
sync
deadline=$((SECONDS + 150))
until tidemark hot --window 5m | grep -F 'tidemark-install-namespace'; do
    (( SECONDS < deadline )) || { journalctl -u tidemarkd; exit 1; }
    sleep 1
done
# 7. Audit query errors are failures, not an empty success.
if [[ ${TIDEMARK_INSTALL_ALLOW_PERMISSIVE:-0} == 1 ]]; then
    echo 'SELinux acceptance is NOT tested in this run (TIDEMARK_INSTALL_ALLOW_PERMISSIVE=1)'
else
    [[ $(getenforce) == Enforcing ]] || { echo 'SELinux must be Enforcing for acceptance' >&2; exit 1; }
fi
systemctl is-active --quiet auditd
set +e
ausearch -m AVC -ts recent -c tidemarkd > /tmp/tidemark-install-avc.log 2>&1
audit_status=$?
set -e
if (( audit_status != 0 )); then
    if (( audit_status != 1 )) || ! grep -Fq '<no matches>' /tmp/tidemark-install-avc.log; then
        cat /tmp/tidemark-install-avc.log
        exit 1
    fi
else
    cat /tmp/tidemark-install-avc.log
    exit 1
fi
systemd-analyze security tidemarkd > /var/lib/tidemark-install/security.txt
# 8. Upgrade restarts a running daemon, preserves config, and leaves a stopped one stopped.
echo '# W6-C preserve this edit' >> /etc/tidemark/tidemark.toml
cp /etc/tidemark/tidemark.toml /var/lib/tidemark-install/edited.toml
before_pid=$(systemctl show tidemarkd -p MainPID --value)
[[ $before_pid =~ ^[0-9]+$ && $before_pid != 0 ]]
dnf upgrade -y "$TIDEMARK_INSTALL_UPGRADE_RPM"
systemctl is-active --quiet tidemarkd
after_pid=$(systemctl show tidemarkd -p MainPID --value)
[[ $after_pid =~ ^[0-9]+$ && $after_pid != 0 && $after_pid != "$before_pid" ]]
wait_for_status
cmp /etc/tidemark/tidemark.toml /var/lib/tidemark-install/edited.toml
[[ -f /etc/tidemark/tidemark.toml.rpmnew ]]
[[ $(stat -c '%a:%U:%G' /etc/tidemark/tidemark.toml.rpmnew) == 600:root:root ]]
grep -Fxq '# upgrade fixture 0.1.1' /etc/tidemark/tidemark.toml.rpmnew
systemctl stop tidemarkd
dnf reinstall -y "$TIDEMARK_INSTALL_UPGRADE_RPM"
[[ $(systemctl show tidemarkd -p ActiveState --value) == inactive ]]
systemctl start tidemarkd
wait_for_status
[[ $(rpm -q --qf '%{VERSION}' tidemark) == 0.1.1 ]]
cmp /etc/tidemark/tidemark.toml /var/lib/tidemark-install/edited.toml
[[ $(stat -c '%a:%U:%G' /etc/tidemark/tidemark.toml) == 600:root:root ]]
# 9. Erase stops/disables service and retains state bytes.
systemctl enable tidemarkd
systemctl is-enabled --quiet tidemarkd
systemctl is-active --quiet tidemarkd
printf 'retain\n' > /var/lib/tidemark/install-retain
rpm -e tidemark
if systemctl is-active --quiet tidemarkd; then
    echo 'tidemarkd unexpectedly active' >&2
    exit 1
fi
if systemctl is-enabled --quiet tidemarkd; then
    echo 'tidemarkd unexpectedly enabled' >&2
    exit 1
fi
[[ ! -e /etc/systemd/system/multi-user.target.wants/tidemarkd.service && ! -L /etc/systemd/system/multi-user.target.wants/tidemarkd.service ]]
[[ $(cat /var/lib/tidemark/install-retain) == retain ]]
cmp /etc/tidemark/tidemark.toml.rpmsave /var/lib/tidemark-install/edited.toml
[[ $(stat -c '%a' /etc/tidemark/tidemark.toml.rpmsave) == 600 ]]
# 10. Reinstall, stage with the W4-B test; external harness reboots then runs verify.
dnf install -y "$TIDEMARK_INSTALL_UPGRADE_RPM"
export TIDEMARK_DEPLOY_VM_PHASE=stage TIDEMARK_DEPLOY_VM_CONFIG=$config
cargo test --offline -p tidemark-cli --features privileged --test w4b w4b::privileged::deploy_stage_reboot_verify -- --exact --nocapture
printf '%s\n' 'Stage complete. Harness must reboot, restore the loop mount, then run TIDEMARK_INSTALL_PHASE=verify.'
