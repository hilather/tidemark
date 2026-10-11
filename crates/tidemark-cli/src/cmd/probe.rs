//! Owned by: W1-A
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Root paths to probe for capabilities.
    #[arg(long, value_name = "PATH", num_args = 1..)]
    pub root: Vec<std::path::PathBuf>,
}

/// Execute the read-only probe and append PostgreSQL discovery.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    let filesystem = filesystem_report(args, &context.config);
    let mut postgres = std::collections::BTreeMap::new();
    let fact = |value: Option<String>, why: Option<String>| tidemark_fs::probe::Fact { value, why };
    match tidemark_pg::probe_from_config(&context.config.postgres) {
        Ok(pg) => {
            postgres.insert(
                "reachable".into(),
                fact(Some(pg.reachable.to_string()), pg.error),
            );
            postgres.insert(
                "server_version_num".into(),
                fact(pg.server_version_num.map(|v| v.to_string()), None),
            );
            postgres.insert(
                "in_recovery".into(),
                fact(pg.in_recovery.map(|v| v.to_string()), None),
            );
            postgres.insert("data_directory".into(), fact(pg.data_directory, None));
            postgres.insert(
                "missing_privileges".into(),
                fact(Some(pg.missing_privileges.join(", ")), None),
            );
            for f in postgres.values_mut() {
                if f.value.is_none() && f.why.is_none() {
                    f.why = Some("PostgreSQL did not report this fact".into());
                }
            }
        }
        Err(e) => {
            for key in [
                "reachable",
                "server_version_num",
                "in_recovery",
                "data_directory",
                "missing_privileges",
            ] {
                postgres.insert(key.into(), fact(None, Some(e.to_string())));
            }
        }
    }
    crate::render::probe::write(
        &crate::render::probe::Report {
            filesystem,
            postgres,
        },
        context.json,
        &mut std::io::stdout().lock(),
    )
}

fn filesystem_report(
    args: &Args,
    config: &tidemark_core::config::Config,
) -> tidemark_fs::probe::ProbeReport {
    // Keep original paths until the filesystem collector performs realpath.
    let mut config = config.clone();
    config.roots.extend(
        args.root
            .iter()
            .map(|path| tidemark_core::config::RootConfig {
                path: path.clone(),
                ..Default::default()
            }),
    );
    tidemark_fs::probe::probe(&config, &[])
}
#[cfg(test)]
#[path = "../../tests/w1a/paths.rs"]
mod tests;
