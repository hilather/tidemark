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
        "set -eu; cargo generate-rpm --version >/dev/null 2>&1 || {{ echo 'Install cargo-generate-rpm in the build image' >&2; exit 1; }}; cargo build --offline --locked --release -p tidemark-cli -p tidemarkd; target/release/tidemark completions bash > target/release/tidemark.bash; cargo run --offline --locked -p xtask -- man; cargo {}",
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

/// Parsed RPM build options; container mode remains the default.
#[derive(Debug, PartialEq, Eq)]
pub struct Options {
    /// Build directly on the current host.
    pub native: bool,
    /// Permit Cargo dependency fetching during the release build.
    pub online: bool,
    /// Explicitly bypass the EL8 host guard.
    pub allow_foreign_host: bool,
    /// Use the changed-default install-test package metadata.
    pub upgrade_fixture: bool,
    /// Validated package version.
    pub version: String,
}

/// Parse options without probing the host or executing commands.
pub fn parse_args(args: impl Iterator<Item = String>) -> Result<Options> {
    let mut options = Options {
        native: false,
        online: false,
        allow_foreign_host: false,
        version: "0.1.0".into(),
        upgrade_fixture: false,
    };
    let mut seen = std::collections::BTreeSet::new();
    let mut args = args;
    while let Some(arg) = args.next() {
        if !seen.insert(arg.clone()) {
            bail!("duplicate rpm argument: {arg}");
        }
        match arg.as_str() {
            "--native" => options.native = true,
            "--upgrade-fixture" => options.upgrade_fixture = true,
            "--container" => {}
            "--online" => options.online = true,
            "--allow-foreign-host" => options.allow_foreign_host = true,
            "--version" => options.version = args.next().context("--version needs a value")?,
            _ => bail!("unknown rpm argument: {arg}"),
        }
    }
    if options.native && seen.contains("--container") {
        bail!("--native and --container conflict");
    }
    if !options.native && (options.online || options.allow_foreign_host) {
        bail!("--online and --allow-foreign-host require --native");
    }
    if options.upgrade_fixture && (!options.native || options.version != "0.1.1") {
        bail!("--upgrade-fixture requires --native and --version 0.1.1");
    }
    generate_rpm_args(&options.version)?;
    Ok(options)
}

/// Identify EL8 using injected os-release text, without executing that text.
pub fn is_el8(content: &str) -> bool {
    let value = |key: &str| {
        content
            .lines()
            .find_map(|line| line.trim().strip_prefix(key))
            .map(|v| v.trim().trim_matches(['\"', '\'']))
    };
    matches!(
        value("ID="),
        Some("rocky" | "rhel" | "centos" | "almalinux" | "ol")
    ) && value("VERSION_ID=").is_some_and(|v| v == "8" || v.starts_with("8."))
}

/// One native command and optional stdout destination relative to the workspace.
#[derive(Debug, PartialEq, Eq)]
pub struct NativeCommand {
    /// Executable name or workspace-relative path.
    pub program: String,
    /// Arguments passed without shell interpretation.
    pub args: Vec<String>,
    /// Optional workspace-relative file receiving standard output.
    pub stdout: Option<String>,
}

/// Assemble the ordered build, completion, manual and generator commands.
pub fn native_commands(
    version: &str,
    online: bool,
    upgrade_fixture: bool,
) -> Result<Vec<NativeCommand>> {
    if upgrade_fixture && version != "0.1.1" {
        bail!("--upgrade-fixture requires version 0.1.1");
    }
    let mut rpm_args = generate_rpm_args(version)?;
    if upgrade_fixture {
        rpm_args[4] = "tests/install/rpm-upgrade.toml".into();
    }
    let mut build = vec![
        "build",
        "--locked",
        "--release",
        "-p",
        "tidemark-cli",
        "-p",
        "tidemarkd",
    ]
    .into_iter()
    .map(String::from)
    .collect::<Vec<_>>();
    if !online {
        build.push("--offline".into());
    }
    Ok(vec![
        NativeCommand {
            program: "cargo".into(),
            args: build,
            stdout: None,
        },
        NativeCommand {
            program: "target/release/tidemark".into(),
            args: vec!["completions".into(), "bash".into()],
            stdout: Some("target/release/tidemark.bash".into()),
        },
        NativeCommand {
            program: "cargo".into(),
            args: {
                let mut args = vec!["run", "--locked", "-p", "xtask"]
                    .into_iter()
                    .map(String::from)
                    .collect::<Vec<_>>();
                if !online {
                    args.push("--offline".into());
                }
                args.extend(["--".into(), "man".into()]);
                args
            },
            stdout: None,
        },
        NativeCommand {
            program: "cargo".into(),
            args: rpm_args,
            stdout: None,
        },
    ])
}

/// Build natively on EL8 or in the existing pre-provisioned container.
pub fn run(root: &Path, args: impl Iterator<Item = String>) -> Result<()> {
    let options = parse_args(args)?;
    if options.native {
        if !options.allow_foreign_host
            && !is_el8(
                &std::fs::read_to_string("/etc/os-release")
                    .context("read /etc/os-release (use --allow-foreign-host to override)")?,
            )
        {
            bail!("Native RPM builds require Rocky Linux 8 or another EL8; use --allow-foreign-host to override");
        }
        if !Command::new("cargo")
            .args(["generate-rpm", "--version"])
            .current_dir(root)
            .output()
            .is_ok_and(|o| o.status.success())
        {
            bail!("Install cargo-generate-rpm 0.20.0 before running rpm --native");
        }
        for spec in native_commands(&options.version, options.online, options.upgrade_fixture)? {
            let mut command = Command::new(&spec.program);
            command.args(&spec.args).current_dir(root);
            if let Some(path) = spec.stdout {
                command.stdout(std::fs::File::create(root.join(path))?);
            }
            let status = command
                .status()
                .with_context(|| format!("run {}", spec.program))?;
            if !status.success() {
                bail!("Native RPM command {} failed: {status}", spec.program);
            }
        }
        return Ok(());
    }
    let command = container_args(root, &options.version)?;
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
