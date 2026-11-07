//! Dates, times of day, timezones and numbers as typed in the forms.

use chrono::{DateTime, Datelike, Duration, FixedOffset, Local, Months, NaiveDate, NaiveTime};
use chrono_tz::Tz;

/// Timezone for start/end times when neither the entry nor the account names a known one.
pub const FALLBACK_TIMEZONE: Tz = chrono_tz::Europe::Amsterdam;

pub fn last_of_month(month: NaiveDate) -> NaiveDate {
    (month + Months::new(1)).pred_opt().expect("valid date")
}

pub fn monday_of(date: NaiveDate) -> NaiveDate {
    date - Duration::days(date.weekday().num_days_from_monday() as i64)
}

pub fn first_of_month(date: NaiveDate) -> NaiveDate {
    date.with_day(1).expect("day 1 exists")
}

/// `date` moved by `months` (clamped to the month's last day: 31 Jan + 1 month = 28/29 Feb) and
/// then by `days`; None outside chrono's range.
pub fn shift_date(date: NaiveDate, days: i64, months: i32) -> Option<NaiveDate> {
    let shifted = match months {
        0 => Some(date),
        m if m > 0 => date.checked_add_months(Months::new(m.unsigned_abs())),
        m => date.checked_sub_months(Months::new(m.unsigned_abs())),
    };
    shifted?.checked_add_signed(Duration::days(days))
}

pub fn today() -> NaiveDate {
    Local::now().date_naive()
}

/// Parses a time of day, leniently: `12` → 12:00, `930` → 9:30, `1230` → 12:30, and `12:30` or
/// `12.30` as written. Empty means "not given".
pub fn parse_time(s: &str) -> Result<Option<NaiveTime>, String> {
    let s = s.trim();
    if s.is_empty() {
        return Ok(None);
    }
    let invalid = || format!("Invalid time '{s}', use e.g. 9, 930, 12:30");
    let (hours, minutes) = match s.split_once([':', '.']) {
        Some((h, m)) => (h, m),
        None if !s.chars().all(|c| c.is_ascii_digit()) => return Err(invalid()),
        None if s.len() <= 2 => (s, "0"),
        None if s.len() <= 4 => s.split_at(s.len() - 2),
        None => return Err(invalid()),
    };
    let hours: u32 = hours.parse().map_err(|_| invalid())?;
    let minutes: u32 = match minutes {
        "0" => 0,
        m if m.len() == 2 => m.parse().map_err(|_| invalid())?,
        _ => return Err(invalid()),
    };
    NaiveTime::from_hms_opt(hours, minutes, 0).map(Some).ok_or_else(invalid)
}

/// A duration after `+` in the end field: `15` / `15m` minutes, `1h`, `1.5h`, `1h30` / `1h30m`.
fn parse_offset(s: &str) -> Option<i64> {
    let s = s.trim().to_ascii_lowercase();
    let minutes = |m: &str| -> Option<f64> {
        let m = m.trim().trim_end_matches('m').trim();
        if m.is_empty() { Some(0.0) } else { m.parse().ok() }
    };
    let total = match s.split_once('h') {
        Some((h, m)) => h.trim().replace(',', ".").parse::<f64>().ok()? * 60.0 + minutes(m)?,
        None if s.is_empty() => return None,
        None => minutes(&s)?,
    };
    (total.is_finite() && total >= 0.0).then(|| total.round() as i64)
}

/// Parses the end time; `+15`, `+1h`, `+1h30` mean that long after `start`, otherwise as `parse_time`.
pub fn parse_end(start: Option<NaiveTime>, s: &str) -> Result<Option<NaiveTime>, String> {
    let Some(offset) = s.trim().strip_prefix('+') else { return parse_time(s) };
    let minutes = parse_offset(offset).ok_or_else(|| format!("Invalid end '{}', use e.g. +15 or +1h30", s.trim()))?;
    let start = start.ok_or("Fill in the start to use +duration")?;
    let (end, wrapped) = start.overflowing_add_signed(chrono::Duration::minutes(minutes));
    if wrapped != 0 {
        return Err("End must be on the same day".into());
    }
    Ok(Some(end))
}

/// The `HH:MM` an end field of `+N` stands for, given the start field; None when it isn't one
/// (or can't be resolved yet).
pub fn expand_end(start: &str, end: &str) -> Option<String> {
    if !end.trim().starts_with('+') {
        return None;
    }
    let end = parse_end(parse_time(start).ok()?, end).ok()??;
    Some(end.format("%H:%M").to_string())
}

