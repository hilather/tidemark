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
    checks=0
    state="$work/fs/state"
    root="$work/fs/tree"
    config="$work/config.toml"
    trap cleanup EXIT
    trap 'exit 130' INT
    trap 'exit 143' TERM
    mkdir "$work/fs"
    mkdir -m 0755 "$work/metrics"
    truncate -s 8G "$work/image"
    run_helper 60 mkfs.xfs -q "$work/image"
    mounted=1
    run_helper 60 mount -o loop "$work/image" "$work/fs"
    run_helper 10 findmnt -n -o SOURCE --target "$work/fs" > "$work/loop"
    read -r loop < "$work/loop"
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
textfile = "$work/metrics/metrics.prom"
interval = "1s"
CONFIG
}
pass() { echo "PASS: $*"; }
fail() { echo "FAIL: $script: $*" >&2; failed=$((failed + 1)); }
check() {
    local label=$1
    shift
    checks=$((checks + 1))
    local evidence_file="$work/check-$checks.log"
    if "$@" > "$evidence_file" 2>&1; then
        pass "$label: $(cat "$evidence_file") exit=0"
    else
        fail "$label: $(cat "$evidence_file")"
    fi
}
# wait is interruptible by Bash traps; no long helper is foregrounded.
run_helper() {
    local seconds=$1 helper_pid helper_rc input_fd
    shift
    exec {input_fd}<&0
    setsid timeout --foreground "$seconds" "$@" <&"$input_fd" &
    helper_pid=$!
    exec {input_fd}<&-
    children+=("$helper_pid")
    if wait "$helper_pid"; then helper_rc=0; else helper_rc=$?; fi
    # Kill any descendants left behind by an early helper exit.
    kill -KILL -- "-$helper_pid" 2>/dev/null || true
    forget_child "$helper_pid"
    return "$helper_rc"
}
cli_json() { run_helper 20 "$cli" -c "$config" --json "$@"; }
start_daemon() {
    local started=$SECONDS
    setsid "$daemon" --config "$config" >> "$work/daemon.log" 2>&1 &
    daemon_pid=$!
    children+=("$daemon_pid")
    local deadline=$((SECONDS + 60))
    until cli_json status > "$work/status.json" 2> "$work/status.err"; do
        (( SECONDS < deadline )) || { fail 'daemon readiness deadline'; exit 1; }
        kill -0 "$daemon_pid" 2>/dev/null || { fail 'daemon exited before readiness'; exit 1; }
        sleep 0.2
    done
    pass "private daemon ready: pid=$daemon_pid elapsed_seconds=$((SECONDS - started))"
}
stop_daemon() {
    kill -TERM "$daemon_pid"
    await_child "$daemon_pid" 30
    [[ $child_rc == 0 ]] || fail "daemon SIGTERM exit $child_rc"
}
await_child() {
    local pid=$1 deadline=$((SECONDS + $2))
    while kill -0 "$pid" 2>/dev/null; do
        (( SECONDS < deadline )) || { fail "child $pid exceeded deadline"; exit 1; }
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
    setsid timeout --foreground 900 "$cli" -c "$config" --json scan "$root" --mode walk --throttle max > "$work/scan.json" 2> "$work/scan.err" &
    scan_pid=$!
    children+=("$scan_pid")
}
fixture() { run_helper 600 python3 "$repo/tests/fault/check.py" fixture "$root" "$1"; }
no_tmp() { run_helper 120 python3 "$repo/tests/fault/check.py" no-tmp "$state"; }
snapshots() { run_helper 120 python3 "$repo/tests/fault/check.py" snapshots "$state"; }
cleanup() {
    local rc=$? child deadline
    trap - EXIT INT TERM
    set +e
    # Each tracked child is a setsid group leader. Signal the whole group,
    # including descendants of timeout, before touching the mounted fixture.
    for child in "${children[@]}"; do
        kill -CONT -- "-$child" 2>/dev/null
        kill -TERM -- "-$child" 2>/dev/null
    done
    deadline=$((SECONDS + 10))
    for child in "${children[@]}"; do
        while kill -0 -- "-$child" 2>/dev/null && (( SECONDS < deadline )); do sleep 0.1; done
        kill -KILL -- "-$child" 2>/dev/null
        wait "$child" 2>/dev/null
        if kill -0 -- "-$child" 2>/dev/null; then
            echo "FAIL: cleanup: process group $child survived SIGKILL" >&2
            rc=1
        fi
    done
    if (( rc != 0 || failed != 0 )); then
        echo "FAIL: $script: exit=$rc recorded_failures=$failed" >&2
        for child in "$work"/*.err "$work"/*.log "$work"/*.json; do
            if [[ -f $child ]]; then cat "$child" >&2; fi
        done
        ls -la "$state" "$state/snapshots" >&2
        cli_json status >&2
        cli_json now >&2
    fi
    if (( mounted )) && findmnt -n --mountpoint "$work/fs" >/dev/null; then
        if [[ -z $loop ]]; then
            loop=$(timeout --foreground 10 findmnt -n -o SOURCE --mountpoint "$work/fs")
        fi
        if ! timeout --foreground 30 umount "$work/fs"; then echo 'FAIL: cleanup: unmount' >&2; rc=1; fi
    fi
    if [[ -n $loop && $(timeout --foreground 10 losetup --list --noheadings --output BACK-FILE "$loop" 2>/dev/null) == "$work/image" ]]; then
        if ! timeout --foreground 10 losetup -d "$loop"; then echo 'FAIL: cleanup: detach loop' >&2; rc=1; fi
    fi
    if (( rc == 0 && failed == 0 )); then
        if [[ ${TIDEMARK_FAULT_KEEP:-0} == 1 ]]; then
            echo "Evidence retained: $work"
        else
            rm -rf -- "$work"
        fi
        echo "RESULT $script: PASS"
    else
        echo "Evidence retained: $work" >&2
        echo "RESULT $script: FAIL"
        rc=1
    fi
    exit "$rc"
}
