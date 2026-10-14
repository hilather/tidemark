#!/bin/bash
# Owned by: DOCS-fault. Disposable Rocky guest fault injection.
set -euo pipefail
# Historical text pinned by xtask/tests/w7c.rs: SKIP: 04-compaction-crash.sh needs TIDEMARK_FAULT_SLOW=1 (about 70 minutes)
# The message below corrects that obsolete estimate.
if [[ ${TIDEMARK_FAULT_SLOW:-0} != 1 ]]; then
    echo 'SKIP: 04-compaction-crash.sh needs TIDEMARK_FAULT_SLOW=1 (default 4 hourly phases, about 244 minutes / 4 hours)'
    exit 0
fi
[[ ${TIDEMARK_DEPLOY_TEST_VM:-} == 1 ]] || { echo 'Requires disposable VM marker TIDEMARK_DEPLOY_TEST_VM=1' >&2; echo "RESULT $(basename "$0"): FAIL"; exit 1; }
[[ $EUID == 0 ]] || { echo 'Requires root in disposable VM' >&2; echo "RESULT $(basename "$0"): FAIL"; exit 1; }
cd "$(dirname "$0")/../.." || exit 1
# shellcheck source=tests/fault/common.bash
source tests/fault/common.bash
cycles=${TIDEMARK_FAULT_CYCLES:-2}
[[ $cycles == 1 || $cycles == 2 ]] || { echo 'TIDEMARK_FAULT_CYCLES must be 1 or 2' >&2; exit 1; }
# The shared harness installs the EXIT trap before allocating loop resources.
setup
trap cleanup EXIT
# No production clock manipulation: the real writer compacts hourly.
# Initialize schema, then seed only our private database while daemon is stopped.
start_daemon
stop_daemon
cat >> "$config" <<'CONFIG'
[retention.samples]
raw = "48h"
five_min = "35d"
hourly = "400d"
CONFIG
# Avoid sampler WAL traffic around the hourly compaction boundary.
python3 - "$config" <<'PYCODE'
import pathlib, sys
p = pathlib.Path(sys.argv[1])
p.write_text(p.read_text().replace('interval = "1s"', 'interval = "24h"'))
PYCODE
echo "Compaction: $cycles kill cycles + 2 recovery/idempotence phases, about $(((cycles + 2) * 61)) minutes; each phase bounded by 3800 seconds"
for ((cycle = 0; cycle < cycles; cycle++)); do
    delay=0.02
    if (( cycle == 1 )); then delay=0.2; fi
    run_helper 120 python3 tests/fault/compaction.py seed "$state/timeseries.db"
    start_daemon
    # Observe uncommitted WAL spill rather than assuming an hourly timer hit.
    check "kill during compaction (delay $delay)" run_helper 3800 python3 tests/fault/compaction.py kill "$state/timeseries.db" "$daemon_pid" "$delay"
    await_child "$daemon_pid" 30
    check 'killed compaction database opens cleanly' python3 tests/fault/check.py integrity "$state/timeseries.db"
done
start_daemon
# Capture start time before either verification phase, rejecting PID reuse and zombies.
daemon_start=$(python3 - "$daemon_pid" <<'PYCODE'
import pathlib, sys
fields = pathlib.Path('/proc/%s/stat' % sys.argv[1]).read_text().rsplit(')', 1)[1].split()
assert fields[0] not in ('Z', 'X'), 'daemon is dead'
print(fields[19])
PYCODE
)
check 'recovery compaction reaches oracle' run_helper 3800 python3 tests/fault/compaction.py complete "$state/timeseries.db"
# Seed disjoint due rows after recovery; require their exact compaction and unchanged history.
check 'second compaction is idempotent' run_helper 3800 python3 tests/fault/compaction.py idempotent "$state/timeseries.db" "$daemon_pid" "$daemon_start"
check 'compacted database integrity' python3 tests/fault/check.py integrity "$state/timeseries.db"
