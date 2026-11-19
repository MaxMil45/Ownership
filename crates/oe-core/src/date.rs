//! A small civil date type (proleptic Gregorian calendar, no time zone).
//!
//! Filings carry plain dates (`2026-03-02`). We only need ordering, day
//! arithmetic and calendar fields, so a dependency-free type is enough.

use std::fmt;
use std::str::FromStr;

/// A calendar date stored as days since 1970-01-01.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    days: i32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DateParseError(pub String);

impl fmt::Display for DateParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid date: {}", self.0)
    }
}

impl std::error::Error for DateParseError {}

impl Date {
    /// Builds a date, returning `None` for impossible dates such as 2026-02-30.
    pub fn from_ymd(year: i32, month: u32, day: u32) -> Option<Date> {
        if !(1..=12).contains(&month) || day == 0 || day > days_in_month(year, month) {
            return None;
        }
        Some(Date {
            days: days_from_civil(year, month, day),
        })
    }

    pub fn ymd(self) -> (i32, u32, u32) {
        civil_from_days(self.days)
    }

    pub fn year(self) -> i32 {
        self.ymd().0
    }

    pub fn month(self) -> u32 {
        self.ymd().1
    }

    pub fn day(self) -> u32 {
        self.ymd().2
    }

    pub fn add_days(self, n: i32) -> Date {
        Date {
            days: self.days + n,
        }
    }

    /// Number of days from `self` to `other` (negative if `other` is earlier).
    pub fn days_until(self, other: Date) -> i32 {
        other.days - self.days
    }
}

impl FromStr for Date {
    type Err = DateParseError;

    /// Parses `YYYY-MM-DD`. Anything after the first ten characters (for
    /// example a time-zone suffix such as `-05:00`) is ignored.
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let s = s.trim();
        let err = || DateParseError(s.to_string());
        let head = s.get(..10).ok_or_else(err)?;
        let bytes = head.as_bytes();
        if bytes[4] != b'-' || bytes[7] != b'-' {
            return Err(err());
        }
        let year: i32 = head[0..4].parse().map_err(|_| err())?;
        let month: u32 = head[5..7].parse().map_err(|_| err())?;
        let day: u32 = head[8..10].parse().map_err(|_| err())?;
        Date::from_ymd(year, month, day).ok_or_else(err)
    }
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let (y, m, d) = self.ymd();
        write!(f, "{y:04}-{m:02}-{d:02}")
    }
}

/// The US Eastern calendar date of a UTC timestamp such as
/// `2026-03-11T22:05:12.000Z`: the date EDGAR uses for a filing's
/// `filingDate` and the day the public could first see it.
///
/// EDGAR's `acceptanceDateTime` is UTC even though filing deadlines are in
/// Eastern time, so a filing accepted at 9 pm in New York on 31 December
/// carries a UTC date of 1 January. Daylight saving follows the US rules:
/// second Sunday of March to first Sunday of November since 2007, first Sunday
/// of April to last Sunday of October for 1987 to 2006 (earlier years use the
/// 1987 rule, far outside EDGAR's electronic era).
///
/// Only the date and the hour are read; returns `None` if either is missing or
/// invalid.
pub fn us_eastern_date(utc: &str) -> Option<Date> {
    let utc = utc.trim();
    let date: Date = utc.parse().ok()?;
    let hour: i32 = utc.get(11..13)?.parse().ok()?;
    if utc.as_bytes().get(10) != Some(&b'T') || !(0..24).contains(&hour) {
        return None;
    }
    let offset = if is_us_dst(date, hour as u32) { 4 } else { 5 };
    Some(if hour < offset {
        date.add_days(-1)
    } else {
        date
    })
}

/// Whether US daylight saving time is in force at `hour` UTC on `date`.
/// Both transitions happen at 2:00 local time: 07:00 UTC when clocks go
/// forward (still EST) and 06:00 UTC when they go back (still EDT).
fn is_us_dst(date: Date, hour: u32) -> bool {
    let year = date.year();
    let (start, end) = if year >= 2007 {
        (nth_sunday(year, 3, 2), nth_sunday(year, 11, 1))
    } else {
        (nth_sunday(year, 4, 1), last_sunday(year, 10))
    };
    (date, hour) >= (start, 7) && (date, hour) < (end, 6)
}

/// 0 = Sunday. 1970-01-01 was a Thursday.
fn weekday(date: Date) -> i32 {
    (date.days + 4).rem_euclid(7)
}

fn nth_sunday(year: i32, month: u32, n: u32) -> Date {
    let first = Date::from_ymd(year, month, 1).expect("valid month");
    let to_sunday = (7 - weekday(first)) % 7;
    first.add_days(to_sunday + 7 * (n as i32 - 1))
}

fn last_sunday(year: i32, month: u32) -> Date {
    let last = Date::from_ymd(year, month, days_in_month(year, month)).expect("valid month");
    last.add_days(-weekday(last))
}

fn is_leap(year: i32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || year % 400 == 0
}

