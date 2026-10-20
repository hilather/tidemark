//! Owned by: W0-A
//! Workspace checks and future task command entry points.

#![forbid(unsafe_code)]

mod bench;
mod checks;
mod gen_fixture;
mod man;
mod rpm;
mod ts_size;
mod vm_test;

use anyhow::{bail, Context, Result};
use std::{env, path::Path, process::Command};

fn run(root: &Path, args: &[&str]) -> Result<()> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
    let status = Command::new(cargo).args(args).current_dir(root).status()?;
    if !status.success() {
        bail!("cargo {} failed: {status}", args.join(" "));
    }
    Ok(())
}

fn parse_ci_args(args: impl IntoIterator<Item = String>) -> Result<bool> {
    let mut require_deny = false;
    for arg in args {
        match arg.as_str() {
            "--require-deny" if !require_deny => require_deny = true,
            _ => bail!("usage: cargo xtask ci [--require-deny]"),
        }
    }
    Ok(require_deny)
}

#[derive(Debug, PartialEq, Eq)]
enum DenyAction {
    Check,
    Warn,
}

fn deny_action(installed: bool, require_deny: bool) -> Result<DenyAction> {
    match (installed, require_deny) {
        (true, _) => Ok(DenyAction::Check),
        (false, false) => Ok(DenyAction::Warn),
        (false, true) => bail!("cargo-deny is required by --require-deny but is not installed"),
    }
}

fn main() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must be inside workspace")?;
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("ci") => {
            let require_deny = parse_ci_args(args)?;
            checks::check_sources(root)?;
            run(root, &["fmt", "--check"])?;
            run(
                root,
                &[
                    "clippy",
                    "--workspace",
                    "--all-targets",
                    "--",
                    "-D",
                    "warnings",
                ],
            )?;
            run(
                root,
                &[
                    "test",
                    "-p",
                    "tidemark-store",
                    "--features",
                    "strict-ancestry",
                    "--lib",
                    "strict_feature_tests",
                ],
            )?;
            run(root, &["test", "--workspace", "--no-fail-fast"])?;
            run(
                root,
                &[
                    "build",
                    "--workspace",
                    "--features",
                    "privileged",
                    "--tests",
                ],
            )?;
            let cargo = env::var_os("CARGO").unwrap_or_else(|| "cargo".into());
            let installed = Command::new(cargo)
                .args(["deny", "--version"])
                .output()?
                .status
                .success();
            match deny_action(installed, require_deny)? {
                DenyAction::Check => run(root, &["deny", "check"])?,
                DenyAction::Warn => {
                    eprintln!("warning: cargo-deny is not installed; skipping cargo deny check");
                }
            }
            println!("ci: all available checks passed");
            Ok(())
        }
        Some("vm-test") => vm_test::run(root, args),
        Some("gen-fixture") => gen_fixture::run(env::args_os().skip(2)),
        Some("bench") => bench::run(root, env::args_os().skip(2)),
        Some("man") => man::run(root, env::args_os().skip(2)),
        Some("rpm") => rpm::run(root, args),
        Some("ts-size") => ts_size::run(env::args_os().skip(2)),
        _ => bail!("usage: cargo xtask <ci|vm-test|gen-fixture|bench|rpm|man|ts-size>"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ci_arguments_accept_only_optional_require_deny() {
        assert!(!parse_ci_args(Vec::new()).unwrap());
        assert!(parse_ci_args(vec!["--require-deny".into()]).unwrap());
        assert!(parse_ci_args(vec!["--unknown".into()]).is_err());
        assert!(parse_ci_args(vec!["--require-deny".into(), "--require-deny".into()]).is_err());
        assert!(parse_ci_args(vec!["--require-deny".into(), "extra".into()]).is_err());
    }

    #[test]
    fn missing_deny_warns_unless_required() {
        assert_eq!(deny_action(false, false).unwrap(), DenyAction::Warn);
        assert!(deny_action(false, true)
            .unwrap_err()
            .to_string()
            .contains("--require-deny"));
        assert_eq!(deny_action(true, false).unwrap(), DenyAction::Check);
        assert_eq!(deny_action(true, true).unwrap(), DenyAction::Check);
    }
}
