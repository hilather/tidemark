//! Owned by: W0-B1
//! Byte sizes and rates with binary display units.

use crate::error::CoreError;
use serde::{Deserialize, Deserializer, Serialize};
use std::{fmt, str::FromStr};

/// A nonnegative byte count; TOML/JSON accept integers or unit strings.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Serialize)]
#[serde(transparent)]
pub struct ByteSize(
    /// Number of bytes.
    pub u64,
);
impl FromStr for ByteSize {
    type Err = CoreError;
    fn from_str(input: &str) -> Result<Self, Self::Err> {
        let input = input.trim();
        let end = input
            .find(|c: char| !c.is_ascii_digit() && c != '.')
            .unwrap_or(input.len());
        let (number, suffix) = input.split_at(end);
        let factor: u64 = match suffix.trim() {
            "" | "B" => 1,
            "KiB" => 1 << 10,
            "MiB" => 1 << 20,
            "GiB" => 1 << 30,
            "TiB" => 1 << 40,
            "PiB" => 1 << 50,
            "EiB" => 1 << 60,
            "kB" | "KB" => 1_000,
            "MB" => 1_000_000,
            "GB" => 1_000_000_000,
            "TB" => 1_000_000_000_000,
            "PB" => 1_000_000_000_000_000,
            _ => return Err(CoreError::Parse(format!("unknown size unit in {input:?}"))),
        };
        // Decimal rational arithmetic preserves integer byte counts above 2^53.
        let mut parts = number.split('.');
        let whole = parts
            .next()
            .unwrap_or("")
            .parse::<u64>()
            .map_err(|_| CoreError::Parse(input.into()))?;
        let fraction = parts.next();
        if parts.next().is_some() {
            return Err(CoreError::Parse(input.into()));
        }
        let mut bytes = whole
            .checked_mul(factor)
            .ok_or_else(|| CoreError::Parse(format!("size overflow: {input}")))?;
        if let Some(fraction) = fraction {
            if fraction.is_empty() || fraction.len() > 18 {
                return Err(CoreError::Parse(input.into()));
            }
            let n = fraction
                .parse::<u64>()
                .map_err(|_| CoreError::Parse(input.into()))?;
            let extra =
                (u128::from(n) * u128::from(factor) / 10u128.pow(fraction.len() as u32)) as u64;
            bytes = bytes
                .checked_add(extra)
                .ok_or_else(|| CoreError::Parse(format!("size overflow: {input}")))?;
        }
        Ok(Self(bytes))
    }
}
impl<'de> Deserialize<'de> for ByteSize {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            Number(u64),
            Text(String),
        }
        match Repr::deserialize(d)? {
            Repr::Number(n) => Ok(Self(n)),
            Repr::Text(s) => s.parse().map_err(serde::de::Error::custom),
        }
    }
}
impl fmt::Display for ByteSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&fmt_bytes(self.0))
    }
}
fn significant(value: f64) -> String {
    let digits = if value >= 100.0 {
        0
    } else if value >= 10.0 {
        1
    } else {
        2
    };
    format!("{value:.digits$}")
}
/// Binary units with three significant digits (bytes remain exact).
pub fn fmt_bytes(bytes: u64) -> String {
    if bytes < 1024 {
        return format!("{bytes} B");
    }
    let mut value = bytes as f64;
    let mut unit = 0;
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{} {}", significant(value), units[unit])
}
/// Parse a size per duration (`50GiB/h`) into bytes per second.
pub fn parse_rate(input: &str) -> Result<f64, CoreError> {
    let (size, duration) = input
        .split_once('/')
        .ok_or_else(|| CoreError::Parse(format!("rate needs size/duration: {input}")))?;
    let seconds = match duration.trim() {
        "s" => 1.0,
        "m" | "min" => 60.0,
        "h" => 3600.0,
        "d" => 86400.0,
        other => humantime::parse_duration(other)
            .map_err(|e| CoreError::Parse(e.to_string()))?
            .as_secs_f64(),
    };
    if seconds <= 0.0 {
        return Err(CoreError::Parse("rate duration must be positive".into()));
    }
    Ok(size.parse::<ByteSize>()?.0 as f64 / seconds)
}
/// Signed binary rate, expressed per second.
pub fn fmt_rate(bytes_per_sec: f64) -> String {
    if !bytes_per_sec.is_finite() {
        return "unknown".into();
    }
    let sign = if bytes_per_sec < 0.0 { "-" } else { "" };
    let mut value = bytes_per_sec.abs();
    let units = ["B", "KiB", "MiB", "GiB", "TiB", "PiB", "EiB"];
    let mut unit = 0;
    while value >= 1024.0 && unit < units.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    format!("{sign}{} {}/s", significant(value), units[unit])
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn sizes_and_rates() {
        assert_eq!("1GiB".parse::<ByteSize>().unwrap().0, 1 << 30);
        assert_eq!("1.5KiB".parse::<ByteSize>().unwrap().0, 1536);
        assert_eq!(
            "9007199254740993B".parse::<ByteSize>().unwrap().0,
            9007199254740993
        );
        assert!("18446744073709551616B".parse::<ByteSize>().is_err());
        assert!("-1GiB".parse::<ByteSize>().is_err());
        assert_eq!(
            serde_json::from_str::<ByteSize>("\"1GiB\"").unwrap(),
            ByteSize(1 << 30)
        );
        assert_eq!(
            serde_json::from_str::<ByteSize>("1024").unwrap(),
            ByteSize(1024)
        );
        assert_eq!(
            parse_rate("50GiB/h").unwrap(),
            50.0 * (1u64 << 30) as f64 / 3600.0
        );
        assert!(parse_rate("1GiB/0s").is_err());
        assert_eq!(fmt_bytes(611 << 30), "611 GiB");
        assert_eq!(fmt_bytes(1320702444), "1.23 GiB");
    }
}
