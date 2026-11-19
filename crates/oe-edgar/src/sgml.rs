//! The two date formats of EDGAR's SGML headers and indexes, read in one place.

use oe_core::Date;

/// `20261118` → 2026-11-18; `None` unless it is 8 digits forming a real date.
pub(crate) fn compact_date(s: &str) -> Option<Date> {
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Date::from_ymd(
        s[..4].parse().ok()?,
        s[4..6].parse().ok()?,
        s[6..].parse().ok()?,
    )
}

/// `20261102180039` → `2026-11-02T18:00:39`; `None` unless it is 14 digits
/// forming a real date and time. EDGAR writes these in US Eastern time.
pub(crate) fn format_stamp(s: &str) -> Option<String> {
    if s.len() != 14 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let date = compact_date(&s[..8])?;
    let (h, m, sec): (u32, u32, u32) = (
        s[8..10].parse().ok()?,
        s[10..12].parse().ok()?,
        s[12..].parse().ok()?,
    );
    (h < 24 && m < 60 && sec < 60).then(|| format!("{date}T{h:02}:{m:02}:{sec:02}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dates_and_stamps() {
        assert_eq!(compact_date("20261118"), Date::from_ymd(2026, 11, 18));
        for bad in ["", "2026111", "202611180", "20261318", "2026111x"] {
            assert_eq!(compact_date(bad), None, "{bad:?}");
        }
        assert_eq!(
            format_stamp("20261102180039").as_deref(),
            Some("2026-11-02T18:00:39")
        );
        for bad in [
            "",
            "2026110218003",
            "20261102250039",
            "20261102186039",
            "20261302180039",
        ] {
            assert_eq!(format_stamp(bad), None, "{bad:?}");
        }
    }
}
