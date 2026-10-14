//! Owned by: DOCS-fault
//! Static validation of disposable-guest fault entrypoints.

use std::{io::Write, path::Path, process::Command};

#[test]
fn fault_scripts_are_guarded_and_valid_shell() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut scripts: Vec<_> = std::fs::read_dir(root.join("tests/fault"))
        .unwrap()
        .map(|entry| entry.unwrap().path())
        .filter(|path| path.extension().is_some_and(|ext| ext == "sh"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "fault suite must have entrypoints");
    let shellcheck = Command::new("shellcheck").arg("--version").output();
    let has_shellcheck = match shellcheck {
        Ok(output) => {
            assert!(
                output.status.success(),
                "shellcheck --version failed:\n{}\n{}",
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
            true
        }
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => false,
        Err(error) => panic!("cannot execute shellcheck: {error}"),
    };
    for path in scripts {
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(text.starts_with("#!/bin/bash\n"), "{}", path.display());
        assert!(text.contains("set -euo pipefail"), "{}", path.display());
        assert!(
            text.contains("TIDEMARK_DEPLOY_TEST_VM"),
            "{}",
            path.display()
        );
        assert!(text.contains("$EUID == 0"), "{}", path.display());
        // The common harness owns the executable trap; require the delegation too.
        assert!(text.contains("trap") && text.contains("source tests/fault/common.bash"));
        for line in text
            .lines()
            .filter(|line| !line.trim_start().starts_with('#'))
        {
            if line.contains("timeout ") {
                assert!(line.contains("--foreground"), "untracked timeout: {line}");
            }
        }
        if path.file_name().unwrap() == "04-compaction-crash.sh" {
            assert!(text.contains("${TIDEMARK_FAULT_SLOW:-0} != 1"));
            assert!(text.contains(
                "SKIP: 04-compaction-crash.sh needs TIDEMARK_FAULT_SLOW=1 (about 70 minutes)"
            ));
            assert!(text.find("TIDEMARK_FAULT_SLOW").unwrap() < text.find("setup\n").unwrap());
        }
        let output = Command::new("bash").arg("-n").arg(&path).output().unwrap();
        assert!(
            output.status.success(),
            "bash -n {}: {}",
            path.display(),
            String::from_utf8_lossy(&output.stderr)
        );
        if has_shellcheck {
            // Follow the same sources for advisory diagnostics and the warning gate.
            let notes = Command::new("shellcheck")
                .current_dir(root)
                .arg("-x")
                .arg(&path)
                .output()
                .unwrap();
            let mut stderr = std::io::stderr().lock();
            stderr
                .write_all(
                    format!(
                        "shellcheck notes for {}:\n{}\n{}",
                        path.display(),
                        String::from_utf8_lossy(&notes.stdout),
                        String::from_utf8_lossy(&notes.stderr),
                    )
                    .as_bytes(),
                )
                .unwrap();
            stderr.write_all(b"\n").unwrap();
            drop(stderr);
            let output = Command::new("shellcheck")
                .current_dir(root)
                .arg("--severity=warning")
                .arg("-x")
                .arg(&path)
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "shellcheck {}:\n{}\n{}",
                path.display(),
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr)
            );
        }
    }
    let common = std::fs::read_to_string(root.join("tests/fault/common.bash")).unwrap();
    assert!(common.contains("trap cleanup EXIT"));
    assert!(common.contains("mkdir -m 0755 \"$work/metrics\""));
    assert!(common.contains("$work/metrics/metrics.prom"));
    for line in common
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
    {
        if line.contains("timeout ") {
            assert!(line.contains("--foreground"), "untracked timeout: {line}");
        }
    }
    let output = Command::new("bash")
        .arg("-n")
        .arg(root.join("tests/fault/common.bash"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "bash -n common.bash: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

// Deliberately conservative: reject forbidden command words even in quoted
// subprocess argv, absolute paths, and inline shell/Python snippets. This is a
// source guard, not shell evaluation or dynamic command/data-flow analysis.
fn forbidden_guest_tool(source: &str) -> Option<&str> {
    source
        .lines()
        .filter(|line| !line.trim_start().starts_with('#'))
        .flat_map(|line| line.split(|c: char| !c.is_ascii_alphanumeric() && c != '_' && c != '-'))
        .find(|word| matches!(*word, "rg" | "jq" | "fd" | "bc"))
}

#[test]
fn fault_scripts_use_available_guest_tools() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let mut pending = vec![root.join("tests/fault")];
    let mut checked = 0;
    while let Some(directory) = pending.pop() {
        for entry in std::fs::read_dir(directory).unwrap() {
            let entry = entry.unwrap();
            let path = entry.path();
            if entry.file_type().unwrap().is_dir() {
                pending.push(path);
            } else if path
                .extension()
                .is_some_and(|ext| ext == "sh" || ext == "bash" || ext == "py")
            {
                let source = std::fs::read_to_string(&path).unwrap();
                assert_eq!(
                    forbidden_guest_tool(&source),
                    None,
                    "unsupported guest tool in {}",
                    path.display()
                );
                checked += 1;
            }
        }
    }
    assert!(checked >= 9, "must check entrypoints and shared helpers");
    let common = std::fs::read_to_string(root.join("tests/fault/common.bash")).unwrap();
    assert!(common.find("command -v mkfs.xfs").unwrap() < common.find("work=$(mktemp").unwrap());
    assert!(common.contains("install xfsprogs in the disposable guest"));
}

#[test]
fn guest_tool_guard_covers_shell_and_python_commands() {
    for tool in ["rg", "jq", "fd", "bc"] {
        for source in [
            format!("{tool} -q pattern file"),
            format!("if /usr/bin/{tool} file; then :; fi"),
            format!("subprocess.run(['{tool}', 'file'])"),
            format!("os.system(\"{tool} file\")"),
            format!("run_helper 20 {tool} file"),
        ] {
            assert_eq!(forbidden_guest_tool(&source), Some(tool), "{source}");
        }
    }
    assert_eq!(
        forbidden_guest_tool("# rg is unavailable\ngrep -Fq NoSpace file\nchild_fd = 1"),
        None
    );
}

#[test]
fn shellcheck_advisories_survive_passing_harness_and_warnings_fail() {
    use std::os::unix::fs::PermissionsExt;
    let dir = tempfile::tempdir().unwrap();
    let stub = dir.path().join("shellcheck");
    let path = std::env::join_paths(std::iter::once(dir.path().to_owned()).chain(
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default()),
    ))
    .unwrap();
    for fail_warning in [false, true] {
        std::fs::write(&stub, format!(
            "#!/bin/sh\nif [ \"$1\" = --version ]; then exit 0; fi\nfor arg in \"$@\"; do\nif [ \"$arg\" = --severity=warning ]; then echo WARNING_GATE >&2; exit {}; fi\ndone\necho ADVISORY_STDOUT\necho ADVISORY_STDERR >&2\nexit 1\n",
            u8::from(fail_warning),
        )).unwrap();
        std::fs::set_permissions(&stub, std::fs::Permissions::from_mode(0o700)).unwrap();
        // Exact filtering avoids recursively invoking this regression. No nocapture.
        let output = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "fault_scripts_are_guarded_and_valid_shell",
                "--test-threads=1",
            ])
            .env("PATH", &path)
            .output()
            .unwrap();
        assert_eq!(
            output.status.success(),
            !fail_warning,
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(stderr.contains("ADVISORY_STDOUT"), "{stderr}");
        assert!(stderr.contains("ADVISORY_STDERR"), "{stderr}");
        if fail_warning {
            assert!(String::from_utf8_lossy(&output.stdout).contains("WARNING_GATE"));
        }
    }
}

