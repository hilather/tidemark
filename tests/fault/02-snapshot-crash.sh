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
timeout 180 python3 - "$root" <<'PYCODE'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
for n in range(50000):
    d = p / f'd{n:05d}'
    d.mkdir()
    (d / 'file').touch()
PYCODE
hits=0
for delay in 0.02 0.04 0.08 0.12 0.2 0.3 0.5 0.7 1 1.3 1.6 2; do
    # Run the binary directly so SIGKILL reaches the writer, not a timeout wrapper.
    "$cli" -c "$config" --json scan "$root" --mode walk --throttle max > "$work/scan.json" 2> "$work/scan.err" &
    pid=$!
    children+=("$pid")
    deadline=$((SECONDS + 300))
    until python3 - "$state" <<'PYCODE'
import pathlib, sys
sys.exit(0 if list(pathlib.Path(sys.argv[1]).rglob('*.db.tmp')) else 1)
PYCODE
    do
        (( SECONDS < deadline )) || fail 'snapshot writer observation deadline'
        kill -0 "$pid" 2>/dev/null || fail 'snapshot completed before writer observation'
        sleep 0.01
    done
    sleep "$delay"
    if python3 - "$state" <<'PYCODE'
import pathlib, sys
sys.exit(0 if list(pathlib.Path(sys.argv[1]).rglob('*.db.tmp')) else 1)
PYCODE
    then
        hits=$((hits + 1))
        echo "PASS: delay $delay observed snapshot .tmp"
    else
        echo "PASS: delay $delay did not observe snapshot .tmp (timing evidence only)"
    fi
    kill -KILL "$pid" 2>/dev/null || true
    await_child "$pid" 30
    check "publication invariant after kill at $delay" snapshots
    start_daemon
    check "startup cleans temporary after $delay" no_tmp
    stop_daemon
done
(( hits > 0 )) || fail 'no delay hit an active snapshot write'
pass "$hits delays hit snapshot write"
scan_background
await_child "$scan_pid" 900
[[ $child_rc == 0 ]] || fail "recovery scan exit $child_rc"
check 'recovery scan completes' python3 tests/fault/check.py scan "$work/scan.json" huge 50000
check 'recovered publications complete' snapshots
