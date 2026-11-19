//! The SGML header file of a filing
//! (`https://www.sec.gov/Archives/edgar/data/<CIK>/<accession without dashes>/<accession>.hdr.sgml`).
//!
//! It is under 1 KB and gives the exact acceptance time, which matters for
//! 10-Q and 10-K: the filing date EDGAR prints is the next business day for a
//! report accepted after 5:30 pm Eastern. Real example: accepted 18:00:39 on
//! 2 November 2026, `FILING-DATE` 3 November. Like the full submission's
//! header, the time is US Eastern.

use oe_core::Date;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingHeader {
    pub accession: String,
    /// `TYPE`, e.g. `10-Q`.
    pub form: String,
    /// US Eastern, `YYYY-MM-DDTHH:MM:SS`.
    pub accepted_eastern: String,
    /// `PERIOD`: the end of the reporting period, if the header has one.
    pub period: Option<Date>,
    /// `FILING-DATE`: EDGAR's filing date (not always the acceptance date).
    pub filing_date: Option<Date>,
}

impl FilingHeader {
    /// The Eastern date the SEC accepted the filing (the day the public could
    /// first see it), not EDGAR's filing date.
    pub fn known_on(&self) -> Option<Date> {
        self.accepted_eastern.parse().ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HeaderError {
    NotAHeader,
    Missing(&'static str),
}

impl std::fmt::Display for HeaderError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeaderError::NotAHeader => f.write_str("not an EDGAR SGML header"),
            HeaderError::Missing(what) => write!(f, "header has no {what}"),
        }
    }
}

impl std::error::Error for HeaderError {}

pub fn parse_header_sgml(text: &str) -> Result<FilingHeader, HeaderError> {
    if !text.trim_start().starts_with("<SEC-HEADER>") {
        return Err(HeaderError::NotAHeader);
    }
    let field = |tag: &'static str| {
        let prefix = format!("<{tag}>");
        text.lines()
            .find_map(|l| l.trim().strip_prefix(prefix.as_str()))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or(HeaderError::Missing(tag))
    };

    let stamp = field("ACCEPTANCE-DATETIME")?;
    let accepted_eastern =
        format_stamp(&stamp).ok_or(HeaderError::Missing("readable ACCEPTANCE-DATETIME"))?;
    Ok(FilingHeader {
        accession: field("ACCESSION-NUMBER")?,
        form: field("TYPE")?,
        accepted_eastern,
        period: field("PERIOD").ok().and_then(|p| compact_date(&p)),
        filing_date: field("FILING-DATE").ok().and_then(|p| compact_date(&p)),
    })
}

/// `20260930` → 2026-09-30.
fn compact_date(s: &str) -> Option<Date> {
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Date::from_ymd(
        s[..4].parse().ok()?,
        s[4..6].parse().ok()?,
        s[6..].parse().ok()?,
    )
}

/// `20261102180039` → `2026-11-02T18:00:39`.
fn format_stamp(s: &str) -> Option<String> {
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

    fn fixture() -> String {
        let path = format!(
            "{}/../../fixtures/edgar/filing_header_synthetic.hdr.sgml",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    #[test]
    fn a_report_accepted_after_the_cutoff_is_known_on_the_acceptance_date() {
        let h = parse_header_sgml(&fixture()).unwrap();
        assert_eq!(h.accession, "0009999901-26-000048");
        assert_eq!(h.form, "10-Q");
        assert_eq!(h.accepted_eastern, "2026-11-02T18:00:39");
        // EDGAR dated it the next business day; the public could see it on the 2nd.
        assert_eq!(h.filing_date, Date::from_ymd(2026, 11, 3));
        assert_eq!(h.known_on(), Date::from_ymd(2026, 11, 2));
        assert_eq!(h.period, Date::from_ymd(2026, 9, 30));
    }

    #[test]
    fn rejects_other_text_and_bad_headers() {
        assert_eq!(
            parse_header_sgml("<html>Request Rate Threshold</html>"),
            Err(HeaderError::NotAHeader)
        );
        let no_stamp = fixture().replace("<ACCEPTANCE-DATETIME>20261102180039\n", "");
        assert_eq!(
            parse_header_sgml(&no_stamp),
            Err(HeaderError::Missing("ACCEPTANCE-DATETIME"))
        );
        let bad = fixture().replace("20261102180039", "20261302180039");
        assert!(matches!(
            parse_header_sgml(&bad),
            Err(HeaderError::Missing(_))
        ));
        // PERIOD and FILING-DATE are optional.
        let slim = "<SEC-HEADER>x\n<ACCEPTANCE-DATETIME>20261102180039\n<ACCESSION-NUMBER>0000000000-26-000001\n<TYPE>8-K\n";
        let h = parse_header_sgml(slim).unwrap();
        assert_eq!((h.period, h.filing_date), (None, None));
    }
}
