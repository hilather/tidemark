# Owned by: W7-C2-fault. Shared disposable-guest harness (not a runner entrypoint).
# shellcheck shell=bash
setup() {
    umask 077
    script=$(basename "$0")
    repo=$(pwd)
    cli="$repo/target/debug/tidemark"
    daemon="$repo/target/debug/tidemarkd"
    [[ -x $cli && -x $daemon ]] || {
        echo 'FAIL: binaries missing: build as SSH user with cargo build --offline --workspace --features privileged (runner does not build)' >&2
        echo "RESULT $script: FAIL"
        exit 1
    }
    work=$(mktemp -d /tmp/tidemark-fault.XXXXXXXX)
    chmod 0700 "$work"
    children=()
    mounted=0
    loop=''
    failed=0
    state="$work/fs/state"
    root="$work/fs/tree"
    config="$work/config.toml"
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    mkdir "$work/fs"
    truncate -s 8G "$work/image"
    timeout 60 mkfs.xfs -q "$work/image"
    timeout 60 mount -o loop "$work/image" "$work/fs"
    mounted=1
    loop=$(timeout 10 findmnt -n -o SOURCE --target "$work/fs")
    mkdir -m 0700 "$state" "$root"
    write_config false
}
write_config() {
    cat > "$config" <<CONFIG
[general]
state_dir = "$state"
socket = "$work/api.sock"
log_level = "debug"
[[roots]]
path = "$root"
mode = "walk"
schedule = "off"
throttle = "max"
[scan]
threads = 1
memory_budget = "1280MiB"
[sampler]
mounts = ["$work/fs"]
interval = "1s"
[lvm]
enabled = false
[procscan]
enabled = false
[watch]
enabled = $1
mounts = ["$work/fs"]
[metrics]
textfile = "$work/metrics.prom"
interval = "1s"
CONFIG
}
pass() { echo "PASS: $*"; }
fail() { echo "FAIL: $script: $*" >&2; failed=1; exit 1; }
check() {
    local label=$1
    shift
    if "$@"; then pass "$label"; else fail "$label"; fi
}
cli_json() { timeout 20 "$cli" -c "$config" --json "$@"; }
start_daemon() {
    "$daemon" --config "$config" >> "$work/daemon.log" 2>&1 &
    daemon_pid=$!
    children+=("$daemon_pid")
    local deadline=$((SECONDS + 60))
    until cli_json status > "$work/status.json" 2> "$work/status.err"; do
        (( SECONDS < deadline )) || fail 'daemon readiness deadline'
        kill -0 "$daemon_pid" 2>/dev/null || fail 'daemon exited before readiness'
        sleep 0.2
    done
    pass 'private daemon ready'
}
stop_daemon() {
    kill -TERM "$daemon_pid"
    await_child "$daemon_pid" 30
    [[ $child_rc == 0 ]] || fail "daemon SIGTERM exit $child_rc"
}
await_child() {
    local pid=$1 deadline=$((SECONDS + $2))
    while kill -0 "$pid" 2>/dev/null; do
        (( SECONDS < deadline )) || fail "child $pid exceeded deadline"
        sleep 0.1
    done
    if wait "$pid"; then child_rc=0; else child_rc=$?; fi
    forget_child "$pid"
}
forget_child() {
    local pid=$1 next=() child
    for child in "${children[@]}"; do
        if [[ $child != "$pid" ]]; then next+=("$child"); fi
    done
    children=("${next[@]}")
}
scan_background() {
    timeout 900 "$cli" -c "$config" --json scan "$root" --mode walk --throttle max > "$work/scan.json" 2> "$work/scan.err" &
    scan_pid=$!
    children+=("$scan_pid")
}
fixture() { timeout 600 python3 "$repo/tests/fault/check.py" fixture "$root" "$1"; }
no_tmp() { python3 "$repo/tests/fault/check.py" no-tmp "$state"; }
snapshots() { python3 "$repo/tests/fault/check.py" snapshots "$state"; }
cleanup() {
    local rc=$? child deadline
    trap - EXIT INT TERM
    set +e
    if (( rc != 0 )); then
        echo "FAIL: $script: exit $rc" >&2
        for child in "$work"/*.err "$work"/*.log "$work"/*.json; do
            if [[ -f $child ]]; then cat "$child" >&2; fi
        done
        ls -la "$state" "$state/snapshots" >&2
        cli_json status >&2
        cli_json now >&2
    fi
    for child in "${children[@]}"; do kill -CONT "$child" 2>/dev/null; kill -TERM "$child" 2>/dev/null; done
    deadline=$((SECONDS + 10))
    for child in "${children[@]}"; do
        while kill -0 "$child" 2>/dev/null && (( SECONDS < deadline )); do sleep 0.1; done
        if kill -0 "$child" 2>/dev/null; then kill -KILL "$child" 2>/dev/null; fi
        deadline=$((SECONDS + 10))
        while kill -0 "$child" 2>/dev/null && (( SECONDS < deadline )); do sleep 0.1; done
        if kill -0 "$child" 2>/dev/null; then
            echo "FAIL: cleanup: child $child survived SIGKILL" >&2
            rc=1
        else
            wait "$child" 2>/dev/null
        fi
    done
    if (( mounted )); then
        if ! timeout 30 umount "$work/fs"; then echo 'FAIL: cleanup: unmount' >&2; rc=1; fi
    fi
    if [[ -n $loop && $(timeout 10 losetup --list --noheadings --output BACK-FILE "$loop" 2>/dev/null) == "$work/image" ]]; then
        if ! timeout 10 losetup -d "$loop"; then echo 'FAIL: cleanup: detach loop' >&2; rc=1; fi
    fi
    if (( rc == 0 && failed == 0 )); then
        rm -rf -- "$work"
        echo "RESULT $script: PASS"
    else
        echo "Evidence retained: $work" >&2
        echo "RESULT $script: FAIL"
        rc=1
    fi
    exit "$rc"
}
