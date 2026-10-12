#!/bin/sh
if [ "$1" -eq 0 ]; then
    systemctl stop tidemarkd.service || :
    systemctl disable tidemarkd.service || :
fi
