//! Owned by: W5-B
//! Command error mapping and frozen argument contracts.
use crate::{
    cmd::{phantom, writers},
    CliError, Context,
};
use clap::Parser;
#[test]
fn daemon_unavailable_exit_four() {
    let ctx = Context {
        config: tidemark_core::config::Config::default(),
        json: true,
        color: false,
    };
    let err = phantom::run(
        &phantom::Args {
            mount: None,
            direct: false,
        },
        &ctx,
    )
    .unwrap_err();
    assert_eq!(err.downcast_ref::<CliError>().unwrap().exit_code(), 4);
    let err = writers::run(
        &writers::Args {
            mount: None,
            direct: false,
            window: None,
            interval: std::time::Duration::from_secs(10),
        },
        &ctx,
    )
    .unwrap_err();
    assert_eq!(err.downcast_ref::<CliError>().unwrap().exit_code(), 4);
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