#[test]
fn watcher_helpers_pin_modes_counts_caps_and_recovery_phases() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = Command::new("python3")
        .arg("-c")
        .arg(r#"
import json, pathlib, subprocess, sys, tempfile
helper = sys.argv[1]
with tempfile.TemporaryDirectory() as directory:
    base = pathlib.Path(directory)
    def save(name, data):
        path = base / name
        path.write_text(json.dumps(data))
        return str(path)
    def run(mode, *args, good=True):
        result = subprocess.run([sys.executable, helper, mode, *args], capture_output=True, text=True)
        assert (result.returncode == 0) == good, (mode, args, result.stdout, result.stderr)
        return result.stdout
    for mode in ('fd_mount', 'fd_filesystem', 'dfid_name_mount', 'dfid_name_filesystem', 'failed', 'unknown'):
        status = save('status', {'data': {'watch_modes': [['/mount', mode]]}})
        text = run('watch-mode', status, '/mount', good=mode not in ('failed', 'unknown'))
        if mode != 'unknown':
            assert 'watch_mode=' + mode in text
    before = save('before', {'events': 1000, 'overflows': 2})
    metrics = base / 'metrics'
    def counters(events=1100, overflows=2):
        metrics.write_text('tidemark_watch_events_total{mount="/mount"} %s\ntidemark_watch_overflows_total{mount="/mount"} %s\n' % (events, overflows))
    def now(loss):
        return save('now', {'data': {'mounts': [{'mount': '/mount', 'lossy_minutes_15m': loss}]}})
    def health(state, detail):
        return save('health', {'data': {'components': [{'name': 'watcher:/mount', 'state': state, 'detail': detail}]}})
    counters()
    run('burst', str(metrics), before, '100', now(0), '/mount')
    run('burst', str(metrics), before, '100', now(1), '/mount', good=False)
    counters(1101)
    run('burst', str(metrics), before, '100', now(0), '/mount', good=False)
    counters(1099)
    run('resumed', str(metrics), before, '100', now(1), '/mount', good=False)
    counters(overflows=3)
    run('resumed', str(metrics), before, '100', now(1), '/mount', good=False)
    counters()
    run('resumed', str(metrics), before, '100', now(1), '/mount')
    overflow = 'lost events: queue overflow in the minute 12:00 UTC'
    run('watch-resumed', health('degraded', overflow), '/mount')
    run('watch-ok', health('degraded', overflow), '/mount', good=False)
    run('watch-resumed', health('error', overflow), '/mount', good=False)
    run('watch-resumed', health('degraded', 'lost events: read error'), '/mount', good=False)
    run('watch-ok', health('ok', 'mode=fd/filesystem'), '/mount')
    run('watch-ok', health('ok', overflow), '/mount', good=False)
    cap = 'truncated: distinct-file cap reached in the minute 12:00 UTC (events complete, per-file counts capped)'
    run('distinct-cap', str(metrics), before, '100', now(1), '/mount')
    run('distinct-cap', str(metrics), before, '100', now(0), '/mount', good=False)
    run('watch-cap', health('degraded', cap), '/mount')
    run('watch-cap', health('degraded', cap + '; ' + overflow), '/mount', good=False)
    run('watch-cap', health('degraded', 'truncated: current-key cap reached'), '/mount', good=False)
    run('watch-clean', health('ok', cap), '/mount', good=False)
"#)
        .arg(root.join("tests/fault/check.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let script = std::fs::read_to_string(root.join("tests/fault/03-fanotify-overflow.sh")).unwrap();
    assert!(script.contains("local distinct=20000"));
    assert!(script.contains("n % int(sys.argv[2])"));
    assert!(script.contains("burst distinct-cap distinct-cap"));
    assert!(script.contains("resumed_at=$SECONDS"));
    assert!(script.contains("deadline=$((resumed_at + 120))"));
    assert!(script.contains("check.py resumed"));
    assert!(script.contains("check.py watch-resumed"));
    let slow = script
        .find("if [[ ${TIDEMARK_FAULT_SLOW:-0} == 1 ]]")
        .unwrap();
    assert!(script[slow..].contains("check.py watch-ok"));
    assert!(script[slow..].contains("SKIP: long fifteen-minute history recovery"));
    assert!(!script[..slow].contains("check.py watch-ok"));
}

#[test]
fn second_compaction_requires_new_exact_rows_and_live_daemon() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap();
    let output = Command::new("python3")
        .arg("-c")
        .arg(r#"
import json, os, pathlib, runpy, sqlite3, sys, tempfile, time
helper = sys.argv[1]
subject = '__tidemark_fault_compaction__'
witness = subject + '_second__'
identity = pathlib.Path('/proc/self/stat').read_text().rsplit(')', 1)[1].split()[19]
real_sleep, real_monotonic = time.sleep, time.monotonic
for case in ('unchanged', 'exact', 'wrong_value', 'extra_raw', 'changed_prior', 'wrong_pid_identity', 'dead_pid'):
    with tempfile.TemporaryDirectory() as directory:
        path = pathlib.Path(directory) / 'timeseries.db'
        expected = [[1, 300 * n, 1000000, n, 500000, 1000, 100] for n in range(1000)]
        (path.parent / 'compaction-oracle.json').write_text(json.dumps(expected))
        with sqlite3.connect(str(path)) as db:
            db.execute('CREATE TABLE fs_samples (mount TEXT,res INTEGER,ts INTEGER,size INTEGER,used INTEGER,avail INTEGER,inodes INTEGER,inodes_used INTEGER,PRIMARY KEY(mount,res,ts))')
            db.executemany('INSERT INTO fs_samples VALUES (?,?,?,?,?,?,?,?)', ([subject] + row for row in expected))
        elapsed = [0]
        sleeps = [0]
        def sleep(seconds):
            sleeps[0] += 1
            if sleeps[0] > 1:
                elapsed[0] += 3701
                return
            with sqlite3.connect(str(path)) as db:
                assert db.execute('SELECT count(*) FROM fs_samples WHERE mount=? AND res=0', (witness,)).fetchone()[0] == 600
                if case not in ('unchanged', 'wrong_pid_identity', 'dead_pid'):
                    # Fixture controls exercise the helper oracle, not production compaction.
                    raw = db.execute('SELECT ts,size,used,avail,inodes,inodes_used FROM fs_samples WHERE mount=? ORDER BY ts', (witness,)).fetchall()
                    db.execute('DELETE FROM fs_samples WHERE mount=?', (witness,))
                    for row in (raw[299], raw[599]):
                        db.execute('INSERT INTO fs_samples VALUES (?,?,?,?,?,?,?,?)', [witness, 1, row[0] // 300 * 300] + list(row[1:]))
                    if case == 'wrong_value':
                        db.execute('UPDATE fs_samples SET used=-1 WHERE mount=?', (witness,))
                    if case == 'extra_raw':
                        db.execute('INSERT INTO fs_samples VALUES (?,0,0,0,0,0,0,0)', (witness,))
                    if case == 'changed_prior':
                        db.execute('UPDATE fs_samples SET used=-1 WHERE mount=? AND ts=0', (subject,))
            elapsed[0] += 1
        time.sleep = sleep
        time.monotonic = lambda: elapsed[0]
        sys.argv = [helper, 'idempotent', str(path), str(os.getpid()), identity]
        if case == 'wrong_pid_identity':
            sys.argv[-1] = 'invalid-start-time'
        if case == 'dead_pid':
            sys.argv[-2] = '2147483647'
        try:
            runpy.run_path(helper, run_name='__main__')
        except (AssertionError, ProcessLookupError) as error:
            assert case != 'exact', (case, str(error))
            if case == 'unchanged':
                assert 'no second compaction witness' in str(error), str(error)
            if case == 'changed_prior':
                assert 'earlier compacted rows changed' in str(error), str(error)
            if case in ('wrong_value', 'extra_raw'):
                assert 'no second compaction witness' in str(error), str(error)
        else:
            assert case == 'exact', case + ' falsely passed'
        finally:
            time.sleep, time.monotonic = real_sleep, real_monotonic
"#)
        .arg(root.join("tests/fault/compaction.py"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let script = std::fs::read_to_string(root.join("tests/fault/04-compaction-crash.sh")).unwrap();
    assert!(
        script.contains("idempotent \"$state/timeseries.db\" \"$daemon_pid\" \"$daemon_start\"")
    );
}
