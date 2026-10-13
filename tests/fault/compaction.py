#!/usr/bin/env python3
# Owned by: W7-C2-fault. Real daemon compaction spill detection and independent oracle.
import json
import os
import pathlib
import signal
import sqlite3
import sys
import time

mode, path, *args = sys.argv[1:]
subject = '__tidemark_fault_compaction__'
manifest = pathlib.Path(path).parent / 'compaction-oracle.json'

def connect():
    return sqlite3.connect(path, timeout=2)

def rows(db):
    return db.execute('SELECT res,ts,size,used,avail,inodes,inodes_used FROM fs_samples WHERE mount=? ORDER BY res,ts', (subject,)).fetchall()

if mode == 'seed':
    # 300,000 raw rows across 1000 aligned five-minute buckets, three days old.
    base = (int(time.time()) - 3 * 86400) // 300 * 300 - 300000
    expected = [[1, base + n * 300, 1000000, n * 300 + 299, 500000, 1000, 100] for n in range(1000)]
    manifest.write_text(json.dumps(expected))
    with connect() as db:
        db.execute('DELETE FROM fs_samples WHERE mount=?', (subject,))
        db.executemany('INSERT INTO fs_samples (mount,res,ts,size,used,avail,inodes,inodes_used) VALUES (?,0,?,1000000,?,500000,1000,100)', ((subject, base + n, n) for n in range(300000)))
        db.commit()
        db.execute('PRAGMA wal_checkpoint(TRUNCATE)')
elif mode == 'kill':
    pid, delay = int(args[0]), float(args[1])
    wal = pathlib.Path(path + '-wal')
    # Daemon readiness happens after the writer starts; initial sampler settles.
    time.sleep(3590)
    baseline = wal.stat().st_size if wal.exists() else 0
    deadline = time.monotonic() + 120
    while time.monotonic() < deadline:
        os.kill(pid, 0)
        size = wal.stat().st_size if wal.exists() else 0
        if size > baseline:
            with connect() as db:
                raw = db.execute('SELECT count(*) FROM fs_samples WHERE mount=? AND res=0', (subject,)).fetchone()[0]
            if raw == 300000:
                # Spill has begun but historical rows have not committed.
                time.sleep(delay)
                with connect() as db:
                    assert db.execute('SELECT count(*) FROM fs_samples WHERE mount=? AND res=0', (subject,)).fetchone()[0] == 300000, 'compaction committed before kill delay elapsed'
                os.kill(pid, signal.SIGKILL)
                print(f'PASS: observed uncommitted WAL spill; SIGKILL after {delay}s')
                break
            raise AssertionError('compaction committed before spill observation; no proven hit')
        time.sleep(0.01)
    else:
        raise AssertionError('no uncommitted compaction spill within hourly deadline')
elif mode in ('complete', 'idempotent'):
    expected = json.loads(manifest.read_text())
    if mode == 'idempotent':
        # Wait beyond the writer next hourly tick, then compare the exact historical set.
        deadline = time.monotonic() + 3660
        while time.monotonic() < deadline:
            time.sleep(min(1, max(0, deadline - time.monotonic())))
    deadline = time.monotonic() + (60 if mode == 'idempotent' else 3700)
    while time.monotonic() < deadline:
        with connect() as db:
            actual = [list(row) for row in rows(db)]
        if actual == expected:
            print(f'PASS: {mode}: 1000 exact last-value buckets')
            break
        time.sleep(1)
    else:
        raise AssertionError(f'compaction differs from oracle: {len(actual)} rows')
else:
    raise ValueError(mode)
