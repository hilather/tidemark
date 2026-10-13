#!/bin/sh
# Owned by: W7-D1
# Capture read-only pilot observations into a new, private evidence directory.
set -eu
umask 077
if [ "$#" -lt 4 ]; then
    printf '%s\n' 'Usage: capture-baseline.sh OUTPUT_DIR LATENCY_LABEL LATENCY_VALUE SUBTREE [SUBTREE ...]' >&2
    exit 2
fi
pilot_output=$1
pilot_latency_label=$2
pilot_latency_value=$3
shift 3
case "$pilot_output" in
    /*) ;;
    *) pilot_output=$PWD/$pilot_output ;;
esac
# Refuse existing paths, including dangling symlinks; never overwrite evidence.
if [ -e "$pilot_output" ] || [ -L "$pilot_output" ]; then
    printf '%s\n' "Output path already exists: $pilot_output" >&2
    exit 2
fi
if ! mkdir "$pilot_output"; then
    printf '%s\n' 'Cannot create output directory; its parent must exist.' >&2
    exit 2
fi
pilot_failed=0
capture() {
    pilot_file=$1
    shift
    if "$@" > "$pilot_output/$pilot_file" 2>&1; then
        printf '%s: ok\n' "$pilot_file" >> "$pilot_output/checks.txt"
    else
        pilot_status=$?
        printf '%s: failed (%s)\n' "$pilot_file" "$pilot_status" >> "$pilot_output/checks.txt"
        pilot_failed=1
    fi
}
capture started-utc.txt date -u '+%Y-%m-%dT%H:%M:%SZ'
capture host.txt uname -n
capture kernel.txt uname -r
printf 'Metric/source/interval: %s\nObserved value: %s\n' "$pilot_latency_label" "$pilot_latency_value" > "$pilot_output/workload-latency.txt"
capture load-before.txt uptime
capture loadavg-before.txt cat /proc/loadavg
capture df-bytes.txt df -P -B1
capture df-inodes.txt df -P -i
# First report is since boot; subsequent reports cover five-second intervals.
capture iostat.txt iostat -x 5 13
pilot_index=0
for pilot_subtree do
    pilot_index=$((pilot_index + 1))
    case "$pilot_subtree" in
        /*) ;;
        *) pilot_subtree=$PWD/$pilot_subtree ;;
    esac
    printf '%s\n' "$pilot_subtree" > "$pilot_output/subtree-$pilot_index.txt"
    # Separate invocations count links per subtree; do not sum these figures.
    capture "du-$pilot_index.txt" nice -n 19 du -sxB1 -- "$pilot_subtree"
done
capture load-after.txt uptime
capture loadavg-after.txt cat /proc/loadavg
capture finished-utc.txt date -u '+%Y-%m-%dT%H:%M:%SZ'
printf 'Captured evidence in %s\n' "$pilot_output"
if [ "$pilot_failed" -ne 0 ]; then
    printf '%s\n' 'Some measurements failed; inspect checks.txt and the corresponding output.' >&2
    exit 1
fi
