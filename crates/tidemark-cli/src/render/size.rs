//! Owned by: W0-B2
//! Shared binary-size and raw-path formatting.

/// Format binary units using the frozen core contract.
pub fn bytes(value: u64) -> String {
    tidemark_core::units::fmt_bytes(value)
}
/// Display raw path bytes with core escaping.
pub fn path(value: &tidemark_core::path::RawPath) -> String {
    value.display_lossy()
}

/// Format injected Unix seconds as a UTC civil timestamp.
// Civil date arithmetic in 400-year Gregorian eras, with Euclidean division
// so dates before the Unix epoch have the same behavior as later dates.
pub fn timestamp(ts: i64) -> String {
    let z = ts.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let year = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = mp + if mp < 10 { 3 } else { -9 };
    let year = year + i64::from(month <= 2);
    let second = ts.rem_euclid(86_400);
    format!(
        "{year:04}-{month:02}-{day:02} {:02}:{:02}:{:02}Z",
        second / 3600,
        (second / 60) % 60,
        second % 60
    )
}
