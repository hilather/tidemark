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
# Stop all daemon threads so the kernel queue deterministically overflows.
kill -STOP "$daemon_pid"
run_helper 120 python3 - "$root" <<'PYCODE'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
for n in range(100000):
    with (p / f'burst{n:06d}').open('wb') as f:
        f.write(b'x')
PYCODE
kill -CONT "$daemon_pid"
deadline=$((SECONDS + 150))
until cli_json now > "$work/now.json" 2> "$work/now.err" && python3 tests/fault/check.py lossy "$work/now.json" "$work/fs" > "$work/lossy-check.log" 2>&1; do
    (( SECONDS < deadline )) || { fail 'overflow did not flag a lossy minute'; break; }
    sleep 1
done
deadline=$((SECONDS + 30))
until python3 tests/fault/check.py overflow "$work/metrics/metrics.prom" 0 > "$work/counter-check.log" 2>&1; do
    (( SECONDS < deadline )) || { fail 'overflow counter did not grow'; break; }
    sleep 1
done
check 'overflow counter grows' python3 tests/fault/check.py overflow "$work/metrics/metrics.prom" 0
check 'lossy minute flagged' python3 tests/fault/check.py lossy "$work/now.json" "$work/fs"
# A new close-write after recovery must be visible in hot JSON, not just a live PID.
mkdir "$root/recovered"
printf 'recovery' > "$root/recovered/after-overflow"
deadline=$((SECONDS + 90))
until cli_json now > "$work/recovery.json" 2> "$work/recovery.err" && python3 - "$work/recovery.json" "$root/recovered" <<'PYCODE'
import json, sys
mounts = json.load(open(sys.argv[1]))['data']['mounts']
assert any(e['events'] > 0 and e['path'] == sys.argv[2] for m in mounts for e in m['hot_dirs_5m']), mounts
PYCODE
 do
    (( SECONDS < deadline )) || { fail 'no close-write activity after recovery'; break; }
    sleep 1
done
check 'watcher records activity after overflow' python3 tests/fault/check.py activity "$work/recovery.json" "$root/recovered"
if cli_json status > "$work/status.json" 2> "$work/status.err"; then
    pass "watcher remains responsive: $(cat "$work/status.json")"
else
    fail 'watcher status unavailable after overflow'
fi
if kill -0 "$daemon_pid"; then
    pass "daemon remains running after overflow: pid=$daemon_pid"
else
    fail 'watcher daemon exited'
fi
