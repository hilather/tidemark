#!/bin/sh
set -eu
if [ -n "${RUST_TOOLCHAIN_BIN:-}" ]; then
    PATH="$RUST_TOOLCHAIN_BIN:$PATH"
    export PATH
fi
cd "$(dirname "$0")/.."
exec cargo xtask ci "$@"
