# Tidemark

Tidemark is a metadata-only disk-usage intelligence tool for Rocky Linux 8 / XFS
servers. The `tidemark` CLI queries snapshots and the root daemon `tidemarkd`.
Administrators use `sudo`: config, API socket and snapshots are root-only.

Installation does not start or enable the service. Validate the fleet default
(`/` and `/data`) or prepare a host-specific configuration before staging:

```sh
sudo tidemarkd --check-config --config /etc/tidemark/tidemark.toml
sudo tidemark deploy stage --from ./tidemark.toml
# Reboot during the ordinary maintenance cycle, then:
sudo tidemark deploy verify --wait 5m
sudo tidemark status
```

Use `deploy stage --start-now` for immediate startup, or `deploy unstage` to stop
and disable while retaining collected state. RPM removal likewise keeps
`/var/lib/tidemark`. Config is installed with config(noreplace); upgrades retain
administrator edits.

This package includes `07-live-monitoring.md`, `08-interfaces.md` and
`11-rollout-runbook.md` in this directory. They describe configuration, public
interfaces, deployment checks and operational rollout. Tidemark makes no fstab,
LVM or boot configuration changes. Its daemon retains the host mount namespace
so fanotify marks observe writers in other services.
