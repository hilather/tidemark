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
}
fn parse(args: impl IntoIterator<Item = String>) -> Result<Options> {
    let mut args = args.into_iter();
    let mut host = None;
    let mut remote_dir = None;
    let mut filter = None;
    let mut suite = None;
    while let Some(flag) = args.next() {
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
    Ok(Options {
        host,
        remote_dir,
        filter,
        suite,
    })
}
fn quote(text: &str) -> String {
    format!("'{}'", text.replace('\'', "'\\''"))
}
fn remote_command(o: &Options) -> String {
    let mut script = format!("set -eu; cd -- {}; ", quote(&o.remote_dir));
    if o.suite == "cargo" {
        script.push_str("cargo build --workspace --features privileged --tests; sudo env \"PATH=$PATH\" cargo test --workspace --features privileged -- --test-threads=1");
        if let Some(filter) = &o.filter {
            script.push(' ');
            script.push_str(&quote(filter));
        }
    } else {
        script.push_str(&format!("for script in tests/{}/*.sh; do [ -f \"$script\" ] || {{ echo 'missing script suite' >&2; exit 1; }}; sudo env \"PATH=$PATH\" bash \"$script\"; done",o.suite));
    }
    script
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
    let o = parse(args)?;
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
                suite: "cargo".into()
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
    fn constructs_both_script_suites() {
        for suite in ["install", "fault"] {
            let o = parse(args(&["--host", "vm", "--suite", suite])).unwrap();
            let cmd = remote_command(&o);
            assert!(cmd.contains(&format!("tests/{suite}/*.sh")));
            assert!(cmd.contains("sudo env \"PATH=$PATH\" bash"));
        }
    }
}
