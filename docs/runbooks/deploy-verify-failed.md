# Deployment verification failed

## Symptoms

After the maintenance reboot, `deploy verify` exits 6, the firstboot record is stale/unhealthy, or the hotfix pipeline cannot establish readiness. Exit 5 from **stage** is a precondition refusal, not a verify timeout.

## Commands

```sh
sudo tidemark --json deploy verify --wait 5m
sudo tidemark status
sudo systemctl status tidemarkd --no-pager
sudo journalctl -u tidemarkd --since '-30 minutes' --no-pager
sudo tidemarkd --check-config --config /etc/tidemark/tidemark.toml
sudo findmnt -t xfs -o TARGET,SOURCE,OPTIONS
sudo df -h / /data
sudo cat /var/lib/tidemark/deploy/staged.json
sudo cat /var/lib/tidemark/deploy/firstboot.json
```

Use the configured state path if changed. Treat records and logs as sensitive root-only incident evidence.

## Read the output

Inspect each reported check, not only the exit code. Compare staged and firstboot config hash prefixes; a mismatch means firstboot is stale relative to staged. Check timestamps/reboot, installed configuration and component errors. RPM installation alone does not enable or start the daemon. Stage enables it for next boot; only explicit `--start-now` asks for immediate start.

A configured but missing exact mount is not sampled/watched and keeps verification unhealthy; an ordinary `/data` directory is not sufficient. State storage must be eligible and have headroom. Invalid configuration and unit/parser failures appear in the journal. `status --show-config` describes CLI settings, not proof of accepted daemon config.

<!-- gap: firstboot records can be rewritten on later starts with changed configuration; do not use their existence alone as proof of the intended reboot/staged generation. -->

## Actions

1. Keep the rollout gate failed until the checks are understood. Retrying verify alone cannot fix missing storage or invalid config.
2. For config corrections, edit the controlled candidate, validate, and restage:

```sh
sudo tidemarkd --check-config --config ./tidemark.toml
sudo tidemark deploy stage --from ./tidemark.toml
```

Use the approved reboot cycle, or deliberately authorized `--start-now`, then repeat verify. Blocking pre-flight refusal changes nothing. Later stage failure attempts best-effort compensation and may leave config/service changes or downtime; inspect both initiating and compensation errors before retrying.

3. Have the storage owner restore missing mounts through the existing procedure; presence is detected automatically. Confirm fresh observations before trusting rates after remount.
4. If the rollout must be backed out:

```sh
sudo tidemark deploy unstage
```

This stops/disables the service and keeps state for investigation. Unstage remains usable on storage ineligible for stage. Do not purge incident evidence or loosen root-only permissions to make verification pass.

## Escalation

Attach verify JSON/exit/stderr, staged/firstboot records, installed/candidate config hashes, package version, reboot times, status, mount inventory, state free space and journal. Coordinator must review failed runtime compensation or generation mismatch before fleet rollout.

See [staging guidance](../operator-guide.md#stage-reboot-verify) and [pilot sign-off](../pilot/sign-off.md).
