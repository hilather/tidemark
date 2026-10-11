//! Owned by: W4-B
//! Human and machine deployment output.

use crate::deploy::{CheckResult, CheckStatus, StagedRecord, VerifyResult};
use std::io::Write;
pub(super) fn checks(checks: &[CheckResult], output: &mut impl Write) -> anyhow::Result<()> {
    for check in checks {
        let status = match check.status {
            CheckStatus::Pass => "PASS",
            CheckStatus::Warn => "WARN",
            CheckStatus::Fail => "FAIL",
        };
        writeln!(output, "{status:4} {:18} {}", check.name, check.detail)?;
    }
    Ok(())
}
pub(super) fn staged(record: &StagedRecord, json: bool) -> anyhow::Result<()> {
    let mut output = std::io::stdout().lock();
    if json {
        serde_json::to_writer(&mut output, record)?;
        writeln!(output)?;
    } else {
        checks(&record.checks, &mut output)?;
        writeln!(
            output,
            "Staged {} (config {}); {}",
            record.version,
            record.config_sha256,
            if record.start_now {
                "immediate start requested"
            } else {
                "waiting for next start"
            }
        )?;
    }
    Ok(())
}
pub(super) fn verified(result: &VerifyResult, json: bool) -> anyhow::Result<()> {
    let mut output = std::io::stdout().lock();
    if json {
        serde_json::to_writer(&mut output, result)?;
        writeln!(output)?;
    } else {
        checks(&result.items, &mut output)?;
        writeln!(
            output,
            "{}",
            if result.healthy {
                "Deployment healthy"
            } else {
                "Deployment not healthy"
            }
        )?;
    }
    Ok(())
}
pub(super) fn preflight_failure(results: &[CheckResult], json: bool) -> anyhow::Result<()> {
    let mut output = std::io::stdout().lock();
    if json {
        serde_json::to_writer(
            &mut output,
            &serde_json::json!({"staged": false, "checks": results}),
        )?;
        writeln!(output)?;
    } else {
        checks(results, &mut output)?;
    }
    Ok(())
}
pub(super) fn unstaged(json: bool) -> anyhow::Result<()> {
    let mut output = std::io::stdout().lock();
    if json {
        writeln!(output, "{{\"unstaged\":true}}")?;
    } else {
        writeln!(
            output,
            "Service stopped and disabled; staging record removed"
        )?;
    }
    Ok(())
}
