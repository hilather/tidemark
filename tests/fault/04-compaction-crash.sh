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
for delay in 0.02 0.2; do
    timeout 120 python3 tests/fault/compaction.py seed "$state/timeseries.db"
    start_daemon
    # Observe uncommitted WAL spill rather than assuming an hourly timer hit.
    check "kill during compaction (delay $delay)" timeout 3800 python3 tests/fault/compaction.py kill "$state/timeseries.db" "$daemon_pid" "$delay"
    await_child "$daemon_pid" 30
    check 'killed compaction database opens cleanly' python3 tests/fault/check.py integrity "$state/timeseries.db"
done
start_daemon
check 'recovery compaction reaches oracle' timeout 3800 python3 tests/fault/compaction.py complete "$state/timeseries.db"
pass 'restart compaction produces one last-value row per complete bucket'
# One more actual daemon compaction must leave seeded historical rows identical.
check 'second compaction is idempotent' timeout 3800 python3 tests/fault/compaction.py idempotent "$state/timeseries.db"
check 'compacted database integrity' python3 tests/fault/check.py integrity "$state/timeseries.db"
