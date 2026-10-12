//! Owned by: W6-C
//! RPM build command interface.

use anyhow::{bail, Result};
/// Run RPM build, accepting --container; implementation belongs to W6-C.
pub fn run(_root: &std::path::Path, _args: impl Iterator<Item = String>) -> Result<()> {
    bail!("NotYetImplemented: rpm (owner W6-C)")
}
