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
    let roots = args
        .root
        .iter()
        .map(|p| {
            // Keep a missing absolute root in the report so its unknown reason is visible.
            let absolute = if p.is_absolute() {
                p.clone()
            } else {
                std::env::current_dir()?.join(p)
            };
            tidemark_core::path::RawPath::from_path(&absolute).map_err(anyhow::Error::from)
        })
        .collect::<anyhow::Result<Vec<_>>>()?;
    let filesystem = tidemark_fs::probe::probe(&context.config, &roots);
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
