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
