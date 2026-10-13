# Packaging

RPM and service assets are owned by W6-C. Tidemark access is root only; administrators run the CLI with sudo. No packaging is installed by this scaffold.

W6-C owns `systemd/`, `tmpfiles.d/`, `config/`, `rpm/` and `tests/install/`.
W6-B owns `grafana/` and `prometheus/`. The `xtask rpm` module is wired and
returns an explicit preparation error until W6-C implements it.

RPM generation uses the external `cargo generate-rpm` tool, not a Rust library
attachment. Container builds require podman or docker and a Rocky Linux 8 image;
install, reboot and systemd validation run only in the test VMs. The unit must
retain the host mount namespace (ADR-013). No group creation or PostgreSQL assets
are shipped (ADR-018/019).

Tidemark v1 uses only its Unix API socket. Metrics are exported through the node_exporter textfile collector; alert delivery is through the journal. HTTP endpoints and webhooks were removed in ADR-020; old configuration keys are rejected.
