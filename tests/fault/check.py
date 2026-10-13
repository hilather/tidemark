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
elif mode == 'tmp-set':
    print(json.dumps(sorted(str(f) for f in p.rglob('*.db.tmp'))))
elif mode == 'new-tmp':
    old = set(json.loads(pathlib.Path(rest[0]).read_text()))
    new = sorted(str(f) for f in p.rglob('*.db.tmp') if str(f) not in old)
    if not new:
        sys.exit(1)
    print(json.dumps(new))
elif mode == 'no-tmp':
    assert not list(p.rglob('*.tmp')), list(p.rglob('*.tmp'))
    print('temporaries=0')
elif mode == 'snapshots':
    files = sorted((p / 'snapshots').rglob('*.db'))
    for file in files:
        with sqlite3.connect(f'file:{file}?mode=ro', uri=True) as db:
            assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',), file
            meta = dict(db.execute('SELECT key,value FROM meta'))
            assert meta['status'] == 'complete', (file, meta)
            assert int(meta['finished_at']) >= int(meta['started_at']), file
            for key in ('schema_version tool_version scan_id root root_display mode mode_reason status degraded exclude link_instances started_at finished_at host kernel os_release fs_uuid fs_dev fs_blocksize threads throttle_profile dirs files inodes errors races skipped noatime_fallbacks phase_ms gate_stats').split():
                assert key in meta, (file, key)
    print(f'complete_snapshots={len(files)} integrity=ok')
elif mode == 'scan':
    doc = json.loads(p.read_text())
    assert doc['schema'] == 'tidemark.scan.v1', doc
    data = doc['data']
    assert data['status'] == 'complete', data
    if rest[0] == 'huge':
        assert data['summary']['files'] == int(rest[1]), data
    elif rest[0] == 'races':
        assert data['summary']['races'] > 0, data
    print(json.dumps(data['summary'], sort_keys=True))
elif mode == 'alert':
    doc = json.loads(p.read_text())
    data = doc['data']
    assert any(row['rule'] == 'state_space' for row in data), data
    print(json.dumps(doc, sort_keys=True))
elif mode == 'lossy':
    data = json.loads(p.read_text())['data']
    assert any(m['mount'] == rest[0] and m['lossy_minutes_15m'] > 0 for m in data['mounts']), data
    print(json.dumps(data['mounts'], sort_keys=True))
elif mode == 'activity':
    mounts = json.loads(p.read_text())['data']['mounts']
    entries = [e for m in mounts for e in m['hot_dirs_5m']
               if e['events'] > 0 and e['path'] == rest[0]]
    assert entries, mounts
    print(json.dumps(entries, sort_keys=True))
elif mode == 'overflow':
    # Metrics are the public cumulative counter; JSON now supplies lossy facts.
    import re
    values = [float(v) for v in re.findall(r'^tidemark_watch_overflows_total\{[^\n]*\}\s+(\S+)', p.read_text(), re.M)]
    assert values and sum(values) > float(rest[0]), values
    print('\n'.join(line for line in p.read_text().splitlines() if line.startswith('tidemark_watch_overflows_total{')))
    print(f'overflow_total={sum(values)} baseline={rest[0]}')
elif mode in ('watch-counters', 'burst', 'forced-overflow'):
    import re
    text = p.read_text()
    def counter(name):
        values = [float(v) for v in re.findall(
            r'^' + name + r'\{[^\n]*\}\s+(\S+)', text, re.M)]
        assert values, (name, text)
        return sum(values)
    events = counter('tidemark_watch_events_total')
    overflows = counter('tidemark_watch_overflows_total')
    if mode == 'watch-counters':
        print(json.dumps(dict(events=events, overflows=overflows)))
    else:
        before = json.loads(pathlib.Path(rest[0]).read_text())
        expected = int(rest[1])
        delta = events - before['events']
        overflow_delta = overflows - before['overflows']
        mounts = json.loads(pathlib.Path(rest[2]).read_text())['data']['mounts']
        mount = next(m for m in mounts if m['mount'] == rest[3])
        if mode == 'burst':
            assert overflow_delta == 0 and delta >= expected, (delta, overflow_delta, expected)
            assert mount['lossy_minutes_15m'] == 0, mount
        else:
            assert overflow_delta > 0 and 0 <= delta < expected, (delta, overflow_delta, expected)
            assert mount['lossy_minutes_15m'] > 0, mount
            hot_rc = int(pathlib.Path(rest[5]).read_text())
            if hot_rc == 4:
                print('hot window unavailable (CLI exit 4)')
            else:
                assert hot_rc == 0, hot_rc
                hot = json.loads(pathlib.Path(rest[4]).read_text())
                assert hot['schema'] == 'tidemark.hot.v1', hot
                rows = hot['data']
                # Empty success cannot establish current-minute loss.
                assert rows and all(row['lossy'] for row in rows), hot
        print(f'events_delta={delta} writes={expected} overflow_delta={overflow_delta}')
elif mode == 'watch-ok':
    components = json.loads(p.read_text())['data']['components']
    row = next(c for c in components if c['name'] == 'watcher:' + rest[0])
    assert row['state'] == 'ok', row
    print(json.dumps(row, sort_keys=True))
elif mode == 'integrity':
    with sqlite3.connect(f'file:{p}?mode=ro', uri=True) as db:
        assert db.execute('PRAGMA integrity_check').fetchone() == ('ok',)
    print('integrity=ok')
else:
    raise ValueError(mode)