fn days_in_month(year: i32, month: u32) -> u32 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 if is_leap(year) => 29,
        2 => 28,
        _ => 0,
    }
}

// Howard Hinnant's algorithms: http://howardhinnant.github.io/date_algorithms.html
fn days_from_civil(year: i32, month: u32, day: u32) -> i32 {
    let y = if month <= 2 { year - 1 } else { year };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = (y - era * 400) as u32;
    let mp = (month + 9) % 12;
    let doy = (153 * mp + 2) / 5 + day - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146_097 + doe as i32 - 719_468
}

fn civil_from_days(days: i32) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = (if z >= 0 { z } else { z - 146_096 }) / 146_097;
    let doe = (z - era * 146_097) as u32;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe as i32 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_across_a_wide_range() {
        let start = Date::from_ymd(1990, 1, 1).unwrap();
        for n in 0..20_000 {
            let d = start.add_days(n);
            let (y, m, day) = d.ymd();
            assert_eq!(Date::from_ymd(y, m, day), Some(d));
            assert_eq!(d.to_string().parse::<Date>().unwrap(), d);
        }
    }

    #[test]
    fn epoch_and_leap_days() {
        assert_eq!(Date::from_ymd(1970, 1, 1).unwrap().days, 0);
        assert!(Date::from_ymd(2024, 2, 29).is_some());
        assert!(Date::from_ymd(2026, 2, 29).is_none());
        assert!(Date::from_ymd(2026, 13, 1).is_none());
    }

    #[test]
    fn parses_with_suffix_and_rejects_garbage() {
        assert_eq!(
            "2026-03-02-05:00".parse::<Date>().unwrap(),
            Date::from_ymd(2026, 3, 2).unwrap()
        );
        assert!("2026/03/02".parse::<Date>().is_err());
        assert!("26-03-02".parse::<Date>().is_err());
    }

    fn eastern(utc: &str) -> String {
        us_eastern_date(utc).map_or_else(|| "none".to_string(), |d| d.to_string())
    }

    #[test]
    fn eastern_date_matches_real_edgar_filings() {
        // (acceptanceDateTime, filingDate) pairs taken from real Form 4s.
        for (accepted, filed) in [
            ("2025-12-10T00:00:14.000Z", "2025-12-09"),
            ("2026-01-01T02:09:16.000Z", "2025-12-31"),
            ("2026-02-28T00:00:21.000Z", "2026-02-27"),
            ("2026-08-27T00:10:33.000Z", "2026-08-26"),
            ("2026-10-05T22:42:45.000Z", "2026-10-05"),
            ("2026-11-04T02:30:04.000Z", "2026-11-03"),
        ] {
            assert_eq!(eastern(accepted), filed, "{accepted}");
        }
    }

    #[test]
    fn eastern_date_follows_daylight_saving() {
        // Weekdays used below: 2026-03-08 and 2026-11-01 are Sundays.
        assert_eq!(weekday(Date::from_ymd(2026, 3, 8).unwrap()), 0);
        assert_eq!(weekday(Date::from_ymd(2026, 11, 1).unwrap()), 0);
        // Before the spring change (07:00Z on 8 March) the offset is -5.
        assert_eq!(eastern("2026-03-08T04:30:00Z"), "2026-03-07");
        assert_eq!(eastern("2026-03-08T07:00:00Z"), "2026-03-08");
        // After it the offset is -4: 03:59Z is 23:59 the evening before.
        assert_eq!(eastern("2026-03-09T03:59:00Z"), "2026-03-08");
        assert_eq!(eastern("2026-03-09T04:00:00Z"), "2026-03-09");
        // Still summer time until 06:00Z on 1 November, then -5.
        assert_eq!(eastern("2026-11-01T04:30:00Z"), "2026-11-01");
        assert_eq!(eastern("2026-11-02T04:59:00Z"), "2026-11-01");
        assert_eq!(eastern("2026-11-02T05:00:00Z"), "2026-11-02");
        // Older rule: 2006 summer time ran from 2 April to 29 October.
        assert_eq!(eastern("2006-03-20T04:30:00Z"), "2006-03-19");
        assert_eq!(eastern("2006-10-20T04:30:00Z"), "2006-10-20");
        assert_eq!(eastern("2006-10-30T04:30:00Z"), "2006-10-29");
    }

    #[test]
    fn eastern_date_rejects_incomplete_input() {
        assert_eq!(eastern(""), "none");
        assert_eq!(eastern("2026-03-11"), "none");
        assert_eq!(eastern("2026-03-11 22:05:12Z"), "none");
        assert_eq!(eastern("2026-03-11T25:00:00Z"), "none");
    }

    #[test]
    fn day_arithmetic() {
        let a = Date::from_ymd(2026, 2, 27).unwrap();
        let b = Date::from_ymd(2026, 3, 2).unwrap();
        assert_eq!(a.days_until(b), 3);
        assert_eq!(a.add_days(3), b);
    }
}
