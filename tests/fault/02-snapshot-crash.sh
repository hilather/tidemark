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
run_helper 180 python3 - "$root" <<'PYCODE'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
for n in range(50000):
    d = p / f'd{n:05d}'
    d.mkdir()
    (d / 'file').touch()
PYCODE
hits=0
for delay in 0.02 0.04 0.08 0.12 0.2 0.3 0.5 0.7 1 1.3 1.6 2; do
    # Save the old set so stale files cannot satisfy this iteration.
    python3 tests/fault/check.py tmp-set "$state" > "$work/before-tmp.json"
    setsid "$cli" -c "$config" --json scan "$root" --mode walk --throttle max > "$work/scan.json" 2> "$work/scan.err" &
    pid=$!
    children+=("$pid")
    deadline=$((SECONDS + 300))
    observed=0
    while kill -0 "$pid" 2>/dev/null; do
        if python3 tests/fault/check.py new-tmp "$state" "$work/before-tmp.json" > "$work/new-tmp.log"; then
            observed=1
            break
        fi
        (( SECONDS < deadline )) || { fail 'snapshot writer observation deadline'; exit 1; }
        sleep 0.01
    done
    if (( observed )); then sleep "$delay"; fi
    if (( observed )) && python3 tests/fault/check.py new-tmp "$state" "$work/before-tmp.json" > "$work/new-tmp.log"; then
        hits=$((hits + 1))
        pass "delay $delay observed NEW snapshot .tmp: $(cat "$work/new-tmp.log")"
    else
        echo "SKIP: delay $delay (scan finished before the kill)"
        await_child "$pid" 30
        if [[ $child_rc != 0 ]]; then
            fail "delay $delay scan ended without a kill: exit=$child_rc"
        fi
        check "publication invariant at skipped delay $delay: scan_exit=$child_rc" snapshots
        mkdir "$work/delay-$delay"
        cp "$work/scan.json" "$work/scan.err" "$work/before-tmp.json" "$work/new-tmp.log" "$work/delay-$delay/"
        continue
    fi
    kill -KILL "$pid" 2>/dev/null || true
    await_child "$pid" 30
    check "publication invariant after kill at $delay" snapshots
    start_daemon
    check "startup cleans temporary after $delay" no_tmp
    stop_daemon
    mkdir "$work/delay-$delay"
    cp "$work/scan.json" "$work/scan.err" "$work/before-tmp.json" "$work/new-tmp.log" "$work/delay-$delay/"
done
if (( hits >= 3 )); then
    pass "$hits delays hit snapshot write (minimum=3)"
else
    fail "only $hits delays hit an active snapshot write, need >=3"
fi
scan_background
await_child "$scan_pid" 900
if [[ $child_rc == 0 ]]; then
    pass "recovery scan: exit=$child_rc"
else
    fail "recovery scan: exit=$child_rc"
fi
check 'recovery scan completes' python3 tests/fault/check.py scan "$work/scan.json" huge 50000
check 'recovered publications complete' snapshots
