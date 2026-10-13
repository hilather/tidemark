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
# Recognize every public mode, but a failed transport is not ready.
watch_ready() {
    deadline=$((SECONDS + 60))
    until cli_json status > "$work/status.json" &&
        python3 tests/fault/check.py watch-mode "$work/status.json" "$work/fs" > "$work/mode.log" 2>&1; do
        (( SECONDS < deadline )) || { cat "$work/mode.log"; fail 'watcher did not negotiate a mode'; return 1; }
        sleep 1
    done
    cat "$work/mode.log"
}
watch_ready || exit 1
burst() {
    local label=$1 kind=${2:-burst} writer_pid deadline writer_rc
    local distinct=20000 health=watch-clean
    if [[ $kind == distinct-cap ]]; then distinct=100000; health=watch-cap; fi
    # Capture published cumulative counters before starting each independent burst.
    deadline=$((SECONDS + 30))
    until python3 tests/fault/check.py watch-counters "$work/metrics/metrics.prom" > "$work/before.json" 2> "$work/counters.err"; do
        (( SECONDS < deadline )) || { fail 'watch counters unavailable'; return 1; }
        sleep 0.1
    done
    setsid python3 - "$root" "$distinct" "$kind" <<'PYCODE' &
import pathlib, sys, time
p = pathlib.Path(sys.argv[1])
started = int(time.time()) // 60
for n in range(100000):
    with (p / f'burst-{n % int(sys.argv[2]):06d}').open('wb') as f:
        f.write(b'x')
if sys.argv[3] == 'distinct-cap':
    assert int(time.time()) // 60 == started, 'distinct-cap workload crossed minute boundary'
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
        python3 tests/fault/check.py "$kind" "$work/metrics/metrics.prom" "$work/before.json" 100000 "$work/now.json" "$work/fs" > "$work/burst-check.log" 2>&1 &&
        cli_json status > "$work/burst-status.json" &&
        python3 tests/fault/check.py "$health" "$work/burst-status.json" "$work/fs" >> "$work/burst-check.log" 2>&1; do
        kill -0 "$daemon_pid" 2>/dev/null || { fail 'daemon exited after burst'; return 1; }
        (( SECONDS < deadline )) || { fail "burst $label lacks complete no-loss events"; return 1; }
        sleep 0.1
    done
    check "burst $label no-loss events" python3 tests/fault/check.py "$kind" "$work/metrics/metrics.prom" "$work/before.json" 100000 "$work/now.json" "$work/fs"
    cli_json status > "$work/burst-status.json"
    check "status after burst $label" python3 tests/fault/check.py "$health" "$work/burst-status.json" "$work/fs"
    pass "daemon survived burst $label: pid=$daemon_pid"
}
for trial in 1 2 3; do
    burst "$trial" || exit 1
done
stop_daemon
start_daemon 256
watch_ready || exit 1
# A fresh daemon's metrics must replace the previous run's counters before baseline.
sleep 2
burst low-limit || exit 1

# Isolate aggregation truncation from both the clean and queue-overflow passes.
stop_daemon
start_daemon
watch_ready || exit 1
sleep 2
# Begin near a minute boundary so 100,000 distinct identities hit one bucket.
run_helper 65 python3 - <<'PYCODE'
import time
while time.time() % 60 > 2:
    time.sleep(0.1)
PYCODE
burst distinct-cap distinct-cap || exit 1
stop_daemon
start_daemon
watch_ready || exit 1
sleep 2

# Keep the no-loss bursts above; force the separate loss path without kernel tuning.
# cleanup also resumes every tracked process group before terminating it.
resume_cleanup() {
    local rc=$?
    kill -CONT "$daemon_pid" 2>/dev/null || true
    cleanup "$rc"
}
trap resume_cleanup EXIT
# Reject a guest whose queue cannot be overfilled within the agreed write cap.
if [[ -e /proc/sys/fs/fanotify/max_queued_events ]]; then
    read -r queue_limit < /proc/sys/fs/fanotify/max_queued_events
    echo "fanotify queue limit: $queue_limit (kernel sysctl)"
else
    queue_limit=16384
    echo "fanotify queue limit: $queue_limit (EL8 fixed default; sysctl absent)"
fi
[[ $queue_limit =~ ^[0-9]+$ ]] || { fail 'invalid fanotify queue limit'; exit 1; }
writes=$((queue_limit * 2))
(( queue_limit > 0 && writes <= 200000 )) || { fail "queue limit $queue_limit cannot be doubled within 200000 writes"; exit 1; }
cli_json now > "$work/forced-before-now.json"
check 'no pre-existing lossy minutes' python3 - "$work/forced-before-now.json" "$work/fs" <<'PYCODE'
import json, sys
m = next(m for m in json.load(open(sys.argv[1]))['data']['mounts'] if m['mount'] == sys.argv[2])
assert m['lossy_minutes_15m'] == 0, m
PYCODE
python3 tests/fault/check.py watch-counters "$work/metrics/metrics.prom" > "$work/forced-before.json"
kill -STOP "$daemon_pid"
# Confirm all daemon threads have stopped before producing events.
deadline=$((SECONDS + 10))
until [[ $(ps -o stat= -p "$daemon_pid") == T* ]]; do
    (( SECONDS < deadline )) || { fail 'daemon did not stop'; exit 1; }
    sleep 0.1
done
run_helper 180 python3 - "$root" "$writes" <<'PYCODE'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
for n in range(int(sys.argv[2])):
    with (p / f'forced-{n:06d}').open('wb') as f:
        f.write(b'x')
