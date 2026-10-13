# Disposable guest fault suite

Owned by: W7-C2-fault. Run only as root in a disposable Rocky Linux 8 guest,
with `TIDEMARK_DEPLOY_TEST_VM=1`. These scripts create and fill their own loop
XFS images, kill their own processes, and remove only their own temporary data.
They never use systemd, the packaged daemon, its socket, or its state directory.

| Script | Checks | Typical guest duration / deadline |
| --- | --- | --- |
| `01-state-full.sh` | Free-space guard refusal, `state_space` alert, admitted writer interrupted by ENOSPC, no temporary or incomplete publication | 1–5 min; scan deadlines 300 s |
| `02-snapshot-crash.sh` | Twelve fixed kill delays after observing a writer, complete metadata on every published snapshot, startup removes temporaries, subsequent scan succeeds | 2–10 min; 300 s to observe each writer, 900 s recovery |
| `03-fanotify-overflow.sh` | 100,000 distinct close-writes while daemon consumption is stopped, lossy minute, cumulative overflow counter, fresh activity after resume | 1–5 min; burst 120 s, lossy 150 s, recovery 90 s |
| `04-compaction-crash.sh` | SIGKILL during uncommitted compaction WAL spill, database integrity, restart produces independent last-value oracle, second hourly compaction leaves it identical | SKIP by default; slow mode about 4 h, each hourly phase bounded by 3800 s |
| `05-directory-races.sh` | Background subtree deletion produces recorded races, complete valid snapshot, no panic | 1–5 min; scan 900 s |
| `06-huge-directory.sh` | 50,000 entries by default or 1,000,000 with scale marker; direct scan PID peak RSS <= configured 1280 MiB; exact completed file count | 1–5 min normal, 5–20 min scale; fixture 600 s, scan 900 s |

Known baseline concern: inspected healthy daemon startup does not remove orphan
snapshot `.tmp` files. `02-snapshot-crash.sh` asserts the required next-start
cleanup and can expose this product failure; it does not delete those files to
make the check pass. See the handoff for source evidence and remaining doubts.

Permission denial is already covered by the real CLI test
`e2e::scan_reports::scan_to_every_report`: mode-000 fixture, dropped non-root
credentials, error record, degraded complete snapshot, exit 3. No duplicate
script is added. The detailed existing-coverage table is in
`docs/handoff/W7-C2-fault.md`.

The guest needs Bash, python3 (with SQLite), util-linux (`mount`, `umount`,
`losetup`, `findmnt`), xfsprogs (`mkfs.xfs`), and coreutils (`timeout`, `truncate`) and util-linux `setsid`.
The shared harness checks for `mkfs.xfs` before creating resources and reports
how to install the missing dependency. Other executed commands come from the
minimal guest tool set; the Python helpers use only the standard library.
It needs loop devices, root capabilities for mounts/fanotify, the host mount
namespace (the same as PID 1), and at least 10 GiB free backing storage under
`/tmp`. Each script uses an 8 GiB sparse image; the full-state script really
allocates almost all of it. This size allows successful scans above the
product's mandatory 2 GiB free-space floor. The compaction fixture seeds
300,000 rows into its private real schema with the daemon stopped; it never
changes the system clock or uses a fabricated compaction implementation.

Build **in the guest checkout**, as its SSH user:

```sh
cargo build --offline --workspace --features privileged
```

The runner syncs source but excludes `target/` and does not build script suites.
Choose a dedicated remote directory and build there after initial source sync.
For example, from the workstation first sync without running faults:

```sh
rsync -a --exclude target --exclude .git ./ tester@rocky810:tidemark-vm-test/
ssh tester@rocky810 'cd tidemark-vm-test && cargo build --offline --workspace --features privileged'
cargo xtask vm-test --host tester@rocky810 --remote-dir tidemark-vm-test --suite fault --env TIDEMARK_DEPLOY_TEST_VM=1
```

Repeat on the other Rocky guest. For the million-entry case append
`--env TIDEMARK_FAULT_SCALE=1`. The whole suite is fast by default: `04` prints SKIP unless `--env TIDEMARK_FAULT_SLOW=1` is
provided. Slow mode waits on real hourly compaction (the guard prints an
approximate 70-minute budget; the current multi-phase scenario can take about
four hours). The runner runs every script and prints PASS/FAIL, exit status and
seconds per script at the end, exiting nonzero if any script failed. Fast
coverage of the compaction row will be a Rust store test that kills compaction
in a child process, supplied by another task.

Run one script **inside the guest**, from its checkout:

```sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 bash tests/fault/01-state-full.sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_SCALE=1 bash tests/fault/06-huge-directory.sh
```

Every running entrypoint prints check-level PASS/FAIL lines and a final `RESULT`
line; the disabled slow row prints SKIP and exits before allocating resources.
Any failed check or failed cleanup exits nonzero. Failure retains its private
`/tmp/tidemark-fault.*` directory with stderr, JSON responses and daemon log;
the trap prints those plus state listings and current status/now. Inspect the
printed directory before deleting it. No failure means guest acceptance only
after the coordinator reviews the actual transcript.

Local static validation (does not run the faults):

```sh
cargo test --offline -p xtask --test w7c
```

The test runs `bash -n` on every entrypoint, checks VM/root guards and trap
ownership, rejects unsupported command words (`rg`, `jq`, `fd`, `bc`) in shell
and Python helpers recursively, and runs `shellcheck -x` at its default severity when installed.
`common.bash` is sourced and checked through the entrypoints, rather than being
executed as a seventh script by the runner. `check.py` validates public JSON,
SQLite integrity and publication metadata; the public cumulative overflow
counter exists in Prometheus textfile output, so only that check parses metrics.

Checks record failures and continue where the fixture is safe; setup errors and
deadlines stop that script. Snapshot crash iterations compare against the
pre-scan temporary set, report completed-before-kill delays as SKIP, and require
at least three writer hits. Startup cleanup remains an assertion of product
behavior and may fail until the separate product repair lands.

Set `--env TIDEMARK_FAULT_KEEP=1` to retain evidence on PASS as well as FAIL.
PASS lines include judged counts, JSON, metric lines, exit statuses or timing.
The API socket parent remains mode 0700; metrics use a separate mode-0755
`metrics/` parent to avoid competing exporter/API directory locks. Long helpers
run in tracked process groups behind an interruptible `wait`; signal cleanup
kills groups before unmounting and detaching the owned loop.

For slow evidence, inside the disposable guest checkout only:

```sh
sudo env "PATH=$PATH" TIDEMARK_DEPLOY_TEST_VM=1 TIDEMARK_FAULT_KEEP=1 TIDEMARK_FAULT_SLOW=1 bash tests/fault/04-compaction-crash.sh
```
