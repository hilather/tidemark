//! Owned by: W4-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Config file to install; global -c names the installed path.
    #[arg(long, value_name = "FILE")]
    pub from: Option<std::path::PathBuf>,
    /// Start the service immediately after staging.
    #[arg(long)]
    pub start_now: bool,
    /// Skip named warning-level pre-flight checks.
    #[arg(long, value_name = "NAME", num_args = 1.., action = clap::ArgAction::Append)]
    pub skip_check: Vec<String>,
}

/// Stage the supplied configuration after read-only pre-flight checks.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let env = environment(context)?;
    run_with(args, context, &crate::deploy::RealSystemctl, &env)
}
fn environment(context: &crate::Context) -> std::io::Result<crate::deploy::RealEnv> {
    let installed_config = if context.config_path.is_absolute() {
        context.config_path.clone()
    } else {
        std::env::current_dir()?.join(&context.config_path)
    };
    Ok(crate::deploy::RealEnv {
        installed_config,
        now: tidemark_core::time::unix_now(),
    })
}
fn run_with(
    args: &Args,
    context: &crate::Context,
    sys: &dyn crate::deploy::Systemctl,
    env: &dyn crate::deploy::Env,
) -> anyhow::Result<()> {
    let source = args.from.as_deref().unwrap_or(&context.config_path);
    let options = crate::deploy::StageOptions {
        start_now: args.start_now,
        skip_check: args.skip_check.clone(),
    };
    match crate::deploy::stage(source, &options, sys, env) {
        Ok(record) => super::render::staged(&record, context.json),
        Err(crate::deploy::DeployError::Preflight(checks)) => {
            super::render::preflight_failure(&checks, context.json)?;
            Err(crate::CliError::Precondition(
                "deployment pre-flight failed; nothing changed".into(),
            )
            .into())
        }
        Err(crate::deploy::DeployError::Guard(message)) => {
            Err(crate::CliError::Precondition(message).into())
        }
        Err(error) => Err(error.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        io,
        path::{Path, PathBuf},
    };
    struct FakeEnv(PathBuf);
    impl crate::deploy::Env for FakeEnv {
        fn config_path(&self) -> &Path {
            &self.0
        }
        fn now(&self) -> i64 {
            123
        }
        fn installation(&self, _: &tidemark_core::config::Config) -> io::Result<()> {
            Ok(())
        }
        fn local_directory(&self, _: &Path) -> io::Result<()> {
            Ok(())
        }
        fn mount_point(&self, _: &Path) -> io::Result<()> {
            Ok(())
        }
        fn capacity(&self, _: &Path) -> io::Result<(u64, u64)> {
            Ok((4 << 30, 10 << 30))
        }
        fn fanotify(&self, _: &tidemark_core::config::Config) -> io::Result<()> {
            Ok(())
        }
        fn bulkstat(&self, _: &tidemark_core::config::Config) -> io::Result<()> {
            Ok(())
        }
        fn lvm(&self, _: &tidemark_core::config::Config) -> io::Result<()> {
            Ok(())
        }
    }
    struct FakeSystemctl;
    impl crate::deploy::Systemctl for FakeSystemctl {
        fn is_enabled(&self, _: &str) -> io::Result<bool> {
            Ok(false)
        }
        fn is_active(&self, _: &str) -> io::Result<bool> {
            Ok(false)
        }
        fn enable(&self, _: &str) -> io::Result<()> {
            Ok(())
        }
        fn disable_now(&self, _: &str) -> io::Result<()> {
            Ok(())
        }
        fn restart(&self, _: &str) -> io::Result<()> {
            Ok(())
        }
    }
    #[test]
    fn custom_destination_and_replacement_use_source_with_fake_environment() {
        for invalid in [false, true] {
            let dir = tempfile::tempdir().unwrap();
            let installed = dir.path().join("custom.toml");
            if invalid {
                std::fs::write(&installed, b"invalid TOML").unwrap();
            }
            let source = dir.path().join("source.toml");
            let bytes = format!("[general]\nstate_dir = {:?}\n", dir.path().join("state"));
            std::fs::write(&source, &bytes).unwrap();
            let context = crate::Context {
                config: tidemark_core::config::Config::default(),
                config_path: installed.clone(),
                json: true,
                color: false,
            };
            let env = FakeEnv(environment(&context).unwrap().installed_config);
            run_with(
                &Args {
                    from: Some(source),
                    start_now: false,
                    skip_check: vec![],
                },
                &context,
                &FakeSystemctl,
                &env,
            )
            .unwrap();
            assert_eq!(std::fs::read(&installed).unwrap(), bytes.as_bytes());
        }
    }
}