PYCODE
kill -CONT "$daemon_pid"
resumed_at=$SECONDS
deadline=$((SECONDS + 60))
# Share the remaining deadline across requests, rather than granting each 20s.
cli_before_deadline() {
    local remaining=$((deadline - SECONDS))
    (( remaining > 0 )) || return 1
    (( remaining <= 20 )) || remaining=20
    run_helper "$remaining" "$cli" -c "$config" --json "$@"
}
forced_hot() {
    local hot_rc=0
    cli_before_deadline hot --mount "$work/fs" --window 1m > "$work/forced-hot.json" 2> "$work/forced-hot.err" || hot_rc=$?
    printf '%s\n' "$hot_rc" > "$work/forced-hot.rc"
    # Exit 4 is the documented unavailable result; the status request must succeed.
    [[ $hot_rc == 0 || $hot_rc == 4 ]]
}
until kill -0 "$daemon_pid" 2>/dev/null &&
    cli_before_deadline status > "$work/forced-status.json" 2> "$work/forced-status.err" &&
    cli_before_deadline now > "$work/forced-now.json" 2> "$work/forced-now.err" &&
    forced_hot &&
    python3 tests/fault/check.py forced-overflow "$work/metrics/metrics.prom" "$work/forced-before.json" "$writes" "$work/forced-now.json" "$work/fs" "$work/forced-hot.json" "$work/forced-hot.rc" > "$work/forced-check.log" 2>&1; do
    kill -0 "$daemon_pid" 2>/dev/null || { fail 'daemon exited after forced overflow'; exit 1; }
    (( SECONDS < deadline )) || { fail 'forced overflow evidence missing within 60 seconds'; exit 1; }
    sleep 0.1
done
(( SECONDS <= deadline )) || { fail 'forced overflow evidence exceeded 60 seconds'; exit 1; }
check 'forced overflow counted, current minute and hot window lossy, missing events' python3 tests/fault/check.py forced-overflow "$work/metrics/metrics.prom" "$work/forced-before.json" "$writes" "$work/forced-now.json" "$work/fs" "$work/forced-hot.json" "$work/forced-hot.rc"
check 'daemon alive after forced overflow' kill -0 "$daemon_pid"
check 'status after forced overflow' cli_json status
# Let the already-queued forced events finish before taking the new-write baseline.
deadline=$((resumed_at + 90))
python3 tests/fault/check.py watch-counters "$work/metrics/metrics.prom" > "$work/forced-loss-counters.json"
while :; do
    sleep 3
    python3 tests/fault/check.py watch-counters "$work/metrics/metrics.prom" > "$work/settled-counters.json"
    if cmp -s "$work/forced-loss-counters.json" "$work/settled-counters.json"; then break; fi
    cp "$work/settled-counters.json" "$work/forced-loss-counters.json"
    (( SECONDS < deadline )) || { fail 'forced counters did not settle'; exit 1; }
done
# Transport recovery is independent of the retained fifteen-minute loss history.
deadline=$((resumed_at + 120))
run_helper 20 python3 - "$root" <<'PYCODE'
import pathlib, sys
p = pathlib.Path(sys.argv[1]) / 'resumed'
p.mkdir()
for n in range(100):
    with (p / f'write-{n:03d}').open('wb') as f:
        f.write(b'x')
PYCODE
until cli_before_deadline now > "$work/resumed-now.json" &&
    cli_before_deadline status > "$work/resumed-status.json" &&
    python3 tests/fault/check.py resumed "$work/metrics/metrics.prom" "$work/forced-loss-counters.json" 100 "$work/resumed-now.json" "$work/fs" > "$work/resumed-check.log" 2>&1 &&
    python3 tests/fault/check.py watch-resumed "$work/resumed-status.json" "$work/fs" >> "$work/resumed-check.log" 2>&1; do
    kill -0 "$daemon_pid" 2>/dev/null || { fail 'daemon exited during transport recovery'; exit 1; }
    (( SECONDS < deadline )) || { fail 'new writes not observed within 120 seconds after SIGCONT'; exit 1; }
    sleep 0.1
done
(( SECONDS <= deadline )) || { fail 'transport recovery exceeded 120 seconds'; exit 1; }
check '100 new writes observed without new overflow within 120 seconds' python3 tests/fault/check.py resumed "$work/metrics/metrics.prom" "$work/forced-loss-counters.json" 100 "$work/resumed-now.json" "$work/fs"
check 'transport recovered with retained overflow reason allowed' python3 tests/fault/check.py watch-resumed "$work/resumed-status.json" "$work/fs"
if [[ ${TIDEMARK_FAULT_SLOW:-0} == 1 ]]; then
    # Poll the real history expiry; allow fifteen buckets, minute alignment and publication.
    deadline=$((resumed_at + 1020))
    until cli_before_deadline status > "$work/forced-recovery.json" &&
        python3 tests/fault/check.py watch-ok "$work/forced-recovery.json" "$work/fs" > "$work/history-check.log" 2>&1; do
        kill -0 "$daemon_pid" 2>/dev/null || { fail 'daemon exited during history recovery'; exit 1; }
        (( SECONDS < deadline )) || { fail 'watcher loss history did not expire within 17 minutes'; exit 1; }
        sleep 1
    done
    check 'watcher ok after affected minute leaves fifteen-minute window' python3 tests/fault/check.py watch-ok "$work/forced-recovery.json" "$work/fs"
    check 'no further overflow during history recovery' python3 tests/fault/check.py resumed "$work/metrics/metrics.prom" "$work/forced-loss-counters.json" 100 "$work/resumed-now.json" "$work/fs"
else
    echo 'SKIP: long fifteen-minute history recovery check needs TIDEMARK_FAULT_SLOW=1'
fi
