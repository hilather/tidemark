#!/bin/sh
# Owned by: CI-rpm. Inspect an RPM without installing it.
set -eu
export LC_ALL=C
if ! { [ "$#" -ge 1 ] && [ "$#" -le 2 ]; }; then
    echo 'Usage: check-rpm.sh file.rpm [version]' >&2
    exit 1
fi
package=$(realpath "$1")
version=${2:-0.1.0}
# shellcheck disable=SC2164 # set -e exits if this directory change fails.
cd "$(dirname "$0")/.."
fail() { echo "FAIL: $*" >&2; exit 1; }
ok() { echo "OK: $*"; }
[ "$(rpm -qp --qf '%{NAME} %{VERSION} %{RELEASE} %{ARCH}' "$package")" = "tidemark $version 1.el8 x86_64" ] || fail 'identity'
ok 'name, version, release, architecture'
tmp=$(mktemp -d)
trap 'rm -rf "$tmp"' EXIT HUP INT TERM
# Assets are single-line inline tables. Read the manifest, never duplicate its list.
awk '/^[[:space:]]*\{ source =/ {
    n=split($0,a,"\""); path=a[4]; mode=a[6]; user=a[8]; group=a[10];
    flags=""; if ($0 ~ /config = "noreplace"/) flags="cn";
    if ($0 ~ /doc = true/) flags="d";
    print path, mode, user, group, flags
}' packaging/rpm/rpm.toml > "$tmp/expected"
[ -s "$tmp/expected" ] || fail 'empty asset manifest'
rpm -qp --qf '[%{FILENAMES} %{FILEMODES:octal} %{FILEUSERNAME} %{FILEGROUPNAME} %{FILEFLAGS:fflags}\n]' "$package" > "$tmp/actual"
while read -r path mode user group flags; do
    # RPM modes include the regular-file type bits (0100000).
    expected_mode=$(printf '%o' "$((0100000 + mode))")
    expected="$path $expected_mode $user $group"
    actual=$(awk -v p="$path" '$1==p {print $1, $2, $3, $4}' "$tmp/actual")
    [ "$actual" = "$expected" ] || fail "asset mode/owner: $path"
    actual_flags=$(awk -v p="$path" '$1==p {print $5}' "$tmp/actual")
    [ "$actual_flags" = "$flags" ] || fail "asset flags: $path"
    ok "asset: $path"
done < "$tmp/expected"
awk '{print $1}' "$tmp/expected" | sort > "$tmp/expected-paths"
awk '{print $1}' "$tmp/actual" | sort > "$tmp/actual-paths"
cmp -s "$tmp/expected-paths" "$tmp/actual-paths" || fail 'unexpected payload files'
ok 'exact payload file list'
rpm -qp --requires "$package" > "$tmp/requires"
grep -E '^systemd([[:space:]]|$)' "$tmp/requires" >/dev/null || fail 'systemd requirement'
ok 'systemd requirement'
grep -Fx '/bin/sh' "$tmp/requires" >/dev/null || fail '/bin/sh requirement'
ok '/bin/sh requirement'
for pair in 'POSTIN post.sh' 'PREUN preun.sh' 'POSTUN postun.sh'; do
    # shellcheck disable=SC2086 # Split each fixed pair into the RPM tag and script name.
    set -- $pair
    rpm -qp --qf "%{$1}" "$package" > "$tmp/script"
    cp "packaging/rpm/$2" "$tmp/source"
    # Command substitution normalizes the terminal newline only.
    [ "$(cat "$tmp/script")" = "$(cat "$tmp/source")" ] || fail "scriptlet: $1"
    case "$(rpm -qp --qf "[%{$1PROG}]" "$package")" in
        ''|/bin/sh) ;;
        *) fail "scriptlet interpreter: $1" ;;
    esac
    ok "scriptlet: $1"
done
rpm2cpio "$package" > "$tmp/payload.cpio"
mkdir "$tmp/payload"
(cd "$tmp/payload" && cpio -idm --quiet < "$tmp/payload.cpio")
for binary in tidemark tidemarkd; do
    file="$tmp/payload/usr/bin/$binary"
    if ! { [ -f "$file" ] && [ -x "$file" ]; }; then
        fail "executable: $binary"
    fi
    magic=$(od -An -tx1 -N4 "$file" | tr -d ' \n')
    [ "$magic" = 7f454c46 ] || fail "ELF: $binary"
    # ELF e_type is ET_EXEC (2) or ET_DYN (3, PIE), little-endian x86_64.
    type=$(od -An -tx1 -j16 -N2 "$file" | tr -d ' \n')
    case "$type" in 0200|0300) ;; *) fail "ELF executable type: $binary" ;; esac
    ok "ELF executable: $binary"
done
