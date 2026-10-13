# Pilot kit

Use [checklist.md](checklist.md) for each host type and [sign-off.md](sign-off.md) for the go/no-go decision. The phases follow [the rollout plan](../plan/11-rollout-runbook.md), with newer root-only, textfile-only monitoring and generic workload measurements. Keep the filled copies in your change repository; do not put host secrets in public documentation.

Before **and after every phase P0–P6**, capture comparable workload intervals. P0 additionally needs a full business day and a batch window from existing monitoring. Name a latency metric with units, percentile, source and interval (for example application request p95 milliseconds); there is no PostgreSQL-specific collector or benchmark gate.

## Capture a baseline

The POSIX script writes only a **new** operator-selected output directory and reads system counters and metadata. Use a directory on approved local evidence storage outside monitored data trees, with enough room, and a new name per capture. Its parent must exist. The script refuses an existing output path, so it cannot overwrite an earlier baseline. Supply the latency label and observed figure yourself from the agreed workload monitor; the script does not execute a user-provided command.

```sh
sudo sh docs/pilot/capture-baseline.sh /var/tmp/tidemark-P0-before 'request p95 ms; monitor A; 10:00-10:01 UTC' '18.4' /data/reports/sample /data/archive/sample
sudo sh docs/pilot/capture-baseline.sh /var/tmp/tidemark-P0-after 'request p95 ms; monitor A; 11:00-11:01 UTC' '19.1' /data/reports/sample /data/archive/sample
```

Replace the example paths, times and figures with real observations. Record unavailable latency as `unavailable` with a reason in the label; it prevents impact sign-off until resolved. Arguments containing spaces must remain quoted. Use the same sample subtrees before/after. Capture the application monitoring series over the actual phase too: two single figures cannot establish sustained impact.

Files include UTC start/end, host/kernel, `iostat -x 5 13`, filesystem byte/inode usage, load, separate `du -sxB1` results for each subtree, and the supplied latency figure. iostat takes roughly one minute; its first report is since boot and should be excluded from interval comparisons. Install `sysstat` through the normal package process if iostat is absent; the script reports failures and exits 1 while preserving other evidence. Bad invocation or an existing output path exits 2. Success exits 0. Every command's status is recorded in `checks.txt`.

`du` scans metadata and can add I/O and directory-atime updates under normal mount options; this is a baseline-tool caveat, not Tidemark scan behavior. Choose small representative subtrees and a quiet window. Never run an unbounded whole-array `du` during an incident. Each subtree is measured in a separate invocation, so shared hard links can occur in several results; do not add them. The script does not drop caches, force writeback, generate writes, change mounts, query/mutate LVM, or manage services.

## Compare and decide

Match workload intensity, batch phase and sample scope. Compare leaf-device await/utilisation and the named latency SLO, not only load average. Abort scans if profile targets stay exceeded after backoff, await exceeds twice baseline for more than two minutes, or application alerts fire; see [scan impact](../runbooks/scan-impact.md).

Preserve raw baseline directories alongside verify JSON, snapshot IDs, config hash/version, journal and monitoring graphs. The kit records evidence; it does not itself certify rollout acceptance. A colleague who did not build Tidemark must perform the staging walkthrough and record whether they answered “what grew yesterday” without assistance.
