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
for n in range(2000):
    d = p / f'd{n:05d}'
    d.mkdir()
    for k in range(20):
        (d / f'f{k}').touch()
PYCODE
scan_background
# Repeated deletion creates races while the scanner discovers directory entries.
setsid python3 - "$root" <<'PYCODE' &
import pathlib, shutil, sys, time
p = pathlib.Path(sys.argv[1])
time.sleep(0.05)
for d in sorted(p.iterdir(), reverse=True):
    shutil.rmtree(d)
    time.sleep(0.002)
PYCODE
mutator=$!
children+=("$mutator")
await_child "$scan_pid" 900
if [[ $child_rc == 0 || $child_rc == 3 ]]; then
    pass "race scan: exit=$child_rc"
else
    fail "race scan: exit=$child_rc"
fi
await_child "$mutator" 60
check 'removed directories counted as races and scan complete' python3 tests/fault/check.py scan "$work/scan.json" races
check 'race snapshot publication valid' snapshots
