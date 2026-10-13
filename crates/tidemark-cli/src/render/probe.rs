//! Owned by: W1-A
//! Probe text sections and schema-wrapped machine output.
use serde::Serialize;
use std::io::Write;
use tidemark_fs::probe::{Fact, ProbeReport};
/// CLI-composed report.
#[derive(Debug, Serialize)]
pub struct Report {
    /// Host, roots and recommendations collected by tidemark-fs.
    #[serde(flatten)]
    pub filesystem: ProbeReport,
}
/// Render all sections to a caller-provided writer.
pub fn write(report: &Report, json: bool, output: &mut impl Write) -> anyhow::Result<()> {
    if json {
        return super::json("tidemark.probe.v1", report, output);
    }
    writeln!(output, "Host")?;
    facts(&report.filesystem.host.facts, output)?;
    for root in &report.filesystem.roots {
        writeln!(output, "\nRoot {}", root.root.display_lossy())?;
        facts(&root.facts, output)?;
        if let Some(leaves) = &root.leaves {
            for dev in leaves {
                writeln!(
                    output,
                    "  ✓ device {} ({}:{}) {}; rotational={}, scheduler={}",
                    dev.name,
                    dev.major,
                    dev.minor,
                    super::enum_label(&dev.kind),
                    dev.rotational
                        .map(|r| r.to_string())
                        .unwrap_or_else(|| "? (queue/rotational unavailable)".into()),
                    dev.scheduler
                        .as_deref()
                        .unwrap_or("? (queue/scheduler unavailable)")
                )?;
            }
        }
        match &root.other_mounts {
            Some(mounts) => writeln!(
                output,
                "  ✓ other mounts: {}",
                serde_json::to_string(mounts)?
            )?,
            None => writeln!(
                output,
                "  ? other mounts: unknown ({})",
                root.facts
                    .get("mount_point")
                    .and_then(|f| f.why.as_deref())
                    .unwrap_or("mount table unavailable")
            )?,
        }
    }
    writeln!(output, "\nRecommendations")?;
    if report.filesystem.recommendations.is_empty() {
        writeln!(output, "  ✓ none from known facts")?;
    }
    for advice in &report.filesystem.recommendations {
        writeln!(output, "  ✗ {}", advice.message)?;
    }
    Ok(())
}
fn facts(
    facts: &std::collections::BTreeMap<String, Fact>,
    output: &mut impl Write,
) -> std::io::Result<()> {
    for (key, fact) in facts {
        let marker = match fact.value.as_deref() {
            None => "?",
            Some("false") => "✗",
            _ => "✓",
        };
        write!(
            output,
            "  {marker} {key}: {}",
            fact.value.as_deref().unwrap_or("unknown")
        )?;
        if let Some(why) = &fact.why {
            write!(output, " ({why})")?;
        }
        writeln!(output)?;
    }
    Ok(())
}
#[cfg(test)]
#[path = "../../tests/w1a/render.rs"]
mod tests;
