#!/bin/sh
systemd-tmpfiles --create /usr/lib/tmpfiles.d/tidemark.conf || :
systemctl daemon-reload || :
