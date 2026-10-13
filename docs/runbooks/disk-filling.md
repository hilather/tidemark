# Disk filling

## Symptoms

Free space is falling, a time-to-full alert fires, or writes fail with no space. Use the affected mount in place of `/data`. If application writes already fail, escalate immediately while gathering evidence.

## Commands

```sh
sudo tidemark now --mount /data
sudo tidemark hot --mount /data --window 5m --by dir
sudo tidemark hot --mount /data --window 5m --by unit
sudo tidemark writers --mount /data --window 5m
sudo tidemark diff /data --since 24h --explain
sudo tidemark top /data --depth 2 --sort reclaimable
sudo tidemark files /data --limit 25 --min-size 1GiB
sudo tidemark lvm --mount /data
sudo tidemark alerts
sudo df -h /data
sudo df -i /data
```

## Read the output

`now` shows current filesystem availability; the 15-minute growth rate and one-hour time-to-full use different windows. A positive rate is a trend, not a deadline guarantee. `hot` points to close-write activity; `writers` catches growing descriptors that stay open. Unknown or lossy data cannot establish an idle workload. `diff` uses historical snapshots: check From/To ages and coverage. A missing baseline calls for a gentle scan, not a fabricated yesterday total.

SIZE can count the same hard-linked inode in two siblings. RECLAIM estimates what removal could free, ignoring open handles and reflink extents. `lvm` must be checked even when filesystem free space looks safe: thin-pool data or metadata may fill first. Inode exhaustion is visible in `df -i` even with free bytes.

## Actions

1. Notify the owner of the growing directory/unit. Reduce or pause the producing workload through its approved procedure if capacity is urgent.
2. If scans add load, use [scan impact](scan-impact.md). For stale totals, run `sudo tidemark scan /data --throttle gentle` only when the workload can tolerate it.
3. Before approved cleanup, inspect `sudo tidemark links /data --boundary-depth 1` and identify retention obligations. Removing one link may free nothing. Tidemark never deletes data.
4. If `df` is larger than visible trees, follow [df/du mismatch](df-du-mismatch.md); if the thin pool is filling, follow [thin pool](thin-pool-filling.md).

## Escalation

Send the storage and application owners the mount/device, available bytes/inodes, observed trend and time-to-full, snapshot IDs/times, largest growth path, writer/unit, pool percentages and alert times. Archive `sudo tidemark --json now --mount /data` plus stderr. Do not wait for a full scan before escalating imminent exhaustion.

See [operator guide](../operator-guide.md).
