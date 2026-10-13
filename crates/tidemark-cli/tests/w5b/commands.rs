//! Owned by: W5-B
//! Command error mapping and frozen argument contracts.
use crate::{
    cmd::{phantom, writers},
    CliError, Context,
};
use clap::Parser;
#[test]
fn missing_daemon_takes_direct_setup_without_scanning_proc() {
    let dir = ::test_support::private_tempdir().unwrap();
    let mut ctx = Context {
        config: tidemark_core::config::Config::default(),
        config_path: dir.path().join("config.toml"),
        json: true,
        color: false,
    };
    ctx.config.general.socket = dir.path().join("absent.sock");
    // Missing explicit mount fails in direct setup, before ProcScanner::scan_once.
    let mount = Some(dir.path().join("missing-mount"));
    for direct in [false, true] {
        let phantom_error = phantom::run(
            &phantom::Args {
                mount: mount.clone(),
                direct,
            },
            &ctx,
        )
        .unwrap_err();
        let writers_error = writers::run(
            &writers::Args {
                mount: mount.clone(),
                direct,
                window: None,
                interval: std::time::Duration::from_millis(1),
            },
            &ctx,
        )
        .unwrap_err();
        for error in [phantom_error, writers_error] {
            assert!(
                !matches!(
                    error.downcast_ref::<CliError>(),
                    Some(CliError::Unavailable(_))
                ),
                "{error}"
            );
            assert!(!error.to_string().contains("daemon unavailable"), "{error}");
        }
    }
}
#[test]
fn duration_and_interval_validation() {
    assert!(crate::cli::Cli::try_parse_from(["tidemark", "writers", "--window", "0s"]).is_err());
    assert!(crate::cli::Cli::try_parse_from(["tidemark", "writers", "--interval", "1s"]).is_err());
    assert!(crate::cli::Cli::try_parse_from([
        "tidemark",
        "writers",
        "--direct",
        "--interval",
        "1ms",
        "--window",
        "2h"
    ])
    .is_ok());
    let ctx = Context {
        config: tidemark_core::config::Config::default(),
        config_path: "/etc/tidemark/tidemark.toml".into(),
        json: false,
        color: false,
    };
    let err = writers::run(
        &writers::Args {
            mount: None,
            direct: true,
            window: None,
            interval: std::time::Duration::ZERO,
        },
        &ctx,
    )
    .unwrap_err();
    assert_eq!(err.downcast_ref::<CliError>().unwrap().exit_code(), 2);
}

#[test]
fn shared_setup_selects_visible_overmount() {
    let table = tidemark_fs::mount::parse_mountinfo(
        b"1 0 8:1 / / rw - xfs /dev/a rw\n2 1 8:2 / / rw - xfs /dev/b rw\n",
    )
    .unwrap();
    let root = tidemark_core::path::RawPath::new(b"/".to_vec()).unwrap();
    assert_eq!(
        super::visible_mount(&table, &root).unwrap().dev,
        table[1].dev
    );
    let child = tidemark_core::path::RawPath::new(b"/child".to_vec()).unwrap();
    assert!(super::visible_mount(&table, &child).is_none());
}

#[test]
fn shared_setup_canonicalizes_alias_relative_and_symlink_parent() {
    let temp = ::test_support::private_tempdir().unwrap();
    let nested = temp.path().join("nested");
    std::fs::create_dir(&nested).unwrap();
    let alias = temp.path().join("alias");
    std::os::unix::fs::symlink(&nested, &alias).unwrap();
    assert_eq!(
        super::canonical_mount(&alias).unwrap(),
        super::canonical_mount(&nested).unwrap()
    );
    assert_eq!(
        super::canonical_mount(&alias.join("..")).unwrap(),
        super::canonical_mount(temp.path()).unwrap()
    );
    assert_eq!(
        super::canonical_mount(std::path::Path::new(".")).unwrap(),
        super::canonical_mount(&std::env::current_dir().unwrap()).unwrap()
    );
    assert!(super::canonical_mount(&temp.path().join("missing")).is_err());
}
