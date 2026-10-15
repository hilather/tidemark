//! Owned by: W0-B2
//! Shared output helpers.

/// Binary sizes and raw paths.
pub mod size;
/// Aligned text tables and tree markers.
pub mod table;

/// Write a machine document with its command-specific schema.
pub fn json<T: serde::Serialize>(
    command_schema: &'static str,
    data: T,
    output: &mut impl std::io::Write,
) -> anyhow::Result<()> {
    serde_json::to_writer(
        &mut *output,
        &tidemark_core::json::JsonDoc {
            schema: command_schema.into(),
            data,
        },
    )?;
    output.write_all(b"\n")?;
    Ok(())
}
/// Host and filesystem capability probe output.
pub mod probe;

/// Scan progress rendering.
pub mod scan_progress;
/// Tree rendering.
pub mod tree;

/// diff report rendering.
pub mod diff;

/// links report rendering.
pub mod links;

/// symlinks report rendering.
pub mod symlinks;

/// export report rendering.
pub mod export;

/// wide report rendering.
pub mod wide;

/// lvm report rendering.
pub mod lvm;

/// deploy report rendering.
pub mod deploy;

/// Live now output.
pub mod now;

/// Live hot output.
pub mod hot;

/// Live alerts output.
pub mod alerts;

/// Live status output.
pub mod status;

/// Lower-case human label for an enumeration; machine serialization is untouched.
pub fn enum_label(value: impl std::fmt::Debug) -> String {
    format!("{value:?}").to_ascii_lowercase()
}

/// Select the unit noun for a displayed count; zero uses the plural.
pub fn unit(count: u64, singular: &'static str, plural: &'static str) -> &'static str {
    if count == 1 {
        singular
    } else {
        plural
    }
}
