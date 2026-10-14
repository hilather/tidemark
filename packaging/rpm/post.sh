#!/bin/sh
systemd-tmpfiles --create /usr/lib/tmpfiles.d/tidemark.conf || :
systemctl daemon-reload || :
if [ "${1:-0}" -eq 1 ]; then
    printf '%s\n' 'Tidemark installed. Configure /etc/tidemark/tidemark.toml, then: tidemark deploy stage --from <file> --start-now (service: tidemarkd)'
fi
