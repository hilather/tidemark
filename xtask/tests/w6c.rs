//! Owned by: W6-C
//! Packaging contracts and guarded privileged install entry point.

#![allow(clippy::unwrap_used)]

#[path = "../src/rpm.rs"]
mod rpm;
use std::{collections::BTreeMap, io::Write, path::Path, process::Command};
fn root() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR")).parent().unwrap()
}
fn read(path: &str) -> String {
    std::fs::read_to_string(root().join(path)).unwrap()
}
fn block(section: &str, lang: &str) -> String {
    read("docs/plan/08-interfaces.md")
        .split(section)
        .nth(1)
        .unwrap()
        .split(&format!("```{lang}\n"))
        .nth(1)
        .unwrap()
        .split("```")
        .next()
        .unwrap()
        .to_owned()
}
fn unit(text: &str) -> BTreeMap<String, String> {
    let mut section = "";
    let mut result = BTreeMap::new();
    for line in text
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
    {
        if line.starts_with('[') {
            section = line;
            continue;
        }
        let (k, v) = line.split_once('=').unwrap();
        assert!(result
            .insert(
                format!("{section}.{k}"),
                v.split('#').next().unwrap().trim().to_owned()
            )
            .is_none());
    }
    result
}
#[test]
fn exact_normative_unit_and_config() {
    let actual = read("packaging/systemd/tidemarkd.service");
    let expected = block("## 6.", "ini");
    assert_eq!(actual, expected, "line-by-line unit comparison");
    assert_eq!(unit(&actual), unit(&expected));
    for key in ["Group=", "SupplementaryGroups=", "SocketGroup="] {
        assert!(!actual.contains(key));
    }
    let config = read("packaging/config/tidemark.toml");
    assert_eq!(config, block("## 3.", "toml"));
    let parsed: toml::Value = toml::from_str(&config).unwrap();
    assert!(parsed["metrics"].get("listen").is_none());
    assert!(parsed["alerts"].get("webhook").is_none());
    assert!(!config.contains("socket_group"));
    assert_eq!(
        read("packaging/tmpfiles.d/tidemark.conf"),
        "d /run/tidemark 0700 root root -\n"
    );
}
#[test]
fn unit_values_have_no_trailing_comments() {
    let actual = read("packaging/systemd/tidemarkd.service");
    for (index, line) in actual.lines().enumerate() {
        let line = line.trim();
        if line.is_empty()
            || line.starts_with(['#', ';'])
            || (line.starts_with('[') && line.ends_with(']'))
        {
            continue;
        }
        let (_, value) = line.split_once('=').unwrap();
        assert!(
            !value.contains(" #") && !value.contains(" ;"),
            "trailing comment on unit line {}: {line}",
            index + 1
        );
    }
    for setting in ["MemoryLimit=2G", "MemoryMax=2G"] {
        assert!(
            actual.lines().any(|line| line == setting),
            "missing {setting}"
        );
    }
}
#[test]
fn metadata_and_scriptlet_contract() {
    let manifest: toml::Value = toml::from_str(&read("crates/tidemark-cli/Cargo.toml")).unwrap();
    assert!(manifest["package"]["metadata"]["generate-rpm"].is_table());
    let m: toml::Value = toml::from_str(&read("packaging/rpm/rpm.toml")).unwrap();
    let assets = m["assets"].as_array().unwrap();
    for asset in assets {
        let source = asset["source"].as_str().unwrap();
        if !source.starts_with("target/") {
            assert!(root().join(source).is_file(), "missing RPM asset: {source}");
        }
    }
    for key in [
        "post_install_script",
        "pre_uninstall_script",
        "post_uninstall_script",
    ] {
        let script = m[key].as_str().unwrap();
        assert!(
            root().join(script).is_file(),
            "missing RPM script: {script}"
        );
    }
    let expected = [
        ("/usr/bin/tidemark", "0755"),
        ("/usr/bin/tidemarkd", "0755"),
        ("/usr/lib/systemd/system/tidemarkd.service", "0644"),
        ("/usr/lib/tmpfiles.d/tidemark.conf", "0644"),
        ("/etc/tidemark/tidemark.toml", "0600"),
        ("/usr/share/doc/tidemark/README.md", "0644"),
        ("/usr/share/doc/tidemark/07-live-monitoring.md", "0644"),
        ("/usr/share/doc/tidemark/08-interfaces.md", "0644"),
        ("/usr/share/doc/tidemark/11-rollout-runbook.md", "0644"),
        ("/usr/share/bash-completion/completions/tidemark", "0644"),
    ];
    assert_eq!(
        assets
            .iter()
            .filter(|a| !a["source"].as_str().unwrap().starts_with("target/man/"))
            .count(),
        expected.len()
    );
    for (path, mode) in expected {
        let a = assets
            .iter()
            .find(|a| a["dest"].as_str() == Some(path))
            .unwrap();
        assert_eq!(a["mode"].as_str(), Some(mode));
        assert_eq!(a["user"].as_str(), Some("root"));
        assert_eq!(a["group"].as_str(), Some("root"));
        assert_eq!(
            a.get("config").and_then(toml::Value::as_str),
            if path.starts_with("/etc/") {
                Some("noreplace")
            } else {
                None
            }
        );
    }
    assert_eq!(m["version"].as_str(), Some("0.1.0"));
    let post = read(m["post_install_script"].as_str().unwrap());
    assert_eq!(post, "#!/bin/sh\nsystemd-tmpfiles --create /usr/lib/tmpfiles.d/tidemark.conf || :\nsystemctl daemon-reload || :\n");
    assert!(!post.contains("enable"));
    assert!(!post.contains("start"));
    assert!(!post.contains("restart"));
    assert_eq!(
        read(m["pre_uninstall_script"].as_str().unwrap()),
        "#!/bin/sh\nif [ \"$1\" -eq 0 ]; then\n    systemctl stop tidemarkd.service || :\n    systemctl disable tidemarkd.service || :\nfi\n"
    );
    assert_eq!(
        read(m["post_uninstall_script"].as_str().unwrap()),
        "#!/bin/sh\nsystemctl daemon-reload || :\nif [ \"$1\" -ge 1 ]; then\n    systemctl try-restart tidemarkd.service || :\nfi\n"
    );
}
#[test]
fn pure_commands_and_invalid_arguments() {
    assert_eq!(
        rpm::generate_rpm_args("0.1.1").unwrap(),
        [
            "generate-rpm",
            "-p",
            "crates/tidemark-cli",
            "--metadata-overwrite",
            "packaging/rpm/rpm.toml",
            "--set-metadata",
            "version = \"0.1.1\"",
            "--payload-compress",
            "gzip"
        ]
    );
    assert!(rpm::generate_rpm_args("0.1.1;touch /bad").is_err());
    let args = rpm::container_args(Path::new("/tmp/a b"), "0.1.0").unwrap();
    assert_eq!(
        args[..10],
        [
            "run",
            "--rm",
            "--pull=never",
            "--volume",
            "/tmp/a b:/src",
            "--workdir",
            "/src",
            rpm::IMAGE,
            "/bin/bash",
            "-lc"
        ]
        .map(std::ffi::OsString::from)
    );
    let script = args[10].to_str().unwrap();
    assert!(
        script.contains("cargo build --offline --locked --release -p tidemark-cli -p tidemarkd")
    );
    assert!(script.contains("tidemark completions bash > target/release/tidemark.bash"));
    assert!(script.contains("cargo run --offline --locked -p xtask -- man; cargo"));
    assert!(!script.contains("curl"));
    assert!(rpm::run(root(), ["--bad".to_owned()].into_iter()).is_err());
}
#[test]
fn shell_syntax_and_vm_guard() {
    for file in [
        "tests/install/install.sh",
        "packaging/rpm/post.sh",
        "packaging/rpm/preun.sh",
        "packaging/rpm/postun.sh",
        "tools/check-rpm.sh",
        "tools/el8-build-env.sh",
        "docs/pilot/capture-baseline.sh",
    ] {
        assert!(Command::new("bash")
            .arg("-n")
            .arg(root().join(file))
            .status()
            .unwrap()
            .success());
        if Command::new("shellcheck")
            .arg("--version")
            .output()
            .is_ok_and(|o| o.status.success())
        {
            // Keep informational/style diagnostics visible without making them a gate.
            let notes = Command::new("shellcheck")
                .arg(root().join(file))
                .output()
                .unwrap();
            let mut stderr = std::io::stderr().lock();
            stderr
                .write_all(
                    format!(
                        "shellcheck notes for {file}:\n{}{}",
                        String::from_utf8_lossy(&notes.stdout),
                        String::from_utf8_lossy(&notes.stderr),
                    )
                    .as_bytes(),
                )
                .unwrap();
            stderr.write_all(b"\n").unwrap();
            drop(stderr);
            let output = Command::new("shellcheck")
                .arg("--severity=warning")
                .arg(root().join(file))
                .output()
                .unwrap();
            assert!(
                output.status.success(),
                "shellcheck failed for {file} ({}):\n{}{}",
                output.status,
                String::from_utf8_lossy(&output.stdout),
                String::from_utf8_lossy(&output.stderr),
            );
        }
    }
    let output = Command::new("bash")
        .arg(root().join("tests/install/install.sh"))
        .env_remove("TIDEMARK_DEPLOY_TEST_VM")
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Requires disposable VM marker"));
}
#[cfg(feature = "privileged")]
#[test]
fn install_vm() {
    if std::env::var_os("TIDEMARK_INSTALL_RPM").is_none() {
        eprintln!("skipped: needs TIDEMARK_INSTALL_RPM");
        return;
    }
    assert_eq!(std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref(), Ok("1"));
    assert!(Command::new("bash")
        .arg(root().join("tests/install/install.sh"))
        .status()
        .unwrap()
        .success());
}