/// The first of `names` that is a known IANA timezone, else `FALLBACK_TIMEZONE`.
pub fn timezone<'a>(names: impl IntoIterator<Item = Option<&'a str>>) -> Tz {
    names.into_iter().flatten().find_map(|n| n.parse().ok()).unwrap_or(FALLBACK_TIMEZONE)
}

/// `date` at `time` on the wall clock of `tz`.
pub fn local(date: NaiveDate, time: NaiveTime, tz: Tz) -> Result<DateTime<FixedOffset>, String> {
    date.and_time(time)
        .and_local_timezone(tz)
        .earliest()
        .map(|t| t.fixed_offset())
        .ok_or_else(|| format!("{date} {time} does not exist in {tz}"))
}

/// A number as typed in a form: surrounding spaces allowed, `,` as the decimal separator too.
pub fn parse_decimal(s: &str) -> Option<f64> {
    s.trim().replace(',', ".").parse().ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shifting_dates() {
        let d = |y, m, d| NaiveDate::from_ymd_opt(y, m, d).unwrap();
        assert_eq!(shift_date(d(2026, 11, 4), 7, 0), Some(d(2026, 11, 11)));
        assert_eq!(shift_date(d(2026, 1, 2), -7, 0), Some(d(2025, 12, 26)));
        assert_eq!(shift_date(d(2026, 1, 31), 0, 1), Some(d(2026, 2, 28)), "clamped to the month's end");
        assert_eq!(shift_date(d(2026, 3, 31), 0, -1), Some(d(2026, 2, 28)));
        assert_eq!(shift_date(NaiveDate::MAX, 1, 0), None);
    }

    #[test]
    fn relative_end() {
        let at = |s: &str, e: &str| expand_end(s, e);
        assert_eq!(at("9", "+15"), Some("09:15".into()));
        assert_eq!(at("9:50", "+ 90"), Some("11:20".into()));
        assert_eq!(at("", "+15"), None);
        assert_eq!(at("9", "10"), None);
        assert_eq!(at("23:50", "+15"), None);
        assert_eq!(at("9", "+1h"), Some("10:00".into()));
        assert_eq!(at("9", "+1H30"), Some("10:30".into()));
        assert_eq!(at("9", "+1h30m"), Some("10:30".into()));
        assert_eq!(at("9", "+1.5h"), Some("10:30".into()));
        assert_eq!(at("9", "+1,25h"), Some("10:15".into()));
        assert_eq!(at("9", "+45m"), Some("09:45".into()));
        assert_eq!(at("9", "+"), None);
        assert_eq!(at("9", "+h"), None);
        assert!(parse_end(None, "+15").is_err());
        assert!(parse_end(None, "+x").is_err());
    }

    fn t(s: &str) -> Option<String> {
        parse_time(s).ok().flatten().map(|t| t.format("%H:%M").to_string())
    }

    #[test]
    fn timezones() {
        assert_eq!(timezone([None, Some("Europe/London")]), chrono_tz::Europe::London);
        assert_eq!(timezone([Some("W. Europe Standard Time"), Some("America/New_York")]), chrono_tz::America::New_York);
        assert_eq!(timezone([None, Some("nonsense")]), FALLBACK_TIMEZONE);

        let date = NaiveDate::from_ymd_opt(2026, 11, 4).unwrap();
        let nine = NaiveTime::from_hms_opt(9, 0, 0).unwrap();
        assert_eq!(local(date, nine, chrono_tz::Europe::Amsterdam).unwrap().to_rfc3339(), "2026-11-04T09:00:00+01:00");
        assert_eq!(local(date, nine, chrono_tz::Europe::London).unwrap().to_rfc3339(), "2026-11-04T09:00:00+00:00");
        // The hour skipped when summer time starts.
        let spring = NaiveDate::from_ymd_opt(2026, 3, 29).unwrap();
        assert!(local(spring, NaiveTime::from_hms_opt(2, 30, 0).unwrap(), chrono_tz::Europe::Amsterdam).is_err());
    }

    #[test]
    fn lenient_times() {
        assert_eq!(t("12"), Some("12:00".into()));
        assert_eq!(t("9"), Some("09:00".into()));
        assert_eq!(t("930"), Some("09:30".into()));
        assert_eq!(t("1230"), Some("12:30".into()));
        assert_eq!(t("0815"), Some("08:15".into()));
        assert_eq!(t("12:30"), Some("12:30".into()));
        assert_eq!(t("12.30"), Some("12:30".into()));
        assert_eq!(t(" 8:05 "), Some("08:05".into()));
        assert_eq!(parse_time("").unwrap(), None);
        for bad in ["24", "1260", "12345", "12:3", "abc", "12:", "2500"] {
            assert!(parse_time(bad).is_err(), "{bad} should be rejected");
        }
    }
}
