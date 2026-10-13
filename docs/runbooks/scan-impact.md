# Scan impact or a slow scan

## Symptoms

During a scan, workload latency rises, array await or utilisation stays high, or scan progress becomes slow. After Ctrl-C, a direct scan may still be writing its durable record.

## Commands

```sh
sudo tidemark status
sudo tidemark now
sudo iostat -x 5 25
sudo uptime
sudo journalctl -u tidemarkd --since '-30 minutes' --no-pager
sudo tidemark snapshots list --root /data
```

Compare the [pilot baseline](../pilot/README.md) and the application's named latency metric over equivalent workload intervals. For confirmed unacceptable scan impact:

```sh
sudo tidemark scan /data --cancel
```

## Read the output

Adaptive throttling intentionally makes scans slow on busy disks. `gentle` targets 50% leaf utilisation and 15ms average latency; `normal` targets 70% and 25ms. Short excursions can occur while the controller reacts. `max` disables throttling. Missing leaf-device discovery uses a conservative fixed rate; read the journal. Check whether the activity is scan I/O, a writer burst, RAID maintenance or state-disk publication.

Abort the pilot scan if leaf utilisation/average wait persist above the selected profile targets after backoff, await is more than twice baseline for over two minutes, or application alerts fire. Record your application's SLO threshold before the pilot.

Direct Ctrl-C stops scan work within a second; exit can take longer during snapshot/catalog publication and returns 3. `--keep-partial` is direct walk-only and partial data is not a growth baseline. Ctrl-C on a forwarded scan stops CLI waiting and leaves the daemon scan running. Explicit `--cancel` cancels the current scan for the named root: it cannot atomically select a scan ID, so inspect current activity first.

## Actions

1. Cancel the affected root if abort criteria hold, and record scan ID, time and workload impact. Avoid repeated hard kills while durable records are written.
2. In controlled config, set that root's throttle to `gentle`, lower `scan.threads`, tighten `[throttle.profiles.gentle]` targets or move its schedule/window. A window controls starting, not finishing. Set `schedule = "off"` temporarily if approved.
3. Validate the installed config, reload, then check health and journal:

```sh
sudo tidemarkd --check-config --config /etc/tidemark/tidemark.toml
sudo systemctl reload tidemarkd
sudo tidemark status --show-config
sudo journalctl -u tidemarkd --since '-5 minutes' --no-pager
```

`--show-config` labels CLI config; invalid reload keeps old daemon settings. Do not assume reload succeeded just because the CLI sees the edited file. Forwarded scans cannot override threads, extra excludes or keep-partial.

4. Keep the 1280 MiB default scan budget below the packaged 2 GiB service limit with room for live components. On cap/memory refusal, reduce scope or seek measured tuning. Use walk when current apparent sizes matter; bulk lag alone does not indicate a hung scan.

## Escalation

Supply before/during/after latency, iostat leaf-device series, load, journal, root/mode/throttle/threads, memory budget, scan ID, elapsed time and cancel/exit times. Escalate persistent kernel/storage stalls or publication delay to the storage owner; the scan-work bound does not bound blocked disk writes.

See [operator tuning](../operator-guide.md#tune-scans-against-workload-impact).
