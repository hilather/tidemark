//! Owned by: W7-D2
//! Deterministic manuals from the binaries' clap definitions.

use anyhow::{bail, Context, Result};
use clap::CommandFactory;
use clap_mangen::roff::{roman, Roff};
use std::{ffi::OsString, io::Write, path::Path};

/// Generate manuals in the default directory or the explicitly selected directory.
pub fn run(root: &Path, args: impl Iterator<Item = OsString>) -> Result<()> {
    let mut args = args;
    let out = match args.next() {
        None => root.join("target/man"),
        Some(arg) if arg == "--out" => args.next().context("--out needs a directory")?.into(),
        Some(_) => bail!("usage: cargo xtask man [--out DIR]"),
    };
    if args.next().is_some() {
        bail!("usage: cargo xtask man [--out DIR]");
    }
    generate(&out)
}

/// Render every visible CLI command and the daemon, without time or host metadata.
pub fn generate(out: &Path) -> Result<()> {
    std::fs::create_dir_all(out).context("create manual directory")?;
    let mut cli = tidemark_cli::cli::command();
    cli.build();
    let version = cli
        .get_version()
        .context("CLI package version missing")?
        .to_owned();
    // The tree is bounded by the compiled command definitions, not user input.
    let mut pending = vec![(cli, "tidemark".to_owned())];
    while let Some((command, name)) = pending.pop() {
        for child in command.get_subcommands().filter(|c| !c.is_hide_set()) {
            pending.push((child.clone(), format!("{name}-{}", child.get_name())));
        }
        render(command, &name, "1", &version, out, false)?;
    }
    let daemon = tidemarkd::Args::command();
    let version = daemon
        .get_version()
        .context("daemon package version missing")?
        .to_owned();
    render(daemon, "tidemarkd", "8", &version, out, true)
}

fn render(
    command: clap::Command,
    name: &str,
    section: &str,
    version: &str,
    out: &Path,
    daemon: bool,
) -> Result<()> {
    let mut file = std::fs::File::create(out.join(format!("{name}.{section}")))?;
    clap_mangen::Man::new(command)
        .title(name)
        .section(section)
        // roff does not quote empty arguments; retain the empty date position.
        .date("\"\"")
        .source(format!("Tidemark {version}"))
        .render(&mut file)?;
    let mut extra = Roff::default();
    extra.control("SH", ["EXIT STATUS"]);
    let statuses: &[(&str, &str)] = if daemon {
        &[
            ("0", "Success."),
            ("1", "Daemon startup or runtime error."),
            (
                "2",
                "Usage error or configuration validation failure with --check-config.",
            ),
        ]
    } else {
        &[
            ("0", "Success."),
            ("1", "Runtime error."),
            ("2", "Usage or configuration error."),
            (
                "3",
                "Completed with errors: degraded or partial scan, or failed queries.",
            ),
            ("4", "Daemon unavailable for live commands."),
            (
                "5",
                "Precondition not met, including deploy stage pre-flight failure.",
            ),
            (
                "6",
                "deploy verify: not healthy yet or timed out; see command output.",
            ),
        ]
    };
    for (code, description) in statuses {
        extra.control("TP", []);
        extra.text([roman(*code)]);
        extra.text([roman(*description)]);
    }
    extra.control("SH", ["FILES"]);
    for (path, description) in [
        (
            "/etc/tidemark/tidemark.toml",
            "Default configuration, 0600 root:root.",
        ),
        (
            "/var/lib/tidemark",
            "Default state directory, 0700 root:root.",
        ),
        (
            "/run/tidemark/api.sock",
            "Unix API socket, 0600 root:root; API requests require uid 0.",
        ),
        (
            "/usr/lib/systemd/system/tidemarkd.service",
            "Packaged systemd service unit.",
        ),
    ] {
        extra.control("TP", []);
        extra.text([roman(path)]);
        extra.text([roman(description)]);
    }
    if daemon {
        extra.control("SH", ["SIGNALS"]);
        extra.control("TP", []);
        extra.text([roman("SIGHUP")]);
        extra.text([roman("Reload configuration. Invalid configuration retains the previous configuration and reports warnings.")]);
        extra.control("TP", []);
        extra.text([roman("SIGTERM, SIGINT")]);
        extra.text([roman("Stop the daemon with bounded shutdown.")]);
    }
    extra.control("SH", ["SEE ALSO"]);
    extra.text([roman(if daemon {
        "tidemark(1), systemctl(1), journalctl(1)"
    } else {
        "tidemarkd(8), du(1), df(1)"
    })]);
    extra.to_writer(&mut file)?;
    file.flush()?;
    Ok(())
}
