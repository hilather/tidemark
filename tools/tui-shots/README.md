capture.py runs the tidemark TUI in a private tmux session on a synthetic fixture and saves raw ANSI captures (140x40 plus Mono/ASCII/80x24) into docs/screenshots/tui-tide-chart/.
render.py turns those captures into the PNGs and builds the side-by-side comparisons with Mira's concept-3 mock; only the PNGs are committed.
