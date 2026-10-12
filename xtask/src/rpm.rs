//! Owned by: W6-C
//! Explicit Rocky 8 container packaging and testable command construction.

use anyhow::{bail, Context, Result};
use std::{ffi::OsString, path::Path, process::Command};

/// Cached build image; its Containerfile derives from rockylinux:8.
pub const IMAGE: &str = "tidemark-rpm:rocky8";

/// Assemble cargo-generate-rpm arguments without executing any tool.
pub fn generate_rpm_args(version: &str) -> Result<Vec<String>> {
    if !matches!(version, "0.1.0" | "0.1.1") {
        bail!("RPM version must be 0.1.0 or 0.1.1");
    }
    Ok(vec![
        "generate-rpm".into(),
        "-p".into(),
        "crates/tidemark-cli".into(),
        "--metadata-overwrite".into(),
        "packaging/rpm/rpm.toml".into(),
        "--set-metadata".into(),
        format!("version = \"{version}\""),
        "--payload-compress".into(),
        "gzip".into(),
    ])
}

/// Assemble a no-pull container invocation, preserving non-UTF8 source paths.
pub fn container_args(root: &Path, version: &str) -> Result<Vec<OsString>> {
    let rpm = generate_rpm_args(version)?;
    let mut volume = root.as_os_str().to_os_string();
    volume.push(":/src");
    let script = format!(
        "set -eu; cargo generate-rpm --version >/dev/null 2>&1 || {{ echo 'Install cargo-generate-rpm in the build image' >&2; exit 1; }}; cargo build --offline --locked --release -p tidemark-cli -p tidemarkd; target/release/tidemark completions bash > target/release/tidemark.bash; cargo {}",
        rpm.iter().map(|s| format!("'{}'", s.replace('\'', "'\\''"))).collect::<Vec<_>>().join(" ")
    );
    Ok(vec![
        "run".into(),
        "--rm".into(),
        "--pull=never".into(),
        "--volume".into(),
        volume,
        "--workdir".into(),
        "/src".into(),
        IMAGE.into(),
        "/bin/bash".into(),
        "-lc".into(),
        script.into(),
    ])
}

/// Build an RPM in a pre-provisioned Rocky 8 image; never install tools or pull images.
/// Accepts optional `--container` and `--version 0.1.0|0.1.1`.
pub fn run(root: &Path, args: impl Iterator<Item = String>) -> Result<()> {
    let mut args = args.peekable();
    let mut container = false;
    let mut version = None;
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--container" if !container => container = true,
            "--version" if version.is_none() => {
                version = Some(args.next().context("--version needs a value")?)
            }
            _ => bail!("unknown or duplicate rpm argument: {arg}"),
        }
    }
    let version = version.as_deref().unwrap_or("0.1.0");
    let command = container_args(root, version)?;
    let tool = ["podman", "docker"]
        .into_iter()
        .find(|tool| {
            Command::new(tool)
                .arg("--version")
                .output()
                .is_ok_and(|o| o.status.success())
        })
        .context("Install podman or docker to build the Rocky Linux 8 RPM")?;
    let image = Command::new(tool)
        .args(["image", "inspect", IMAGE])
        .output()?;
    if !image.status.success() {
        bail!("Build cached {IMAGE} explicitly: {tool} build -t {IMAGE} -f packaging/rpm/Containerfile .; install cargo-generate-rpm and populate its offline Cargo cache (see packaging/rpm/README.md)");
    }
    let status = Command::new(tool)
        .args(command)
        .current_dir(root)
        .status()?;
    if !status.success() {
        bail!("Rocky 8 RPM build failed: {status}");
    }
    Ok(())
}
