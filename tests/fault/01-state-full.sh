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
start_daemon
# The mandatory 2-GiB floor applies even with min_free_pct=0. Leave 1 GiB.
python3 - "$work/fs/filler" <<'PYCODE'
import os, sys
with open(sys.argv[1], 'wb') as f:
    avail = os.statvfs(os.path.dirname(sys.argv[1])).f_bavail * os.statvfs(os.path.dirname(sys.argv[1])).f_frsize
    os.posix_fallocate(f.fileno(), 0, avail - 1024**3)
PYCODE
if cli_json scan "$root" > "$work/scan.json" 2> "$work/scan.err"; then
    fail 'space guard accepted low-space state filesystem'
else
    rc=$?
    [[ $rc == 3 ]] || fail "daemon scan refusal exit $rc, expected 3"
fi
pass 'space guard refuses below mandatory floor'
check 'refused write leaves no temporary' no_tmp
deadline=$((SECONDS + 90))
until cli_json alerts > "$work/alerts.json" 2> "$work/alerts.err" && python3 tests/fault/check.py alert "$work/alerts.json" > "$work/alert-check.log" 2>&1; do
    (( SECONDS < deadline )) || fail 'state_space alert not raised'
    sleep 1
done
pass 'state-space alert raised'
stop_daemon
rm -- "$work/fs/filler"
# Exercise an admitted writer separately from preflight refusal.
timeout 180 python3 - "$root" <<'PYCODE'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
for n in range(50000):
    (p / f'd{n:05d}').mkdir()
PYCODE
"$cli" -c "$config" --json scan "$root" --mode walk --throttle max > "$work/scan.json" 2> "$work/scan.err" &
pid=$!
children+=("$pid")
deadline=$((SECONDS + 300))
until python3 - "$state" <<'PYCODE'
import pathlib, sys
sys.exit(0 if list(pathlib.Path(sys.argv[1]).rglob('*.db.tmp')) else 1)
PYCODE
 do
    (( SECONDS < deadline )) || fail 'no admitted snapshot writer observed'
    kill -0 "$pid" 2>/dev/null || fail 'snapshot finished before fault injection'
    sleep 0.01
done
kill -STOP "$pid"
# Fill only our loop image, leaving less than 1 MiB available. Fallocate failures
# are expected at the XFS reserve; progressively smaller allocations reach it.
timeout 120 python3 - "$work/fs/filler" <<'PYCODE'
import errno, os, sys
with open(sys.argv[1], 'wb') as f:
    offset = 0
    for chunk in (1024**3, 64*1024**2, 1024**2, 4096):
        while True:
            try:
                os.posix_fallocate(f.fileno(), offset, chunk)
                offset += chunk
            except OSError as e:
                if e.errno != errno.ENOSPC:
                    raise
                break
    v = os.statvfs(os.path.dirname(sys.argv[1]))
    assert v.f_bavail * v.f_frsize < 1024**2, 'loop filesystem did not fill'
PYCODE
kill -CONT "$pid"
await_child "$pid" 300
[[ $child_rc == 1 || $child_rc == 5 ]] || fail "aborted direct write exit $child_rc, expected runtime/precondition failure"
pass 'admitted write aborts on filled state filesystem'
check 'aborted write leaves no temporary' no_tmp
check 'no incomplete published snapshot after disk full' snapshots
