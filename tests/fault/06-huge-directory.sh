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
count=50000
if [[ ${TIDEMARK_FAULT_SCALE:-0} == 1 ]]; then count=1000000; fi
fixture "$count"
# Direct child PID is the scanner whose RSS is measured.
setsid "$cli" -c "$config" --json scan "$root" --mode walk --throttle max > "$work/scan.json" 2> "$work/scan.err" &
pid=$!
children+=("$pid")
peak=0
samples=0
deadline=$((SECONDS + 900))
while kill -0 "$pid" 2>/dev/null; do
    (( SECONDS < deadline )) || { fail 'huge scan deadline'; exit 1; }
    if rss=$(python3 - "$pid" <<'PYCODE'
import pathlib, sys
try:
    text = pathlib.Path('/proc', sys.argv[1], 'status').read_text()
except FileNotFoundError:
    sys.exit(1)
print(max([int(line.split()[1]) for line in text.splitlines() if line.startswith(('VmRSS:', 'VmHWM:'))] or [0]))
PYCODE
    ); then
        samples=$((samples + 1))
        if (( rss > peak )); then peak=$rss; fi
    fi
    sleep 0.01
done
await_child "$pid" 10
if [[ $child_rc == 0 ]]; then
    pass "huge scan: exit=$child_rc"
else
    fail "huge scan: exit=$child_rc"
fi
if (( samples > 0 && peak > 0 && peak <= 1280 * 1024 )); then
    pass "peak RSS $peak KiB under configured budget 1310720 KiB ($samples samples)"
else
    fail "RSS peak=$peak KiB samples=$samples budget=1310720 KiB"
fi
check "progress reaches $count files" python3 tests/fault/check.py scan "$work/scan.json" huge "$count"
check 'huge snapshot publication valid' snapshots
