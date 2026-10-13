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
        writeln!(
            output,
            "{status:4} {:18} {}",
            crate::render::table::diagnostic(check.name),
            crate::render::table::diagnostic(&check.detail)
        )?;
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
            crate::render::table::diagnostic(&record.version),
            crate::render::table::diagnostic(&record.config_sha256),
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

#[cfg(test)]
mod tests {
    #[test]
    fn hostile_path_in_check_is_one_escaped_diagnostic_line() {
        let path = "/tmp/admin\u{1b}[31m\nforged PASS";
        let check = crate::deploy::CheckResult {
            name: "destination",
            status: crate::deploy::CheckStatus::Fail,
            detail: format!("cannot use {path}: not eligible"),
        };
        let mut out = Vec::new();
        super::checks(std::slice::from_ref(&check), &mut out).unwrap();
        let text = String::from_utf8(out).unwrap();
        assert_eq!(text.lines().count(), 1);
        assert!(
            text.contains("/tmp/admin\\x1B[31m\\x0Aforged PASS"),
            "{text}"
        );
        assert!(!text.contains('\u{1b}'));
        assert_eq!(
            serde_json::to_value(check).unwrap()["detail"],
            format!("cannot use {path}: not eligible")
        );
    }
}
