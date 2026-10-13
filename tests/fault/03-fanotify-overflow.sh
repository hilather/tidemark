#!/bin/bash
# Owned by: W7-C2-fault. Disposable Rocky guest fault injection.
set -euo pipefail
[[ ${TIDEMARK_DEPLOY_TEST_VM:-} == 1 ]] || { echo 'Requires disposable VM marker TIDEMARK_DEPLOY_TEST_VM=1' >&2; echo "RESULT $(basename "$0"): FAIL"; exit 1; }
[[ $EUID == 0 ]] || { echo 'Requires root in disposable VM' >&2; echo "RESULT $(basename "$0"): FAIL"; exit 1; }
cd "$(dirname "$0")/../.." || exit 1
# shellcheck source=tests/fault/common.bash
source tests/fault/common.bash
# The shared harness installs the EXIT trap before allocating loop resources.
setup
trap cleanup EXIT
write_config true
start_daemon
# Ensure the mark is installed before stopping consumption. No kernel tuning.
deadline=$((SECONDS + 60))
until cli_json status > "$work/status.json" && python3 - "$work/status.json" "$work/fs" <<'PYCODE'
import json, sys
modes = dict(json.load(open(sys.argv[1]))['data']['watch_modes'])
assert modes.get(sys.argv[2]) in ('fd_mount', 'dfid_name_mount', 'dfid_name_filesystem'), modes
PYCODE
 do
    (( SECONDS < deadline )) || { fail 'watcher did not negotiate a mode'; exit 1; }
    sleep 1
done
burst() {
    local label=$1 writer_pid deadline writer_rc
    # Capture published cumulative counters before starting each independent burst.
    deadline=$((SECONDS + 30))
    until python3 tests/fault/check.py watch-counters "$work/metrics/metrics.prom" > "$work/before.json" 2> "$work/counters.err"; do
        (( SECONDS < deadline )) || { fail 'watch counters unavailable'; return 1; }
        sleep 0.1
    done
    setsid python3 - "$root" "$label" <<'PYCODE' &
import pathlib, sys
p = pathlib.Path(sys.argv[1])
for n in range(100000):
    with (p / f'burst-{sys.argv[2]}-{n:06d}').open('wb') as f:
        f.write(b'x')
PYCODE
    writer_pid=$!
    children+=("$writer_pid")
    deadline=$((SECONDS + 180))
    while kill -0 "$writer_pid" 2>/dev/null; do
        kill -0 "$daemon_pid" 2>/dev/null || { fail 'daemon exited during burst'; return 1; }
        # Every request must answer while the writer is running.
        cli_json status > "$work/status.json" 2> "$work/status.err" || { fail 'status failed during burst'; return 1; }
        (( SECONDS < deadline )) || { fail 'burst deadline'; return 1; }
        sleep 0.1
    done
    if wait "$writer_pid"; then writer_rc=0; else writer_rc=$?; fi
    forget_child "$writer_pid"
    [[ $writer_rc == 0 ]] || { fail "writer exit $writer_rc"; return 1; }
    deadline=$((SECONDS + 150))
    until cli_json now > "$work/now.json" 2> "$work/now.err" &&
        python3 tests/fault/check.py burst "$work/metrics/metrics.prom" "$work/before.json" 100000 "$work/now.json" "$work/fs" > "$work/burst-check.log" 2>&1; do
        kill -0 "$daemon_pid" 2>/dev/null || { fail 'daemon exited after burst'; return 1; }
        (( SECONDS < deadline )) || { fail "burst $label lacks accounted events/loss"; return 1; }
        sleep 0.1
    done
    check "burst $label accounted events and loss" python3 tests/fault/check.py burst "$work/metrics/metrics.prom" "$work/before.json" 100000 "$work/now.json" "$work/fs"
    check "status after burst $label" cli_json status
    mkdir "$root/recovered-$label"
    printf 'recovery' > "$root/recovered-$label/after-overflow"
    deadline=$((SECONDS + 90))
    until cli_json now > "$work/recovery.json" 2> "$work/recovery.err" &&
        python3 tests/fault/check.py activity "$work/recovery.json" "$root/recovered-$label" > "$work/recovery-check.log" 2>&1; do
        (( SECONDS < deadline )) || { fail "watcher did not recover after burst $label"; return 1; }
        sleep 0.1
    done
    check "watcher records activity after burst $label" python3 tests/fault/check.py activity "$work/recovery.json" "$root/recovered-$label"
    pass "daemon survived burst $label: pid=$daemon_pid"
}
for trial in 1 2 3; do
    burst "$trial" || exit 1
done
stop_daemon
start_daemon 256
# A fresh daemon's metrics must replace the previous run's counters before baseline.
sleep 2
burst low-limit || exit 1
