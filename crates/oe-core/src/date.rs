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

    #[test]
    fn day_arithmetic() {
        let a = Date::from_ymd(2026, 2, 27).unwrap();
        let b = Date::from_ymd(2026, 3, 2).unwrap();
        assert_eq!(a.days_until(b), 3);
        assert_eq!(a.add_days(3), b);
    }
}
