//! Owned by: W6-A2a
//! Explicit UTC and transition-zone schedule tests without process TZ changes.
use chrono::{FixedOffset, LocalResult, NaiveDate, NaiveDateTime, TimeZone, Utc};
use tidemarkd::scheduler::{next_run_in_timezone, parse_schedule, parse_window};
fn ts(s: &str) -> i64 {
    chrono::DateTime::parse_from_rfc3339(s).unwrap().timestamp()
}
// Explicit US Eastern transition fixture for 2026, independent of host tzdata.
#[derive(Clone)]
struct Eastern;
impl Eastern {
    fn offset(t: &NaiveDateTime) -> FixedOffset {
        let n = t.and_utc().timestamp();
        FixedOffset::west_opt(
            if (ts("2026-03-08T07:00:00Z")..ts("2026-11-01T06:00:00Z")).contains(&n) {
                4 * 3600
            } else {
                5 * 3600
            },
        )
        .unwrap()
    }
}
impl TimeZone for Eastern {
    type Offset = FixedOffset;
    fn from_offset(_: &Self::Offset) -> Self {
        Eastern
    }
    fn offset_from_local_date(&self, d: &NaiveDate) -> LocalResult<Self::Offset> {
        self.offset_from_local_datetime(&d.and_hms_opt(0, 0, 0).unwrap())
    }
    fn offset_from_local_datetime(&self, d: &NaiveDateTime) -> LocalResult<Self::Offset> {
        let mut valid = Vec::new();
        for hours in [4, 5] {
            let offset = FixedOffset::west_opt(hours * 3600).unwrap();
            let utc = *d + chrono::Duration::hours(i64::from(hours));
            if Self::offset(&utc) == offset {
                valid.push(offset);
            }
        }
        match valid.as_slice() {
            [] => LocalResult::None,
            [a] => LocalResult::Single(*a),
            [a, b] => LocalResult::Ambiguous(*a, *b),
            _ => unreachable!(),
        }
    }
    fn offset_from_utc_date(&self, d: &NaiveDate) -> Self::Offset {
        Self::offset(&d.and_hms_opt(0, 0, 0).unwrap())
    }
    fn offset_from_utc_datetime(&self, d: &NaiveDateTime) -> Self::Offset {
        Self::offset(d)
    }
}
#[test]
fn parsing_and_catchup() {
    for s in [
        "",
        "daily 3:00",
        "daily 24:00",
        "daily 00:60",
        "every 0s",
        "every nope",
        "weekly 03:00",
    ] {
        assert!(parse_schedule(s).is_err(), "{s}");
    }
    for w in ["3:00-05:00", "24:00-06:00", "01:00", "01:00-02:00-03:00"] {
        assert!(parse_window(w).is_err(), "{w}");
    }
    assert_eq!(
        next_run_in_timezone(&parse_schedule("off").unwrap(), None, None, 100, &Utc).unwrap(),
        None
    );
    let s = parse_schedule("every 6h").unwrap();
    assert_eq!(
        next_run_in_timezone(&s, None, Some(0), 30_000, &Utc).unwrap(),
        Some(30_000)
    );
    assert_eq!(
        next_run_in_timezone(&s, None, Some(20_000), 30_000, &Utc).unwrap(),
        Some(41_600)
    );
    assert_eq!(
        next_run_in_timezone(&s, None, None, 30_000, &Utc).unwrap(),
        Some(30_000)
    );
    assert_eq!(
        next_run_in_timezone(
            &parse_schedule("every 1ms").unwrap(),
            None,
            Some(100),
            100,
            &Utc
        )
        .unwrap(),
        Some(101)
    );
}
#[test]
fn due_scan_waits_and_window_wraps() {
    let s = parse_schedule("every 1h").unwrap();
    let w = parse_window("23:00-03:00").unwrap();
    for (now, expected) in [
        ("2026-01-01T22:00:00Z", "2026-01-01T23:00:00Z"),
        ("2026-01-01T23:15:37Z", "2026-01-01T23:15:37Z"),
        ("2026-01-02T02:59:59Z", "2026-01-02T02:59:59Z"),
        ("2026-01-02T03:00:00Z", "2026-01-02T23:00:00Z"),
    ] {
        assert_eq!(
            next_run_in_timezone(&s, Some(&w), None, ts(now), &Utc).unwrap(),
            Some(ts(expected))
        );
    }
    let all = parse_window("00:00-00:00").unwrap();
    assert_eq!(
        next_run_in_timezone(&s, Some(&all), None, 123, &Utc).unwrap(),
        Some(123)
    );
}
#[test]
fn daily_gap_runs_at_first_valid_minute_and_fold_once() {
    let gap = parse_schedule("daily 02:30").unwrap();
    assert_eq!(
        next_run_in_timezone(
            &gap,
            None,
            Some(ts("2026-03-07T07:30:00Z")),
            ts("2026-03-08T06:00:00Z"),
            &Eastern
        )
        .unwrap(),
        Some(ts("2026-03-08T07:00:00Z"))
    );
    let fold = parse_schedule("daily 01:30").unwrap();
    assert_eq!(
        next_run_in_timezone(
            &fold,
            None,
            Some(ts("2026-10-31T05:30:00Z")),
            ts("2026-11-01T04:00:00Z"),
            &Eastern
        )
        .unwrap(),
        Some(ts("2026-11-01T05:30:00Z"))
    );
    assert_eq!(
        next_run_in_timezone(
            &fold,
            None,
            Some(ts("2026-11-01T05:31:00Z")),
            ts("2026-11-01T06:00:00Z"),
            &Eastern
        )
        .unwrap(),
        Some(ts("2026-11-02T06:30:00Z"))
    );
}
#[test]
fn windows_across_both_transitions() {
    let s = parse_schedule("every 1h").unwrap();
    let gap = parse_window("02:30-03:30").unwrap();
    assert_eq!(
        next_run_in_timezone(&s, Some(&gap), None, ts("2026-03-08T06:00:00Z"), &Eastern).unwrap(),
        Some(ts("2026-03-08T07:00:00Z"))
    );
    let fold = parse_window("01:15-01:45").unwrap();
    assert_eq!(
        next_run_in_timezone(&s, Some(&fold), None, ts("2026-11-01T05:50:00Z"), &Eastern).unwrap(),
        Some(ts("2026-11-01T06:15:00Z"))
    );
}
#[test]
fn invalid_clock_is_fallible() {
    assert!(next_run_in_timezone(
        &parse_schedule("every 1s").unwrap(),
        None,
        Some(i64::MAX),
        i64::MAX,
        &Utc
    )
    .is_err());
    assert!(next_run_in_timezone(
        &parse_schedule("daily 00:00").unwrap(),
        None,
        None,
        i64::MAX,
        &Utc
    )
    .is_err());
}
