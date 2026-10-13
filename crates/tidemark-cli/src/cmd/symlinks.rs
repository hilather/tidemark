//! Owned by: W3-B
//! Command surface and shared CLI support.

/// Final arguments for this command.
#[derive(Debug, clap::Args)]
pub struct Args {
    /// Directory path to query (canonicalised before lookup).
    #[arg(value_name = "PATH")]
    pub path: Option<std::path::PathBuf>,
    /// Check targets only inside the selected mount; other-filesystem links are not checked beyond its boundary.
    #[arg(long)]
    pub check_targets: bool,
    /// List only dangling symlinks.
    #[arg(long)]
    pub dangling: bool,
    /// List only links resolving outside PATH.
    #[arg(long)]
    pub escaping: bool,
    /// Maximum number of results.
    #[arg(long, value_name = "N")]
    pub limit: Option<usize>,
}

/// Analyze symlinks live under the canonical query directory.
pub fn run(args: &Args, context: &crate::Context) -> anyhow::Result<()> {
    use tidemark_fs::symlinks::SymlinkOpts;
    if args.dangling && !args.check_targets {
        return Err(
            crate::CliError::Configuration("--dangling requires --check-targets".into()).into(),
        );
    }
    let root = tidemark_fs::canon::canonicalize(
        args.path
            .as_deref()
            .unwrap_or_else(|| std::path::Path::new("/data")),
    )?;
    let limit = args.limit.unwrap_or(25);
    if limit > 1_000_000 {
        return Err(crate::CliError::Configuration("limit exceeds 1,000,000".into()).into());
    }
    let scan_gate = super::links::gentle_gate(&root, context)?;
    let gate = scan_gate.gate();
    let cancel = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let sig = signal_hook::flag::register(signal_hook::consts::SIGINT, cancel.clone())?;
    let result = tidemark_fs::symlinks::analyze_matching(
        &root,
        gate.as_ref(),
        &cancel,
        &SymlinkOpts {
            check_targets: args.check_targets,
            limit,
        },
        |path, target, class| matches_example(&root, path, target, class, args),
    );
    signal_hook::low_level::unregister(sig);
    scan_gate.stop();
    let report = result?;
    crate::render::symlinks::report(&root, &report, context.json, &mut std::io::stdout().lock())
}

fn matches_example(
    root: &tidemark_core::path::RawPath,
    path: &tidemark_core::path::RawPath,
    target: &[u8],
    class: tidemark_fs::symlinks::SymlinkClass,
    args: &Args,
) -> bool {
    use tidemark_fs::symlinks::SymlinkClass;
    if args.dangling && class != SymlinkClass::Dangling {
        return false;
    }
    if !args.escaping {
        return true;
    }
    if class == SymlinkClass::OtherFs {
        return true;
    }
    if target.is_empty() {
        return false;
    }
    let mut bytes = if target.starts_with(b"/") {
        Vec::new()
    } else {
        path.parent()
            .map(|p| p.as_bytes().to_vec())
            .unwrap_or_default()
    };
    if !target.starts_with(b"/") {
        bytes.push(b'/');
    }
    bytes.extend_from_slice(target);
    tidemark_core::path::RawPath::new(bytes).is_ok_and(|p| !p.starts_with(root))
}
