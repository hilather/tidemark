#!/usr/bin/env python3
# Owned by: W7-C2-fault. JSON and SQLite invariant checks for guest scripts.
import json
import pathlib
import sqlite3
import sys

mode, arg, *rest = sys.argv[1:]
p = pathlib.Path(arg)
if mode == 'fixture':
    for n in range(int(rest[0])):
        (p / f'f{n:07d}').touch()
elif mode == 'no-tmp':
    assert not list(p.rglob('*.tmp')), list(p.rglob('*.tmp'))
elif mode == 'snapshots':
    for file in sorted((p / 'snapshots').rglob('*.db')):
        with sqlite3.connect(f'file:{file}?mode=ro', uri=True) as db:
            assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',), file
            meta = dict(db.execute('SELECT key,value FROM meta'))
            assert meta['status'] == 'complete', (file, meta)
            assert int(meta['finished_at']) >= int(meta['started_at']), file
            for key in ('schema_version tool_version scan_id root root_display mode mode_reason status degraded exclude link_instances started_at finished_at host kernel os_release fs_uuid fs_dev fs_blocksize threads throttle_profile dirs files inodes errors races skipped noatime_fallbacks phase_ms gate_stats').split():
                assert key in meta, (file, key)
elif mode == 'scan':
    doc = json.loads(p.read_text())
    assert doc['schema'] == 'tidemark.scan.v1', doc
    data = doc['data']
    assert data['status'] == 'complete', data
    if rest[0] == 'huge':
        assert data['summary']['files'] == int(rest[1]), data
    elif rest[0] == 'races':
        assert data['summary']['races'] > 0, data
elif mode == 'alert':
    data = json.loads(p.read_text())['data']
    assert any(row['rule'] == 'state_space' for row in data), data
elif mode == 'lossy':
    data = json.loads(p.read_text())['data']
    assert any(m['mount'] == rest[0] and m['lossy_minutes_15m'] > 0 for m in data['mounts']), data
elif mode == 'overflow':
    # Metrics are the public cumulative counter; JSON now supplies lossy facts.
    import re
    values = [float(v) for v in re.findall(r'^tidemark_watch_overflows_total\{[^\n]*\}\s+(\S+)', p.read_text(), re.M)]
    assert values and sum(values) > float(rest[0]), values
    print(sum(values))
elif mode == 'integrity':
    with sqlite3.connect(f'file:{p}?mode=ro', uri=True) as db:
        assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',)
else:
    raise ValueError(mode)
