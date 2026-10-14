//! Owned by: W0-C
//! Validated SSH/rsync command construction and streamed VM test execution.
use anyhow::{bail, Context, Result};
use std::{
    path::Path,
    process::{Command, ExitStatus},
};
#[derive(Debug, PartialEq, Eq)]
struct Options {
    host: String,
    remote_dir: String,
    filter: Option<String>,
    suite: String,
    env: Vec<String>,
    nocapture: bool,
    no_fail_fast: bool,
}
fn parse(args: impl IntoIterator<Item = String>) -> Result<Options> {
    let mut args = args.into_iter();
    let mut host = None;
    let mut remote_dir = None;
    let mut filter = None;
    let mut suite = None;
    let mut env = Vec::new();
    let mut nocapture = false;
    let mut no_fail_fast = false;
    while let Some(flag) = args.next() {
        if flag == "--no-fail-fast" {
            if no_fail_fast {
                bail!("duplicate --no-fail-fast");
            }
            no_fail_fast = true;
            continue;
        }
        if flag == "--nocapture" {
            if nocapture {
                bail!("duplicate --nocapture");
            }
            nocapture = true;
            continue;
        }
        if flag == "--env" {
            let pair = args.next().context("--env requires NAME=VALUE")?;
            let (name, _) = pair.split_once('=').context("--env requires NAME=VALUE")?;
            let suffix = name.strip_prefix("TIDEMARK_").unwrap_or_default();
            if suffix.is_empty()
                || !suffix
                    .bytes()
                    .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_')
                || pair.contains('\0')
            {
                bail!("--env name must match TIDEMARK_[A-Z0-9_]+ and value must not contain NUL");
            }
            env.push(pair);
            continue;
        }
        let slot = match flag.as_str() {
            "--host" => &mut host,
            "--remote-dir" => &mut remote_dir,
            "--filter" => &mut filter,
            "--suite" => &mut suite,
            _ => bail!("unknown vm-test argument: {flag}"),
        };
        if slot.is_some() {
            bail!("duplicate {flag}");
        }
        let value = args
            .next()
            .with_context(|| format!("{flag} requires a value"))?;
        if value.is_empty() || value.contains(['\0', '\n', '\r']) {
            bail!("invalid {flag} value");
        }
        *slot = Some(value);
    }
    let host = host.context("vm-test requires --host <ssh-host>")?;
    if host.starts_with('-')
        || !host
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"@._-".contains(&b))
    {
        bail!("host must be an SSH alias or user@hostname");
    }
    let remote_dir = remote_dir.unwrap_or_else(|| "tidemark-vm-test".into());
    if remote_dir.starts_with('-') || remote_dir == "/" || remote_dir.contains(':') {
        bail!("invalid remote directory");
    }
    if filter
        .as_ref()
        .is_some_and(|pattern| pattern.starts_with('-'))
    {
        bail!("filter must be a test substring, not a test-runner option");
    }
    let suite = suite.unwrap_or_else(|| "cargo".into());
    if !["cargo", "install", "fault"].contains(&suite.as_str()) {
        bail!("suite must be cargo, install or fault");
    }
    if suite != "cargo" && filter.is_some() {
        bail!("--filter is supported only for --suite cargo");
    }
    if suite != "cargo" && nocapture {
        bail!("--nocapture is supported only for --suite cargo");
    }
    if suite != "cargo" && no_fail_fast {
        bail!("--no-fail-fast is supported only for --suite cargo");
    }
    Ok(Options {
        host,
        remote_dir,
        filter,
        suite,
        env,
        nocapture,
        no_fail_fast,
    })
}
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
fn remote_command(o: &Options) -> String {
    let mut script = format!("set -eu; cd -- {}; ", quote(&o.remote_dir));
    let mut sudo_env = String::from("sudo env \"PATH=$PATH\"");
    for pair in &o.env {
        sudo_env.push(' ');
        sudo_env.push_str(&quote(pair));
    }
    let mut build_env = String::from("env");
    for pair in &o.env {
        build_env.push(' ');
        build_env.push_str(&quote(pair));
    }
    if o.suite == "cargo" {
        let fail_fast = if o.no_fail_fast {
            " --no-fail-fast"
        } else {
            ""
        };
        script.push_str(&format!("{build_env} cargo build --workspace --features privileged --tests; {sudo_env} cargo test --workspace --features privileged{fail_fast} -- --test-threads=1"));
        if let Some(filter) = &o.filter {
            script.push(' ');
            script.push_str(&quote(filter));
        }
        if o.nocapture {
            script.push_str(" --nocapture");
        }
    } else {
        script.push_str(&format!(
            "failed=0; passed=0; skipped=0; summary=$(mktemp); log=$(mktemp); trap 'rm -f \"$summary\" \"$log\"' EXIT; for script in tests/{}/*.sh; do [ -f \"$script\" ] || {{ echo 'missing script suite' >&2; exit 1; }}; started=$(date +%s); if bash -o pipefail -c 'log=$1; shift; \"$@\" 2>&1 | tee \"$log\"' -- \"$log\" {sudo_env} bash \"$script\"; then rc=0; else rc=$?; fi; reason=$(sed -n '/^SKIP:/{{p;q;}}' \"$log\"); if [ \"$rc\" -ne 0 ]; then result=FAIL; failed=1; elif [ -n \"$reason\" ]; then result=SKIP; skipped=$((skipped + 1)); else result=PASS; passed=$((passed + 1)); fi; elapsed=$(($(date +%s) - started)); printf '%s %s exit=%s seconds=%s\\n' \"$result\" \"$script\" \"$rc\" \"$elapsed\" >> \"$summary\"; if [ \"$result\" = SKIP ]; then printf '%s\\n' \"$reason\" >> \"$summary\"; fi; done; cat \"$summary\"; printf 'passed=%s skipped=%s\\n' \"$passed\" \"$skipped\"; exit \"$failed\"",
            o.suite
        ));
    }
    script
}
/// Construct a VM command without contacting a host, for contract tests.
#[cfg(test)]
pub fn test_command(args: Vec<String>, identity: &[String]) -> Result<String> {
    let mut options = parse(args)?;
    options.env.extend_from_slice(identity);
    Ok(remote_command(&options))
}
fn success(status: ExitStatus, step: &str) -> Result<()> {
    if !status.success() {
        if let Some(code) = status.code() {
            std::process::exit(code);
        }
        bail!("{step} terminated by signal");
    }
    Ok(())
}
/// Sync the workspace and stream the selected test suite on a disposable VM.
/// A failed subprocess's numeric exit code is preserved.
pub fn run(root: &Path, args: impl IntoIterator<Item = String>) -> Result<()> {
    let mut o = parse(args)?;
    let (commit, date, _) = crate::rpm::build_identity(root);
    let identity = [
        format!("TIDEMARK_BUILD_COMMIT={commit}"),
        format!("TIDEMARK_BUILD_DATE={date}"),
    ];
    std::fs::write(
        root.join(".tidemark-build-id"),
        format!("{}\n{}\n", identity[0], identity[1]),
    )?;
    o.env.retain(|pair| {
        !pair.starts_with("TIDEMARK_BUILD_COMMIT=") && !pair.starts_with("TIDEMARK_BUILD_DATE=")
    });
    o.env.extend(identity);
    let mkdir = format!("mkdir -p -- {}", quote(&o.remote_dir));
    success(
        Command::new("ssh").args(["--", &o.host, &mkdir]).status()?,
        "remote mkdir",
    )?;
    success(
        Command::new("rsync")
            .args([
                "-az",
                "--protect-args",
                "--exclude=target/",
                "--exclude=.git/",
                "--exclude=.git",
                "--",
            ])
            .arg(format!("{}/", root.display()))
            .arg(format!("{}:{}/", o.host, o.remote_dir))
            .status()?,
        "rsync",
    )?;
    success(
        Command::new("ssh")
            .args(["--", &o.host, &remote_command(&o)])
            .status()?,
        "remote suite",
    )
}
#[cfg(test)]
mod tests {
    use super::*;
    fn args(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| (*s).to_owned()).collect()
    }
    #[test]
    fn validates_arguments_without_contacting_host() {
        assert!(test_command(args(&["--host", "vm"]), &[]).is_ok());
        for v in [
            vec![],
            vec!["--host"],
            vec!["--host", "-oProxyCommand=x"],
            vec!["--host", "vm", "--suite", "bad"],
            vec!["--host", "vm", "--unknown", "x"],
            vec!["--host", "vm", "--host", "other"],
            vec!["--host", "vm", "--suite", "fault", "--filter", "x"],
            vec!["--host", "vm", "--remote-dir", "/"],
            vec!["--host", "vm", "--filter", "--ignored"],
        ] {
            assert!(parse(args(&v)).is_err(), "{v:?}");
        }
        assert_eq!(
            parse(args(&["--host", "tester@vm"])).unwrap(),
            Options {
                host: "tester@vm".into(),
                remote_dir: "tidemark-vm-test".into(),
                filter: None,
                suite: "cargo".into(),
                env: Vec::new(),
                nocapture: false,
                no_fail_fast: false
            }
        );
    }
    #[test]
    fn constructs_cargo_command_with_safe_shell_quoting_and_sudo_path() {
        let o = parse(args(&[
            "--host",
            "vm",
            "--remote-dir",
            "/tmp/a'b c",
            "--filter",
            "$(touch /bad); 'filter'",
        ]))
        .unwrap();
        let cmd = remote_command(&o);
        assert!(cmd.contains("cd -- '/tmp/a'\\''b c'"));
        assert!(cmd.contains("cargo build --workspace --features privileged --tests"));
        assert!(cmd.contains("sudo env \"PATH=$PATH\" cargo test"));
        assert!(cmd.ends_with("'$(touch /bad); '\\''filter'\\'''"));
        assert!(cmd.contains("--test-threads=1"));
        assert!(!cmd.contains("--ignored"));
    }
    #[test]
    fn validates_environment_names_before_connection() {
        for pair in [
            "PATH=x",
            "TIDEMARK_=x",
            "TIDEMARK_lower=x",
            "TIDEMARK_A-B=x",
            "TIDEMARK_A;=x",
            "TIDEMARK_é=x",
            "TIDEMARK_A",
            "TIDEMARK_A=x\0",
        ] {
            assert!(
                parse(args(&["--host", "vm", "--env", pair])).is_err(),
                "{pair:?}"
            );
        }
        assert!(parse(args(&["--host", "vm", "--env"])).is_err());
        assert!(parse(args(&["--host", "vm", "--suite", "fault", "--nocapture"])).is_err());
        assert!(parse(args(&["--host", "vm", "--nocapture", "--nocapture"])).is_err());
        for pair in [
            "TIDEMARK_A=x",
            "TIDEMARK_0_=",
            "TIDEMARK_W1A_MD_DEVICE=/dev/md127",
        ] {
            assert_eq!(
                parse(args(&["--host", "vm", "--env", pair])).unwrap().env,
                [pair]
            );
        }
    }
    #[test]
    fn places_quoted_environment_and_nocapture_in_test_command() {
        let o = parse(args(&[
            "--host",
            "vm",
            "--env",
            "TIDEMARK_A=a b'c;$(false)",
            "--env",
            "TIDEMARK_B=x=y",
            "--filter",
            "probe",
            "--nocapture",
        ]))
        .unwrap();
        assert_eq!(remote_command(&o),
            "set -eu; cd -- 'tidemark-vm-test'; env 'TIDEMARK_A=a b'\\''c;$(false)' 'TIDEMARK_B=x=y' cargo build --workspace --features privileged --tests; sudo env \"PATH=$PATH\" 'TIDEMARK_A=a b'\\''c;$(false)' 'TIDEMARK_B=x=y' cargo test --workspace --features privileged -- --test-threads=1 'probe' --nocapture");
        for suite in ["install", "fault"] {
            let o = parse(args(&[
                "--host",
                "vm",
                "--suite",
                suite,
                "--env",
                "TIDEMARK_A=x",
            ]))
            .unwrap();
            assert!(remote_command(&o).contains("sudo env \"PATH=$PATH\" 'TIDEMARK_A=x' bash"));
        }
    }
    #[test]
    fn no_fail_fast_is_a_cargo_test_argument() {
        for extras in [
            vec![],
            vec!["--filter", "probe"],
            vec!["--nocapture", "--env", "TIDEMARK_A=x"],
            vec!["--filter", "probe", "--nocapture", "--env", "TIDEMARK_A=x"],
        ] {
            let mut input = vec!["--host", "vm", "--no-fail-fast"];
            input.extend(extras);
            let cmd = remote_command(&parse(args(&input)).unwrap());
            let (build, test) = cmd.split_once("; sudo env").unwrap();
            assert!(!build.contains("--no-fail-fast"));
            assert!(test.contains(
                "cargo test --workspace --features privileged --no-fail-fast -- --test-threads=1"
            ));
            assert_eq!(cmd.matches("--no-fail-fast").count(), 1);
        }
    }
    #[test]
    fn rejects_no_fail_fast_for_scripts_and_duplicates() {
        for suite in ["install", "fault"] {
            assert!(parse(args(&["--host", "vm", "--suite", suite, "--no-fail-fast"])).is_err());
        }
        assert!(parse(args(&["--host", "vm", "--no-fail-fast", "--no-fail-fast"])).is_err());
    }
    #[test]
    fn script_suite_collects_failures_and_prints_all_summaries() {
        let temp = tempfile::tempdir().unwrap();
        let scratch = temp.path();
        std::fs::create_dir_all(scratch.join("tests/fault")).unwrap();
        std::fs::create_dir_all(scratch.join("bin")).unwrap();
        std::fs::write(scratch.join("tests/fault/01.sh"), "exit 7\n").unwrap();
        std::fs::write(scratch.join("tests/fault/02.sh"), "echo second-ran\n").unwrap();
        // A fake sudo strips env/PATH arguments; no privileged operation runs.
        let sudo = scratch.join("bin/sudo");
        std::fs::write(&sudo, "#!/bin/sh\nshift\nshift\nexec \"$@\"\n").unwrap();
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&sudo, std::fs::Permissions::from_mode(0o755)).unwrap();
        let o = parse(args(&[
            "--host",
            "vm",
            "--suite",
            "fault",
            "--remote-dir",
            scratch.to_str().unwrap(),
        ]))
        .unwrap();
        let output = Command::new("sh")
            .args(["-c", &remote_command(&o)])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    scratch.join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap();
        let stdout = String::from_utf8(output.stdout).unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(stdout.contains("second-ran"), "{stdout}");
        assert!(
            stdout.contains("FAIL tests/fault/01.sh exit=7 seconds="),
            "{stdout}"
        );
        assert!(
            stdout.contains("PASS tests/fault/02.sh exit=0 seconds="),
            "{stdout}"
        );
        std::fs::write(scratch.join("tests/fault/01.sh"), "exit 0\n").unwrap();
        let output = Command::new("sh")
            .args(["-c", &remote_command(&o)])
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    scratch.join("bin").display(),
                    std::env::var("PATH").unwrap()
                ),
            )
            .output()
            .unwrap();
        assert!(output.status.success());
        assert!(
            stdout.find("second-ran").unwrap() < stdout.find("FAIL tests/fault/01.sh").unwrap(),
            "summaries must follow all script output: {stdout}"
        );
    }
    #[test]
    fn constructs_both_script_suites() {
        for suite in ["install", "fault"] {
            let o = parse(args(&["--host", "vm", "--suite", suite])).unwrap();
            let cmd = remote_command(&o);
            assert!(cmd.contains(&format!("tests/{suite}/*.sh")));
            assert!(cmd.contains("sudo env \"PATH=$PATH\" bash"));
        }
    }
}
