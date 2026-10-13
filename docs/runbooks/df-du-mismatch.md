# df and du disagree

## Symptoms

`df` reports substantially more used space than a tree scan, or planned deletion recovered less than SIZE suggested. All comparisons need matching mount, coverage and observation times.

## Commands

```sh
sudo df -B1 /data
sudo df -i /data
sudo findmnt -T /data -o TARGET,SOURCE,FSTYPE,OPTIONS
sudo tidemark now --mount /data
sudo tidemark phantom --mount /data
sudo tidemark writers --mount /data --window 5m
sudo tidemark snapshots list --root /data
sudo tidemark top /data --wide
sudo tidemark links /data --boundary-depth 1
sudo tidemark status --show-config
```

For a fresh investigation independent of cached process samples:

```sh
sudo tidemark phantom --mount /data --direct
sudo tidemark writers --mount /data --direct --interval 10s
sudo nice -n 19 du -sxB1 /data/reports
sudo tidemark top /data/reports
```

Run `du` only on a named sample subtree in a quiet period: it adds metadata load. Align exclusions and snapshot time before comparing. Separate invocations of `du` on siblings each count a shared inode; adding them double-counts it.

## Read the output

Deleted-open files remain in `df` but have no directory entry for `du` or snapshots. Phantom holder bytes and paths identify who must close them. A limited phantom inventory is not proof of completeness. Growing open files explain a snapshot falling behind live usage. Compare current filesystem totals with the timestamp of the latest complete scan, rather than treating both as simultaneous.

Other causes: filesystem metadata and reservations, inaccessible/excluded paths (including Tidemark state), nested mounts and bind mounts skipped by Tidemark, data hidden below a mount, and races during traversal. Sparse APPARENT is logical length, not disk usage. Reflinks can make summed inode allocation larger than physical extent consumption. Bulk apparent-size lag affects APPARENT, not allocated SIZE. Hard links affect sibling sums and reclamation, not distinct-inode root totals.

## Actions

1. Ask the holding application's owner to rotate/reopen or gracefully close deleted-open files using its supported procedure; deletion of the name again cannot release the handle. Do not truncate `/proc` descriptors.
2. Reconcile scope, excludes, error markers, times and SIZE versus APPARENT. Obtain a new gentle scan in a quiet window if history is stale.
3. Review the probe's reflink capability and link topology before estimating cleanup benefit. Treat RECLAIM as an estimate that excludes open-handle/reflink effects.
4. Do not unmount a production filesystem to inspect hidden data during this investigation; plan a separate storage change if that becomes necessary.

## Escalation

Provide `df` bytes/inodes, snapshot IDs and ages, root/excludes, errors/skipped mounts, phantom holders and limits, writer growth, probe reflink facts and comparable sample `du` results. If unexplained usage persists on a quiescent, equal-coverage tree, ask the filesystem/storage administrator to investigate metadata and extent accounting.

See [size meanings](../operator-guide.md#read-the-size-columns-before-planning-cleanup).
