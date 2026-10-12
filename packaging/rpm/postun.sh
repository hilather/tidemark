#!/bin/sh
systemctl daemon-reload || :
if [ "$1" -ge 1 ]; then
    systemctl try-restart tidemarkd.service || :
fi
