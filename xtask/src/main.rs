//! Owned by: W0-A
//! Workspace checks and future task command entry points.

#![forbid(unsafe_code)]

mod bench;
mod checks;
mod gen_fixture;
mod man;
mod rpm;
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

fn main() -> Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .context("xtask must be inside workspace")?;
    let mut args = env::args().skip(1);
    match args.next().as_deref() {
        Some("ci") => {
            if args.next().is_some() {
                bail!("ci takes no arguments");
            }
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
            run(root, &["test", "--workspace"])?;
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
            if Command::new(cargo)
                .args(["deny", "--version"])
                .output()?
                .status
                .success()
            {
                run(root, &["deny", "check"])?;
            } else {
                eprintln!("warning: cargo-deny is not installed; skipping cargo deny check");
            }
            println!("ci: all available checks passed");
            Ok(())
        }
        Some("vm-test") => vm_test::run(root, args),
        Some("gen-fixture") => gen_fixture::run(env::args_os().skip(2)),
        Some("bench") => bench::run(root, env::args_os().skip(2)),
        Some("man") => man::run(root, env::args_os().skip(2)),
        Some("rpm") => rpm::run(root, args),
        _ => bail!("usage: cargo xtask <ci|vm-test|gen-fixture|bench|rpm|man>"),
    }
}
