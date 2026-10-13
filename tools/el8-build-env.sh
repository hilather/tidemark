#!/bin/sh
# Owned by: CI-rpm. CHANGES THE SYSTEM: disposable build containers/guests only.
set -eu
cd "$(dirname "$0")/.."
# os-release is a trusted system file in this disposable guest.
# shellcheck disable=SC1091 # Provided by the disposable EL8 guest, not the source tree.
. /etc/os-release
case "${ID:-}:${VERSION_ID:-}" in
    rocky:8|rocky:8.*|rhel:8|rhel:8.*|centos:8|centos:8.*|almalinux:8|almalinux:8.*|ol:8|ol:8.*) ;;
    *) echo 'Requires an EL8 disposable build container or guest' >&2; exit 1 ;;
esac
[ "$(id -u)" = 0 ] || { echo 'Run as root in the disposable guest' >&2; exit 1; }
el=${1:-8.10}
case "$el" in
    8.8)
        # shellcheck disable=SC2016 # Match literal yum variables in repository URLs.
        sed -i -e 's|^mirrorlist=|#mirrorlist=|' \
            -e 's|^#baseurl=http://dl.rockylinux.org/\$contentdir/\$releasever/|baseurl=https://dl.rockylinux.org/vault/rocky/8.8/|' \
            /etc/yum.repos.d/*.repo
        if grep -H -E '^(mirrorlist|metalink)=' /etc/yum.repos.d/*.repo; then
            echo 'Active mirrors remain after vault setup' >&2; exit 1
        fi
        ;;
    8.10) ;;
    *) echo 'Expected 8.8 or 8.10' >&2; exit 1 ;;
esac
dnf install -y gcc gcc-c++ make git tar gzip cpio curl ca-certificates rpm-build
export PATH="${CARGO_HOME:-$HOME/.cargo}/bin:$PATH"
toolchain=$(sed -n 's/^channel = "\([^"]*\)"/\1/p' rust-toolchain.toml)
[ -n "$toolchain" ] || { echo 'Missing pinned toolchain' >&2; exit 1; }
if ! command -v cargo >/dev/null 2>&1; then
    installer=$(mktemp)
    trap 'rm -f "$installer"' EXIT HUP INT TERM
    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o "$installer"
    sh "$installer" -y --profile minimal --default-toolchain "$toolchain" --component rustfmt --component clippy
fi
if ! cargo generate-rpm --version >/dev/null 2>&1; then
    cargo install cargo-generate-rpm --version 0.20.0 --locked
fi
cat /etc/os-release
rpm -q glibc
gcc --version
rustc --version
cargo generate-rpm --version
