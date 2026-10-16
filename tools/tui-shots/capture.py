#!/usr/bin/env python3
"""Owned by: W7-A. Capture actual TUI panes in a dedicated, locally created tmux server.
Run after exporting TIDEMARK_TIDE_CAPTURE_DIR=/tmp/tidemark-tide-visual and building tidemark.
"""
import json
import os
from pathlib import Path
import shlex
import subprocess
import tempfile
import time

REPO_ROOT = Path(__file__).resolve().parents[2]
OUT = REPO_ROOT / 'docs' / 'screenshots' / 'tui-tide-chart'
REPO = REPO_ROOT
FIXTURE = Path(os.environ.get('TIDEMARK_TIDE_CAPTURE_DIR', '/tmp/tidemark-tide-visual'))
ROOT = str(FIXTURE / 'tree')
# A fresh private socket guarantees that cleanup cannot target another run's server.
SOCKET_DIR = tempfile.TemporaryDirectory(prefix='tidemark-tide-capture-')
TMUX = ['tmux', '-S', str(Path(SOCKET_DIR.name) / 'tmux.sock')]
BINARY = REPO / 'target/debug/tidemark'
waits = []

def tmux(*args):
    return subprocess.check_output(TMUX + list(args), text=True)

def command(color=True, ascii=False, palette='rgb'):
    term = 'linux' if ascii else 'xterm' if palette == 'basic' else 'xterm-256color'
    return shlex.join(['env', '-u', 'NO_COLOR', 'TERM=' + term,
                      'COLORTERM=' + ('truecolor' if palette == 'rgb' else ''), 'LANG=' + ('C' if ascii else 'C.UTF-8'),
                      *([] if color else ['NO_COLOR=1']), str(BINARY),
                      '-q', '--config', str(FIXTURE / 'config.toml'), '--color', 'always',
                      'tui', str(FIXTURE / 'tree')])

def keys(*args, pane='tide:0'):
    tmux('send-keys', '-t', pane, *args)

def capture(name, expected, pane='tide:0', chart=True):
    start = time.monotonic()
    attempts = 0
    while time.monotonic() - start < 15:
        plain = tmux('capture-pane', '-p', '-t', pane)
        chart_values = plain.splitlines()[9] if len(plain.splitlines()) > 9 else ''
        attempts += 1
        if all(part in plain for part in expected) and (not chart or not any(mark in chart_values for mark in ('…', '...'))):
            raw = tmux('capture-pane', '-e', '-p', '-t', pane)
            (OUT / (name + '.ansi')).write_text(raw)
            waits.append(dict(name=name, seconds=round(time.monotonic()-start, 4), polls=attempts,
                              dimensions=tmux('display-message', '-p', '-t', pane, '#{pane_width}x#{pane_height}').strip(),
                              loading_check='neither Unicode nor ASCII loading marker in value label row; expected subject/state present'))
            return
        time.sleep(.05)
    raise RuntimeError(f'{name} did not settle: {plain}')

tmux('-f', '/dev/null', 'new-session', '-d', '-s', 'tide', '-x', '140', '-y', '40', command())
try:
    capture('projects', [f'tide · {ROOT}/projects · disk', '41.7 GiB'])
    keys('Down'); capture('logs-missing', [f'tide · {ROOT}/logs · disk', 'absent path'])
    keys('Up', 'Enter'); capture('descend', [f'tide · {ROOT}/projects/deep · disk', '3.20 GiB'])
    keys('BSpace'); capture('ascend', [f'tide · {ROOT}/projects · disk', '41.7 GiB'])
    keys('End'); capture('own', ['whole directory', '(files here)', '181 GiB'])
    keys('Home', 'i'); capture('info', [f'info · {ROOT}/projects', 'Disk 41.7 GiB'])
    keys('Left', 'p', 'Down', 'Down', 'Enter'); capture('older', ['Snapshot switched', '39.7 GiB', '● 10-09 13h'])
    keys('p', 'Home', 'Enter'); capture('restored', ['Snapshot switched', '● 10-09 15h'])
    keys('R'); capture('live', ['◆ live', 'live values; delta against snapshot', 'vs snapshot'])
    predecessor = next((FIXTURE / 'state/snapshots').glob('*/20260101T000000Z-00000006.db'))
    parked = predecessor.with_suffix('.pruned')
    predecessor.rename(parked)
    try:
        tmux('new-window', '-d', '-t', 'tide:3', '-n', 'pruned', command())
        capture('pruned-startup', ['n/a: cannot open snapshot', '41.7 GiB'], pane='tide:pruned')
        keys('q', pane='tide:pruned')
    finally:
        parked.rename(predecessor)
    tmux('new-window', '-d', '-t', 'tide:1', '-n', 'mono', command(False))
    capture('mono', ['41.7 GiB', 'tide ·'], pane='tide:mono')
    tmux('new-window', '-d', '-t', 'tide:2', '-n', 'ascii', command(True, True))
    capture('ascii', ['41.7 GiB', '+tide .'], pane='tide:ascii')
    for index, palette in enumerate(['basic', 'indexed'], start=4):
        tmux('new-window', '-d', '-t', f'tide:{index}', '-n', palette, command(palette=palette))
        capture(palette, ['41.7 GiB', 'tide ·'], pane=f'tide:{palette}')
    tmux('resize-window', '-t', 'tide:mono', '-x', '80', '-y', '24')
    capture('compact', ['41.7 GiB', 'hw 181 GiB'], pane='tide:mono', chart=False)
    environment = dict(waits=waits, tmux=tmux('-V').strip(), locale='C.UTF-8; ASCII uses C',
                       colors='RGB: xterm-256color/truecolor; Indexed: xterm-256color; Basic: xterm; NO_COLOR=1 for Mono',
                       fixture=str(FIXTURE), binary=str(BINARY), timestamp=time.strftime('%Y-%m-%dT%H:%M:%SZ', time.gmtime()),
                       command=command(), kernel=os.uname().release)
    (FIXTURE / 'capture.json').write_text(json.dumps(environment, indent=2) + '\n')
finally:
    # This server/socket and all its panes were started by this script.
    # Send normal quit to each TUI, then close only our private server.
    for pane in ['tide:0', 'tide:mono', 'tide:ascii', 'tide:pruned', 'tide:basic', 'tide:indexed']:
        subprocess.run(TMUX + ['send-keys', '-t', pane, 'q'], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    subprocess.run(TMUX + ['kill-server'], check=False, stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    SOCKET_DIR.cleanup()
