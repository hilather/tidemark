//! Owned by: W6-C
//! Packaging contracts and guarded privileged install entry point.

#![allow(clippy::unwrap_used)]

#[path = "../src/rpm.rs"]
mod rpm;
use std::{collections::BTreeMap, path::Path, process::Command};
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
    assert_eq!(parsed["metrics"]["listen"].as_str(), Some(""));
    assert_eq!(parsed["alerts"]["webhook"].as_str(), Some(""));
    assert!(!config.contains("socket_group"));
    assert_eq!(
        read("packaging/tmpfiles.d/tidemark.conf"),
        "d /run/tidemark 0700 root root -\n"
    );
}
#[test]
fn example_config_validates_with_built_daemon() {
    // xtask has no core dependency: invoke the daemon check-only path instead.
    let status = Command::new(env!("CARGO"))
        .args(["build", "--offline", "-p", "tidemarkd"])
        .current_dir(root())
        .status()
        .unwrap();
    assert!(status.success());
    let target = std::env::var_os("CARGO_TARGET_DIR")
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| root().join("target"));
    let output = Command::new(target.join("debug/tidemarkd"))
        .args(["--check-config", "--config"])
        .arg(root().join("packaging/config/tidemark.toml"))
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
#[test]
fn metadata_and_scriptlet_contract() {
    let m: toml::Value = toml::from_str(&read("packaging/rpm/rpm.toml")).unwrap();
    let assets = m["assets"].as_array().unwrap();
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
    assert_eq!(assets.len(), expected.len());
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
    assert_eq!(
        read(m["pre_uninstall_script"].as_str().unwrap()),
        "#!/bin/sh\nif [ \"$1\" -eq 0 ]; then\n    systemctl stop tidemarkd.service || :\n    systemctl disable tidemarkd.service || :\nfi\n"
    );
    assert_eq!(
        read(m["post_uninstall_script"].as_str().unwrap()),
        "#!/bin/sh\nsystemctl daemon-reload || :\n"
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
            assert!(Command::new("shellcheck")
                .arg(root().join(file))
                .status()
                .unwrap()
                .success());
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
    assert_eq!(std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref(), Ok("1"));
    assert!(Command::new("bash")
        .arg(root().join("tests/install/install.sh"))
        .status()
        .unwrap()
        .success());
}
