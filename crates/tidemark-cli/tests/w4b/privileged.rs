//! Owned by: W4-B
//! Dedicated-VM deployment scenarios; compile locally, execute only at the VM gate.

use std::{
    path::{Path, PathBuf},
    sync::Mutex,
    time::Duration,
};
use tidemark_cli::deploy::{self, DeployError, RealEnv, RealSystemctl, StageOptions, Systemctl};

static SERVICE_TEST: Mutex<()> = Mutex::new(());
fn vm_only() {
    assert_eq!(
        std::env::var("TIDEMARK_DEPLOY_TEST_VM").as_deref(),
        Ok("1"),
        "run only in a disposable deployment VM with TIDEMARK_DEPLOY_TEST_VM=1"
    );
}
#[test]
fn deploy_stage_preflight_blocks() {
    vm_only();
    let _lock = SERVICE_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let dir = tempfile::tempdir().unwrap();
    let source = dir.path().join("source.toml");
    let installed = dir.path().join("installed.toml");
    let state = dir.path().join("state");
    std::fs::write(
        &source,
        format!(
            "[general]\nsocket = {:?}\nstate_dir = {:?}\n[[roots]]\npath = {:?}\n",
            dir.path().join("absent-api.sock"),
            state,
            dir.path().join("absent-root")
        ),
    )
    .unwrap();
    std::fs::write(&installed, b"original").unwrap();
    let env = RealEnv {
        installed_config: installed.clone(),
        now: 100,
    };
    let sys = RealSystemctl;
    let before = sys.is_enabled("tidemarkd").unwrap();
    assert!(matches!(
        deploy::stage(&source, &StageOptions::default(), &sys, &env),
        Err(DeployError::Preflight(_))
    ));
    assert_eq!(std::fs::read(&installed).unwrap(), b"original");
    assert!(!state.exists());
    assert_eq!(sys.is_enabled("tidemarkd").unwrap(), before);
}
#[test]
fn deploy_unstage() {
    vm_only();
    let _lock = SERVICE_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let sys = RealSystemctl;
    assert!(
        !sys.is_active("tidemarkd").unwrap(),
        "this scenario needs a stopped daemon in a disposable VM"
    );
    let before = sys.is_enabled("tidemarkd").unwrap();
    let dir = tempfile::tempdir().unwrap();
    let state = dir.path().join("state");
    let source = dir.path().join("source.toml");
    let installed = dir.path().join("installed.toml");
    std::fs::write(&source, format!("[general]\nsocket = {:?}\nstate_dir = {:?}\n[sampler]\nmounts = ['/']\n[watch]\nenabled = false\nmounts = ['/']\n[lvm]\nenabled = false\n[[roots]]\npath = '/'\n", dir.path().join("absent-api.sock"), state)).unwrap();
    let env = RealEnv {
        installed_config: installed,
        now: 100,
    };
    deploy::stage(&source, &StageOptions::default(), &sys, &env).unwrap();
    assert!(sys.is_enabled("tidemarkd").unwrap());
    deploy::unstage(&state, true, &sys).unwrap();
    assert!(!sys.is_active("tidemarkd").unwrap());
    assert!(!sys.is_enabled("tidemarkd").unwrap());
    assert!(!state.exists());
    if before {
        sys.enable("tidemarkd").unwrap();
    }
}
/// The VM harness invokes the same test before and after its ordinary reboot.
#[test]
fn deploy_stage_reboot_verify() {
    let Some(phase) = std::env::var_os("TIDEMARK_DEPLOY_VM_PHASE") else {
        eprintln!(
            "skipped: needs TIDEMARK_DEPLOY_VM_PHASE=stage|verify and TIDEMARK_DEPLOY_VM_CONFIG"
        );
        return;
    };
    vm_only();
    let _lock = SERVICE_TEST.lock().unwrap_or_else(|e| e.into_inner());
    let source = PathBuf::from(
        std::env::var_os("TIDEMARK_DEPLOY_VM_CONFIG")
            .expect("VM harness must supply its configuration file"),
    );
    let cfg = tidemark_core::config::Config::load(&source).unwrap();
    match phase.to_str() {
        Some("stage") => {
            let env = RealEnv {
                installed_config: Path::new("/etc/tidemark/tidemark.toml").into(),
                now: tidemark_core::time::unix_now(),
            };
            deploy::stage(&source, &StageOptions::default(), &RealSystemctl, &env).unwrap();
            assert!(RealSystemctl.is_enabled("tidemarkd").unwrap());
        }
        Some("verify") => {
            let api = deploy::SocketPing {
                socket: cfg.general.socket.clone(),
                timeout: Duration::from_secs(10),
            };
            let result = deploy::verify(
                &cfg.general.state_dir,
                Duration::from_secs(300),
                &RealSystemctl,
                &api,
            )
            .unwrap();
            assert!(result.healthy, "{result:?}");
            let report = result.firstboot.unwrap();
            assert!(
                report["booted_at"].as_i64().unwrap()
                    > result.staged.unwrap()["staged_at"].as_i64().unwrap(),
                "harness must reboot between stage and verify"
            );
        }
        _ => panic!("VM harness must set TIDEMARK_DEPLOY_VM_PHASE=stage or verify"),
    }
}
