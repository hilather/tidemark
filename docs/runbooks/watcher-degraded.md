# Watcher degraded

## Symptoms

Hot activity is missing or marked lossy, overflow counters increase, watcher health is failed/recovering, or a configured mount is reported `not mounted`. **fd mode alone is normal on EL8.**

## Commands

```sh
sudo tidemark status
sudo tidemark now --mount /data
sudo tidemark hot --mount /data --window 5m
sudo tidemark alerts
sudo tidemark probe --root /data
sudo findmnt -T /data -o TARGET,SOURCE,FSTYPE,OPTIONS
sudo journalctl -u tidemarkd --since '-30 minutes' --no-pager
sudo systemctl show tidemarkd -p MainPID
```

For a namespace investigation, substitute the actual daemon PID from MainPID; compare its mount namespace and mount table with the host's. Do not enter or change namespaces:

```sh
sudo readlink /proc/1/ns/mnt
sudo readlink /proc/1234/ns/mnt
sudo cat /proc/1234/mountinfo
```

## Read the output

Rocky 8.8 uses `fd/mount`: writes through another mount of the same filesystem are not observed. Rocky 8.10 supports filesystem marks in fd mode. Neither needs FID mode to be healthy. A process that exits before attribution lookup is shown as `?`, not proof of watcher failure.

`not mounted` requires checking the exact configured mount point, not merely an existing directory. The daemon does not sample/watch it until it appears and then picks it up automatically. Covered scans are refused while absent. Overflow, recovery, truncation and transport loss mean hot data cannot be used as exact totals. Multi-minute FILES can count the same file once per minute.

Watcher CPU allowance is 5% of one core per 1,000 close-write events/second, separate from idle CPU and workload-throughput impact. Measure actual server impact; this budget is not a universal performance guarantee.

## Actions

1. Confirm the daemon uses the host mount namespace and configured mounts match the host layout. Repair service/config through change control rather than enabling broad capabilities or changing SELinux mode. Tidemark ships no SELinux policy and does not require a particular mode.
2. If the mount is absent, restore it through the storage team's established procedure or correct the configured inventory. No mount commands are part of this runbook.
3. Keep `watch.events = ["close_write"]`; remove investigation-only `modify` events if unnecessary. Consider approved `watch.exclude_prefixes` for irrelevant churn, noting those writes become invisible. Validate config before reload.
4. For a burst/overflow, inspect the producer and allow healthy observations to accumulate. Use snapshots and direct writers for independent accounting; do not infer zero growth from missing hot rows.

<!-- verify after W6-watch-windows -->
Watcher alert rules require complete supported whole-minute windows of 1–60 minutes. Hot growth is unavailable across lost/truncated minutes; overflow rules need reliable boundary counts. The ranked `now` hot list does not validate these windows.
<!-- gap: exact daemon watcher alert windows are pending; not-evaluable rules cannot establish watcher health. -->
<!-- gap: mount-transition rate reset and reversible now degradation remain open; confirm fresh samples after recovery. -->

## Escalation

Send status/now JSON, watcher mode and loss details, journal errors, event rate/CPU, configured events/exclusions, host/daemon namespace comparison, exact mount identity and kernel/probe capabilities. Repeated transport failures or unexplained misses on healthy nonexcluded activity require the coordinator's Rocky VM investigation.

See [operator guide](../operator-guide.md#health-and-troubleshooting).
