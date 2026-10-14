# Tidemark

**Disk-usage intelligence for Rocky Linux 8 / XFS on LVM.**

[![CI](https://github.com/hilather/tidemark/actions/workflows/ci.yml/badge.svg)](https://github.com/hilather/tidemark/actions/workflows/ci.yml)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Rust](https://img.shields.io/badge/rust-1.98%2B-orange.svg)](https://www.rust-lang.org/)

Tidemark is a careful, metadata-only observability tool for large XFS filesystems. It answers the questions that `df`, `du`, and `lsof` leave half-answered on production storage servers:

- What is filling the filesystem *right now*?
- Which directories and processes are responsible for growth?
- What can actually be reclaimed once hard links and open handles are taken into account?
- How healthy are the underlying LVM thin pools?
- How has the tree changed since the last known-good snapshot?

It never reads file contents, never follows symlinks, never crosses mount points, and never writes to the filesystems it observes. Scans are gated and throttled so they remain gentle on RAID arrays.

---

## Features

- **Metadata-only scans** with optional XFS bulkstat acceleration
- **Hard-link-correct accounting** — SIZE, CHARGED, RECLAIM, and SHARED columns that add up correctly
- **Live growth attribution** via fanotify (hot directories, writers, deleted-but-open “phantom” files)
- **Snapshot catalog and diffs** with explainable drill-down of growth
- **LVM thin-pool and VG monitoring** with trend estimates
- **Alerts** evaluated against filesystem, growth, and thin-pool rules
- **Interactive TUI** and `ncdu`-compatible export
- **Production-oriented deployment** with staged config, service enablement, and post-reboot verification
- Strict security model: root-owned state, no unnecessary privileges, ancestry checks, no symlink following

---

## Getting Started

### Install (RPM, recommended for production)

```sh
sudo dnf install ./tidemark-0.1.0-1.el8.x86_64.rpm
sudo tidemark probe --root / /data
```

The package leaves the `tidemarkd` service **disabled and stopped**. Review the probe output, then stage a configuration and reboot through your normal maintenance window:

```sh
sudo tidemark deploy stage --from ./tidemark.toml
# reboot
sudo tidemark deploy verify --wait 5m
sudo tidemark now --mount /data
```

See the full [operator guide](docs/operator-guide.md) for configuration, verification, and runbooks.

### Build from source

Requires Rust 1.98+ (pinned in `rust-toolchain.toml`).

```sh
git clone https://github.com/hilather/tidemark.git
cd tidemark
cargo build --release -p tidemark-cli
# binary at target/release/tidemark
```

Privileged tests and full VM validation are available via `cargo xtask`.

---

## How It Works

Tidemark observes storage at three complementary levels:

1. **Sampler** — lightweight periodic samples of free space, inode usage, and LVM state.
2. **Scanner** — bounded, metadata-only walks (or XFS bulkstat) that produce immutable snapshots. Snapshots record allocated sizes, hard-link topology, and directory structure without reading file contents.
3. **Watcher** — optional fanotify stream that attributes close-write events to directories and (when possible) systemd units or processes. Used for “hot” and “writers” views.

All I/O is gated. Directories are opened with `O_NOATIME` (falling back gracefully). Symlinks are never followed. Mount points are never crossed.

The accounting model distinguishes:

| Column   | Meaning |
|----------|---------|
| SIZE     | Allocated bytes of distinct inodes visible in the subtree (comparable to `du -sx`) |
| APPARENT | Logical size (sparse files can be much larger) |
| CHARGED  | Bytes assigned once at the lowest common ancestor of their hard links |
| RECLAIM  | Bytes whose entire link set lives inside the subtree (estimated deletion benefit) |
| SHARED   | Bytes kept alive by links outside the subtree |

See `docs/plan/03-link-semantics.md` for the normative worked example.

---

## Key Commands

```sh
tidemark now --mount /data          # current situation + growth + LVM
tidemark hot --mount /data --window 5m
tidemark scan /data --mode auto     # create a snapshot
tidemark top /data --depth 1
tidemark diff /data --since 24h --explain
tidemark links /data
tidemark lvm --mount /data
tidemark phantom --mount /data
tidemark writers --mount /data
tidemark status
tidemark tui
```

Run `tidemark --help` or `man tidemark` for the full surface.

---

## Architecture

Rust workspace:

- `tidemark-core` — shared types and contracts
- `tidemark-sys` — the only crate allowed to use `unsafe`
- `tidemark-fs` / `tidemark-scan` — filesystem traversal and bulkstat
- `tidemark-store` — snapshot catalog and state
- `tidemark-live` — fanotify and live attribution
- `tidemark-cli` — the `tidemark` binary
- `tidemarkd` — the long-running daemon

Configuration lives in `/etc/tidemark/tidemark.toml`. State defaults to `/var/lib/tidemark`.

---

## Documentation

- [Operator guide](docs/operator-guide.md) — installation, configuration, day-to-day commands, and examples
- [Security model](docs/security.md)
- [Runbooks](docs/runbooks/)
- Design documents under `docs/plan/`

---

## License

MIT. See [LICENSE](LICENSE).

---

Built for operators who need precise, low-impact answers about large XFS filesystems.
