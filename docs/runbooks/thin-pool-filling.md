# Thin pool filling

## Symptoms

Tidemark reports high thin-pool data or metadata percentage, a thin-pool alert fires, or storage writes fail while `df` still reports filesystem headroom. Treat near-exhaustion as urgent.

## Commands

```sh
sudo tidemark lvm
sudo tidemark lvm --mount /data
sudo tidemark now --mount /data
sudo tidemark hot --mount /data --window 5m --by dir
sudo tidemark diff /data --since 24h --explain
sudo tidemark alerts
sudo lvs -o vg_name,lv_name,segtype,lv_size,data_percent,metadata_percent,pool_lv
sudo vgs -o vg_name,vg_size,vg_free
sudo df -h /data
```

Use ordinary LVM reporting commands; do not add `--readonly`, which hides thin-pool usage. Repeat now/hot/diff for each mapped thin LV mount sharing the affected pool.

## Read the output

Pool DATA % is physical data allocation; META % is thin mapping metadata. Either can fill independently of a filesystem's free bytes and a thin LV's virtual size. VG free bytes are potential capacity for an approved expansion, not already usable filesystem space. Other thin LVs can fill a shared pool, so the read-only `lvs` inventory matters even if the tracked mount looks quiet.

Daemon `lvm` omits segment type and other LVs; direct fallback contains more fields. An unknown percentage or missing trend is not zero. A 24-hour trend needs at least three available samples and reports percentage points/day. Current percentages matter more than a reassuring historical slope during a burst.

## Actions

1. Notify the storage administrator and owners of all LVs sharing the pool. Use now/hot/diff to identify growth in the tracked mounts.
2. Reduce/pause the growing workload through its approved procedure if exhaustion is close. Cancel discretionary scans if they are adding impact.
3. Arrange a separate approved storage change to extend pool data/metadata capacity, or application-owned retention cleanup. This diagnostic runbook supplies no LVM mutation commands.
4. Deleting filesystem files may not immediately release thin-pool blocks: discard policy and shared/snapshot extents matter. Have the storage administrator verify actual pool reduction; do not run discard or remove snapshots as an improvised fix.

## Escalation

Provide VG/pool names, both percentages, pool size, VG free bytes, all participating LVs, current filesystem availability, rate/trend timestamps, alert times and identified producers. If either pool resource approaches exhaustion, escalate immediately without waiting for another scan.

See [LVM interpretation](../operator-guide.md#check-lvm-separately-from-filesystem-fullness).
