//! Owned by: WTEST-e2e-deploy
//! One fake-host scenario table exercising deployment orchestration and durable effects.

use super::*;
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::{fs::MetadataExt, net::UnixStream},
    thread,
};

#[derive(Clone, Copy, Debug)]
enum Command {
    Stage,
    Unstage(bool),
    Verify(Socket),
}
#[derive(Clone, Copy, Debug)]
enum Socket {
    Healthy,
    Unhealthy,
    Absent,
}
#[derive(Clone, Copy, Debug)]
enum Setup {
    Previous,
    Invalid,
    Malformed,
    Mode(u32),
    ForeignOwner,
    NonLocal,
    MissingState(&'static str),
    StateFilesystem(&'static str),
    NonLocalData,
    Full,
    MissingArtifact(usize),
    ProbeFailure(&'static str),
    DestinationSymlink(bool),
    Staged,
    Disabled,
    PublicationFailure,
    EnableFailure,
    Data,
}
#[derive(Clone, Copy, Debug)]
enum Effects {
    Unchanged,
    Installed,
    Repaired,
    Compensated,
    Retained,
    Purged,
}
struct Row {
    name: &'static str,
    command: Command,
    setup: Setup,
    code: u8,
    calls: &'static [&'static str],
    effects: Effects,
}

// 08 §1.3/§5: success 0, runtime/compensation failure 1, blocking
// pre-flight/unsafe unstage 5, verify unhealthy (including absent API) 6.
const ROWS: &[Row] = &[
    Row {
        name: "existing NFS state",
        command: Command::Stage,
        setup: Setup::StateFilesystem("nfs"),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "missing state local",
        command: Command::Stage,
        setup: Setup::MissingState("ext4"),
        code: 0,
        calls: &["enable"],
        effects: Effects::Installed,
    },
    Row {
        name: "missing state NFS ancestor",
        command: Command::Stage,
        setup: Setup::MissingState("nfs"),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "unknown state filesystem",
        command: Command::Stage,
        setup: Setup::StateFilesystem("unknown-test-fs"),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "unstage NFS state",
        command: Command::Unstage(false),
        setup: Setup::NonLocalData,
        code: 0,
        calls: &["disable_now"],
        effects: Effects::Retained,
    },
    Row {
        name: "purge NFS state",
        command: Command::Unstage(true),
        setup: Setup::NonLocalData,
        code: 0,
        calls: &["disable_now"],
        effects: Effects::Purged,
    },
    Row {
        name: "valid backup",
        command: Command::Stage,
        setup: Setup::Previous,
        code: 0,
        calls: &["enable"],
        effects: Effects::Installed,
    },
    Row {
        name: "validation failure",
        command: Command::Stage,
        setup: Setup::Invalid,
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "malformed config",
        command: Command::Stage,
        setup: Setup::Malformed,
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "state 0777",
        command: Command::Stage,
        setup: Setup::Mode(0o777),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "state 0750",
        command: Command::Stage,
        setup: Setup::Mode(0o750),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "foreign owner",
        command: Command::Stage,
        setup: Setup::ForeignOwner,
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "full state filesystem",
        command: Command::Stage,
        setup: Setup::Full,
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "missing CLI",
        command: Command::Stage,
        setup: Setup::MissingArtifact(0),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "missing daemon",
        command: Command::Stage,
        setup: Setup::MissingArtifact(1),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "missing unit",
        command: Command::Stage,
        setup: Setup::MissingArtifact(2),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "missing service manager",
        command: Command::Stage,
        setup: Setup::MissingArtifact(3),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "installation failure",
        command: Command::Stage,
        setup: Setup::ProbeFailure("installation"),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "root failure",
        command: Command::Stage,
        setup: Setup::ProbeFailure("roots"),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "mount failure",
        command: Command::Stage,
        setup: Setup::ProbeFailure("mounts"),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "capacity failure",
        command: Command::Stage,
        setup: Setup::ProbeFailure("state_space"),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "destination symlink",
        command: Command::Stage,
        setup: Setup::DestinationSymlink(false),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "identical destination symlink",
        command: Command::Stage,
        setup: Setup::DestinationSymlink(true),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "stage twice",
        command: Command::Stage,
        setup: Setup::Staged,
        code: 0,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "repair enablement",
        command: Command::Stage,
        setup: Setup::Disabled,
        code: 0,
        calls: &["enable"],
        effects: Effects::Repaired,
    },
    Row {
        name: "publication compensation",
        command: Command::Stage,
        setup: Setup::PublicationFailure,
        code: 1,
        calls: &["enable", "disable_now"],
        effects: Effects::Compensated,
    },
    Row {
        name: "enable compensation",
        command: Command::Stage,
        setup: Setup::EnableFailure,
        code: 1,
        calls: &["enable", "disable_now"],
        effects: Effects::Compensated,
    },
    Row {
        name: "unstage keep",
        command: Command::Unstage(false),
        setup: Setup::Data,
        code: 0,
        calls: &["disable_now"],
        effects: Effects::Retained,
    },
    Row {
        name: "unstage purge",
        command: Command::Unstage(true),
        setup: Setup::Data,
        code: 0,
        calls: &["disable_now"],
        effects: Effects::Purged,
    },
    Row {
        name: "unstage 0777",
        command: Command::Unstage(false),
        setup: Setup::Mode(0o777),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "purge 0777",
        command: Command::Unstage(true),
        setup: Setup::Mode(0o777),
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "verify healthy socket",
        command: Command::Verify(Socket::Healthy),
        setup: Setup::Staged,
        code: 0,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "verify unhealthy socket",
        command: Command::Verify(Socket::Unhealthy),
        setup: Setup::Staged,
        code: 6,
        calls: &[],
        effects: Effects::Unchanged,
    },
    Row {
        name: "verify absent socket",
        command: Command::Verify(Socket::Absent),
        setup: Setup::Staged,
        code: 6,
        calls: &[],
        effects: Effects::Unchanged,
    },
];

// Hash the entire bounded fixture tree without following symlinks. Include names,
// types, ownership, modes, lengths and bytes/targets; omit volatile access/change times
// (reads themselves change atime). Name sorting and length framing avoid ambiguity.
fn tree_hash(root: &Path) -> Vec<u8> {
    fn field(hash: &mut Sha256, bytes: &[u8]) {
        hash.update((bytes.len() as u64).to_le_bytes());
        hash.update(bytes);
    }
    let mut hash = Sha256::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(path) = pending.pop() {
        let meta = fs::symlink_metadata(&path).unwrap();
        field(
            &mut hash,
            path.strip_prefix(root)
                .unwrap()
                .as_os_str()
                .as_encoded_bytes(),
        );
        hash.update(meta.mode().to_le_bytes());
        hash.update(meta.uid().to_le_bytes());
        hash.update(meta.gid().to_le_bytes());
        hash.update(meta.mtime().to_le_bytes());
        hash.update(meta.mtime_nsec().to_le_bytes());
        if meta.file_type().is_symlink() {
            field(
                &mut hash,
                fs::read_link(&path).unwrap().as_os_str().as_encoded_bytes(),
            );
        } else if meta.is_dir() {
            let mut entries: Vec<_> = fs::read_dir(&path)
                .unwrap()
                .map(|e| e.unwrap().path())
                .collect();
            entries.sort_by(|a, b| {
                a.as_os_str()
                    .as_encoded_bytes()
                    .cmp(b.as_os_str().as_encoded_bytes())
            });
            pending.extend(entries.into_iter().rev());
        } else if meta.is_file() {
            field(&mut hash, &fs::read(&path).unwrap());
        } else {
            // Unix sockets have no readable contents.
            assert!(std::os::unix::fs::FileTypeExt::is_socket(&meta.file_type()));
        }
    }
    hash.finalize().to_vec()
}

// Keep command-level mappings explicit, matching cmd/deploy adapters. The CLI
// subprocess tests remain because these engine tests cannot cover parsing/rendering.
fn command_code<T>(result: Result<T, DeployError>) -> u8 {
    match result {
        Ok(_) => 0,
        Err(DeployError::Preflight(_) | DeployError::Guard(_)) => {
            crate::CliError::Precondition(String::new()).exit_code()
        }
        Err(error) => crate::command_error(error.into()).exit_code(),
    }
}

// Connected sockets use a real socket pair so a fake host does not need to bind
// a filesystem listener. The injected ApiPing boundary speaks the frozen status
// protocol; an absent socket exercises the production SocketPing adapter.
struct FakeSocket {
    stream: RefCell<Option<UnixStream>>,
    absent: PathBuf,
}
impl ApiPing for FakeSocket {
    fn ping(&self) -> io::Result<()> {
        let mut slot = self.stream.borrow_mut();
        let Some(stream) = slot.as_mut() else {
            return SocketPing {
                socket: self.absent.clone(),
                timeout: Duration::from_secs(10),
            }
            .ping();
        };
        stream.set_read_timeout(Some(Duration::from_secs(10)))?;
        stream.set_write_timeout(Some(Duration::from_secs(10)))?;
        writeln!(
            stream,
            "{}",
            serde_json::json!({"v":1,"id":"verify","method":"status","params":{}})
        )?;
        let mut line = String::new();
        // Bound the response even if the fake server is changed accidentally.
        std::io::Read::take(BufReader::new(stream.try_clone()?), MAX_RECORD_BYTES)
            .read_line(&mut line)?;
        let response: tidemark_core::api::Response =
            serde_json::from_str(&line).map_err(io::Error::other)?;
        if response.v != 1 || response.id != "verify" || !response.ok {
            return Err(io::Error::other("fake status API unhealthy"));
        }
        let _: tidemark_core::api::StatusResult = serde_json::from_value(
            response
                .result
                .ok_or_else(|| io::Error::other("missing status result"))?,
        )
        .map_err(io::Error::other)?;
        Ok(())
    }
}

fn run_row(row: &Row) {
    let mut f = Fixture::new();
    let _owner = OwnerProbe;
    // Explicit fake root/mount inputs ensure those probes really run.
    let root = f._dir.path().join("tracked");
    fs::create_dir(&root).unwrap();
    fs::write(&f.source, format!("[general]\nstate_dir = {:?}\n[[roots]]\npath = {:?}\n[sampler]\nmounts = [{:?}]\n[watch]\nmounts = [{:?}]\n", f.state, root, root, root)).unwrap();
    fs::create_dir_all(f.env.installed.parent().unwrap()).unwrap();
    fs::write(&f.env.installed, b"original").unwrap();
    match row.setup {
        Setup::Previous => {}
        Setup::Invalid => fs::write(&f.source, "[general]\nstate_dir = 'relative'\n").unwrap(),
        Setup::Malformed => fs::write(&f.source, "[general]\nunknown = true\n").unwrap(),
        Setup::Mode(mode) => {
            fs::create_dir(&f.state).unwrap();
            fs::set_permissions(&f.state, fs::Permissions::from_mode(mode)).unwrap();
            fs::write(f.state.join("keep"), b"private data").unwrap();
        }
        Setup::ForeignOwner => {
            fs::create_dir(&f.state).unwrap();
            fs::set_permissions(&f.state, fs::Permissions::from_mode(0o700)).unwrap();
            FOREIGN_OWNER.with(|owner| *owner.borrow_mut() = Some(f.state.clone()));
        }
        Setup::NonLocal => {
            f.env.filesystem = "nfs";
            f.env.filesystem_path = Some(f.state.clone());
        }
        Setup::MissingState(kind) => {
            // Two absent components ensure the probe uses the longest existing ancestor.
            f.state = f._dir.path().join("missing/state");
            fs::write(
                &f.source,
                format!(
                    "[general]\nstate_dir = {:?}\n[[roots]]\npath = {:?}\n",
                    f.state, root
                ),
            )
            .unwrap();
            f.env.filesystem = kind;
            f.env.filesystem_path = Some(f._dir.path().to_path_buf());
        }
        Setup::StateFilesystem(kind) => {
            fs::create_dir(&f.state).unwrap();
            fs::set_permissions(&f.state, fs::Permissions::from_mode(0o700)).unwrap();
            f.env.filesystem = kind;
            f.env.filesystem_path = Some(f.state.clone());
        }
        Setup::Full => f.env.capacity = (0, 10 << 30),
        Setup::MissingArtifact(index) => fs::remove_file(&f.env.required[index]).unwrap(),
        Setup::ProbeFailure(name) => f.env.failures = vec![name],
        Setup::DestinationSymlink(identical) => {
            f.stage();
            let outside = f._dir.path().join("outside.toml");
            fs::write(
                &outside,
                if identical {
                    fs::read(&f.source).unwrap()
                } else {
                    b"outside".to_vec()
                },
            )
            .unwrap();
            fs::remove_file(&f.env.installed).unwrap();
            symlink(outside, &f.env.installed).unwrap();
        }
        Setup::PublicationFailure => f.env.fail_record.set(true),
        Setup::EnableFailure => f.sys.fail_enable.set(true),
        Setup::Staged | Setup::Disabled | Setup::Data | Setup::NonLocalData => {
            let record = f.stage();
            if matches!(row.command, Command::Verify(_)) {
                f.report(&record.config_sha256, true);
                f.sys.active.set(true);
            }
            if matches!(row.setup, Setup::Disabled) {
                f.sys.enabled.set(false);
            }
            if matches!(row.setup, Setup::Data | Setup::NonLocalData) {
                fs::write(f.state.join("keep.db"), b"own data").unwrap();
                fs::create_dir(f.state.join("nested")).unwrap();
                fs::write(f.state.join("nested/file"), b"own data").unwrap();
                let outside = f._dir.path().join("outside");
                fs::create_dir(&outside).unwrap();
                fs::write(outside.join("keep"), b"untouched").unwrap();
                symlink(outside, f.state.join("external-link")).unwrap();
                f.sys.active.set(true);
            }
        }
    }
    if matches!(row.setup, Setup::NonLocalData) {
        f.env.filesystem = "nfs";
        f.env.filesystem_path = Some(f.state.clone());
    }
    f.sys.calls.borrow_mut().clear();
    f.sys.queries.set(0);
    let socket = f._dir.path().join("api.sock");
    let pair = match row.command {
        Command::Verify(Socket::Healthy | Socket::Unhealthy) => {
            Some(UnixStream::pair().expect("fake socket pair is a required precondition"))
        }
        _ => None,
    };
    let before = tree_hash(f._dir.path());
    let (api, server) = if let Some((client, mut stream)) = pair {
        let healthy = matches!(row.command, Command::Verify(Socket::Healthy));
        let api = FakeSocket {
            stream: RefCell::new(Some(client)),
            absent: socket.clone(),
        };
        let server = thread::spawn(move || {
            stream
                .set_read_timeout(Some(Duration::from_secs(10)))
                .unwrap();
            let mut line = String::new();
            BufReader::new(stream.try_clone().unwrap())
                .read_line(&mut line)
                .unwrap();
            let request: serde_json::Value = serde_json::from_str(&line).unwrap();
            assert_eq!(request["method"], "status");
            let response = if healthy {
                serde_json::json!({"v":1,"id":request["id"],"ok":true,"result":{"version":"fake","uptime_s":120,"components":[],"watch_modes":[],"last_errors":[]}})
            } else {
                serde_json::json!({"v":1,"id":request["id"],"ok":false,"error":{"code":"unavailable","message":"injected unhealthy API"}})
            };
            writeln!(stream, "{response}").unwrap();
        });
        (api, Some(server))
    } else {
        (
            FakeSocket {
                stream: RefCell::new(None),
                absent: socket,
            },
            None,
        )
    };
    let code = match row.command {
        Command::Stage => {
            let result = stage(&f.source, &StageOptions::default(), &f.sys, &f.env);
            if matches!(row.effects, Effects::Compensated) {
                let message = result.as_ref().unwrap_err().to_string();
                assert!(message.contains("injected"));
                assert!(message.contains("re-run deploy stage or deploy unstage"));
            }
            if matches!(row.setup, Setup::Staged) {
                let record = result.as_ref().unwrap();
                assert_eq!(record.staged_at, 100);
                assert_eq!(
                    record.config_sha256,
                    format!("{:x}", Sha256::digest(fs::read(&f.source).unwrap()))
                );
            }
            if matches!(
                row.setup,
                Setup::NonLocal | Setup::MissingState("nfs") | Setup::StateFilesystem(_)
            ) {
                let DeployError::Preflight(checks) = result.as_ref().unwrap_err() else {
                    panic!("expected filesystem pre-flight refusal");
                };
                let check = checks
                    .iter()
                    .find(|check| check.name == "state_filesystem")
                    .unwrap();
                assert_eq!(check.status, CheckStatus::Fail);
                assert!(check.detail.contains(&f.state.display().to_string()));
                assert!(check.detail.contains(f.env.filesystem));
                assert_eq!(f.env.mutations.get(), 0);
            }
            command_code(result)
        }
        Command::Unstage(purge) => command_code(unstage(&f.state, purge, &f.sys)),
        Command::Verify(_) => {
            let result = verify(&f.state, Duration::ZERO, &f.sys, &api).unwrap();
            if result.healthy {
                0
            } else {
                crate::CliError::Unhealthy(String::new()).exit_code()
            }
        }
    };
    if let Some(server) = server {
        server.join().unwrap();
    }
    assert_eq!(code, row.code, "{}", row.name);
    assert_eq!(*f.sys.calls.borrow(), row.calls, "{}", row.name);
    let queries = if row.code == 5 {
        0
    } else {
        match row.command {
            Command::Stage if matches!(row.effects, Effects::Compensated) => 4,
            Command::Stage => 2,
            Command::Unstage(true) => 3,
            Command::Unstage(false) | Command::Verify(_) => 1,
        }
    };
    assert_eq!(f.sys.queries.get(), queries, "{} service queries", row.name);
    match row.effects {
        Effects::Unchanged => {
            assert_eq!(
                tree_hash(f._dir.path()),
                before,
                "{} changed the fake host",
                row.name
            );
            if row.code == 5 {
                assert_eq!(
                    f.sys.queries.get(),
                    0,
                    "{} queried services before refusal",
                    row.name
                );
            }
        }
        Effects::Installed | Effects::Repaired => {
            let bytes = fs::read(&f.source).unwrap();
            assert_eq!(fs::read(&f.env.installed).unwrap(), bytes);
            assert!(f.sys.enabled.get());
            assert!(!f.sys.active.get());
            for path in [
                &f.env.installed,
                &f.state.join("deploy/staged.json"),
                &f.state.join(MARKER),
                &f.env.installed.with_file_name("tidemark.toml.100.bak"),
            ] {
                assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o600);
            }
            for path in [&f.state, &f.state.join("deploy")] {
                assert_eq!(fs::metadata(path).unwrap().mode() & 0o777, 0o700);
            }
            assert_eq!(
                fs::read(f.env.installed.with_file_name("tidemark.toml.100.bak")).unwrap(),
                b"original"
            );
            let record: StagedRecord =
                serde_json::from_slice(&fs::read(f.state.join("deploy/staged.json")).unwrap())
                    .unwrap();
            assert_eq!(record.config_sha256, format!("{:x}", Sha256::digest(bytes)));
            assert_eq!(record.staged_at, 100);
        }
        Effects::Compensated => {
            assert_eq!(fs::read(&f.env.installed).unwrap(), b"original");
            assert!(!f
                .env
                .installed
                .with_file_name("tidemark.toml.100.bak")
                .exists());
            assert!(!f.state.join(MARKER).exists());
            assert!(!f.state.join("deploy/staged.json").exists());
            assert!(!f.sys.enabled.get());
            assert!(!f.sys.active.get());
            assert!(f.state.join("deploy").is_dir());
            assert_eq!(fs::read_dir(f.state.join("deploy")).unwrap().count(), 0);
            assert_eq!(fs::read_dir(&f.state).unwrap().count(), 1);
            assert_ne!(
                tree_hash(f._dir.path()),
                before,
                "R2-7 allows new empty directories"
            );
        }
        Effects::Retained | Effects::Purged => {
            assert!(!f.sys.enabled.get());
            assert!(!f.sys.active.get());
            assert_eq!(
                fs::read(f._dir.path().join("outside/keep")).unwrap(),
                b"untouched"
            );
            assert_eq!(
                fs::read(&f.env.installed).unwrap(),
                fs::read(&f.source).unwrap()
            );
            if matches!(row.effects, Effects::Purged) {
                assert!(!f.state.exists());
            } else {
                assert_eq!(fs::read(f.state.join("keep.db")).unwrap(), b"own data");
                assert_eq!(fs::read(f.state.join("nested/file")).unwrap(), b"own data");
                assert!(f.state.join(MARKER).exists());
                assert!(!f.state.join("deploy/staged.json").exists());
            }
        }
    }
}

#[test]
fn deployment_scenario_table() {
    for row in ROWS {
        run_row(row);
    }
}

#[test]
fn deployment_scenario_nonlocal_state() {
    // No tracked roots on nfs: only the state filesystem is non-local. The
    // injected classifier would reject it if production queried that path.
    let row = Row {
        name: "nonlocal state",
        command: Command::Stage,
        setup: Setup::NonLocal,
        code: 5,
        calls: &[],
        effects: Effects::Unchanged,
    };
    run_row(&row);
}