#[test]
fn native_options_and_command_order() {
    let parse = |args: &[&str]| rpm::parse_args(args.iter().map(|s| (*s).to_owned()));
    let defaults = parse(&[]).unwrap();
    assert!(!defaults.native);
    assert_eq!(defaults.version, "0.1.0");
    let options = parse(&[
        "--native",
        "--online",
        "--allow-foreign-host",
        "--version",
        "0.1.1",
    ])
    .unwrap();
    assert!(options.native && options.online && options.allow_foreign_host);
    assert_eq!(options.version, "0.1.1");
    for args in [
        vec!["--native", "--container"],
        vec!["--container", "--native"],
        vec!["--native", "--native"],
        vec!["--online"],
        vec!["--allow-foreign-host"],
        vec!["--native", "--version"],
        vec!["--native", "--version", "2.0.0"],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
    for online in [false, true] {
        let commands = rpm::native_commands("0.1.1", online, false).unwrap();
        assert_eq!(commands.len(), 4);
        assert_eq!(commands[0].program, "cargo");
        assert_eq!(commands[0].args[0], "build");
        assert!(commands[0].args.contains(&"--locked".into()));
        assert!(commands[0].args.contains(&"--release".into()));
        assert!(commands[0].args.contains(&"tidemark-cli".into()));
        assert!(commands[0].args.contains(&"tidemarkd".into()));
        assert_eq!(commands[0].args.contains(&"--offline".into()), !online);
        assert_eq!(commands[1].program, "target/release/tidemark");
        assert_eq!(commands[1].args, ["completions", "bash"]);
        assert_eq!(
            commands[1].stdout.as_deref(),
            Some("target/release/tidemark.bash")
        );
        assert_eq!(commands[2].program, "cargo");
        assert_eq!(commands[2].args.last().map(String::as_str), Some("man"));
        assert_eq!(commands[2].args.contains(&"--offline".into()), !online);
        assert_eq!(commands[3].args, rpm::generate_rpm_args("0.1.1").unwrap());
    }
    assert!(rpm::native_commands("bad", false, false).is_err());
}

#[test]
fn injected_el8_os_release() {
    for id in ["rocky", "rhel", "centos", "almalinux", "ol"] {
        for version in ["8", "8.8", "8.10"] {
            assert!(rpm::is_el8(&format!(
                "ID=\"{id}\"\nVERSION_ID='{version}'\n"
            )));
        }
    }
    for content in [
        "",
        "ID=ubuntu\nVERSION_ID=8\n",
        "ID=rocky\nVERSION_ID=9\n",
        "ID=rocky\nVERSION_ID=80\n",
        "ID_LIKE=rhel\nVERSION_ID=8\n",
    ] {
        assert!(!rpm::is_el8(content), "{content}");
    }
}

#[test]
fn workflow_and_new_shell_syntax() {
    // No YAML parser is a workspace dependency: assert the requested contracts.
    let workflow = read(".github/workflows/ci.yml");
    for required in [
        "el: [\"8.8\", \"8.10\"]",
        "tools/el8-build-env.sh ${{ matrix.el }}",
        "cargo xtask rpm --native --require-clean",
        "TIDEMARK_BUILD_DATE=$(git show -s --format=%cs HEAD)",
        "tidemark --version | grep -F",
        "tidemarkd --version | grep -F",
        "TIDEMARK_BUILD_COMMIT=$commit",
        "tools/check-rpm.sh",
        "permissions:\n  contents: read",
        "fail-fast: false",
    ] {
        assert!(workflow.contains(required), "missing {required}");
    }
    assert!(workflow.find("vault/rocky/8.8").unwrap() < workflow.find("dnf install").unwrap());
    for file in ["tools/check-rpm.sh", "tools/el8-build-env.sh"] {
        assert!(Command::new("sh")
            .arg("-n")
            .arg(root().join(file))
            .status()
            .unwrap()
            .success());
    }
}

#[test]
fn upgrade_fixture_options_and_drift() {
    let parse = |args: &[&str]| rpm::parse_args(args.iter().map(|s| (*s).to_owned()));
    let options = parse(&["--native", "--version", "0.1.1", "--upgrade-fixture"]).unwrap();
    assert!(options.native && options.upgrade_fixture);
    for args in [
        vec!["--upgrade-fixture"],
        vec!["--version", "0.1.1", "--upgrade-fixture"],
        vec!["--native", "--upgrade-fixture"],
        vec!["--native", "--version", "0.1.0", "--upgrade-fixture"],
        vec![
            "--native",
            "--version",
            "0.1.1",
            "--upgrade-fixture",
            "--upgrade-fixture",
        ],
        vec!["--container", "--version", "0.1.1", "--upgrade-fixture"],
    ] {
        assert!(parse(&args).is_err(), "{args:?}");
    }
    assert!(rpm::native_commands("0.1.0", false, true).is_err());
    for online in [false, true] {
        let ordinary = rpm::native_commands("0.1.1", online, false).unwrap();
        let mut fixture = rpm::native_commands("0.1.1", online, true).unwrap();
        assert_eq!(fixture[3].args[4], "tests/install/rpm-upgrade.toml");
        fixture[3].args[4] = "packaging/rpm/rpm.toml".into();
        assert_eq!(fixture, ordinary);
    }
    let metadata = read("packaging/rpm/rpm.toml");
    let source = "source = \"packaging/config/tidemark.toml\"";
    assert_eq!(metadata.matches(source).count(), 1);
    assert_eq!(
        read("tests/install/rpm-upgrade.toml"),
        metadata.replace(
            source,
            "source = \"tests/install/upgrade-config/tidemark.toml\""
        )
    );
    assert_eq!(
        read("tests/install/upgrade-config/tidemark.toml"),
        format!(
            "{}# upgrade fixture 0.1.1\n",
            read("packaging/config/tidemark.toml")
        )
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
            .args(["--exact", "shell_syntax_and_vm_guard", "--test-threads=1"])
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
fn identity_environment_and_clean_flag() {
    assert!(rpm::parse_args(["--require-clean".into()].into_iter()).is_err());
    assert!(
        rpm::parse_args(["--native".into(), "--require-clean".into()].into_iter())
            .unwrap()
            .require_clean
    );
    let commands = rpm::native_commands("0.1.0", false, false).unwrap();
    assert_eq!(
        commands[0].identity_env,
        ["TIDEMARK_BUILD_COMMIT", "TIDEMARK_BUILD_DATE"]
    );
}

#[test]
fn container_receives_host_identity() {
    let args = rpm::container_identity_args(
        root(),
        "0.1.0",
        &[
            ("TIDEMARK_BUILD_COMMIT", "012345abcdef"),
            ("TIDEMARK_BUILD_DATE", "2026-10-13"),
        ],
    )
    .unwrap();
    assert!(args.contains(&"TIDEMARK_BUILD_COMMIT=012345abcdef".into()));
    assert!(args.contains(&"TIDEMARK_BUILD_DATE=2026-10-13".into()));
    assert_eq!(args.iter().filter(|arg| *arg == "--env").count(), 2);
}

#[test]
fn require_clean_rejects_tracked_changes_before_building() {
    let dir = tempfile::tempdir().unwrap();
    let git = |args: &[&str]| {
        let status = Command::new("git")
            .args(args)
            .current_dir(dir.path())
            .status()
            .unwrap();
        assert!(status.success());
    };
    git(&["init", "-q"]);
    std::fs::write(dir.path().join("tracked"), "original").unwrap();
    git(&["add", "tracked"]);
    git(&[
        "-c",
        "user.name=test",
        "-c",
        "user.email=test@invalid",
        "commit",
        "-qm",
        "fixture",
    ]);
    std::fs::write(dir.path().join("tracked"), "changed").unwrap();
    let error = rpm::run(
        dir.path(),
        ["--native".into(), "--require-clean".into()].into_iter(),
    )
    .unwrap_err();
    assert!(error
        .to_string()
        .contains("rejects changes to tracked files"));
}
