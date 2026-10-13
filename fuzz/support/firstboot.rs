//! Owned by: W7-C3-fuzz
//! Public firstboot reader and report decoding seam.
// Fixture setup failures should fail the test or fuzz execution immediately.
#![allow(clippy::unwrap_used)]
use std::fs;
use tidemarkd::firstboot::needs_report;
/// Feed adversarial bytes through the public parser seam in disposable state.
pub fn exercise(bytes: &[u8]) {
    let dir = tempfile::tempdir().unwrap();
    fs::create_dir(dir.path().join("deploy")).unwrap();
    fs::write(dir.path().join("deploy/staged.json"), bytes).unwrap();
    fs::write(dir.path().join("deploy/firstboot.json"), bytes).unwrap();
    let decision = needs_report(dir.path(), "abc");
    let report: Result<tidemarkd::firstboot::FirstbootReport, _> = serde_json::from_slice(bytes);
    if bytes.len() > 1024 * 1024 {
        assert!(!decision, "oversized staged record must be refused");
    } else {
        assert_eq!(
            decision,
            report.map_or(true, |report| report.config_sha256 != "abc")
        );
    }
}
