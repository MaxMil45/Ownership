//! The "complete submission text file" of a filing
//! (`https://www.sec.gov/Archives/edgar/data/<CIK>/<accession>.txt`).
//!
//! It is the one document the daily index points at. For an ownership form it
//! holds an SGML header and the ownership XML, so a single request gives both
//! the XML and the exact acceptance time, with no need to know the XML's file
//! name.
//!
//! The header's `<ACCEPTANCE-DATETIME>` is **US Eastern** local time
//! (`YYYYMMDDHHMMSS`), unlike the submissions API, whose `acceptanceDateTime`
//! is UTC. Checked on a real filing: header `20261005184245`, API
//! `2026-10-05T22:42:45.000Z`.

use oe_core::Date;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FullSubmission {
    pub accession: String,
    /// `CONFORMED SUBMISSION TYPE`, e.g. `4` or `4/A`.
    pub form: String,
    /// Acceptance time in US Eastern local time, `YYYY-MM-DDTHH:MM:SS`.
    pub accepted_eastern: String,
    /// The `<ownershipDocument>` element, if the submission contains one.
    pub ownership_xml: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmissionError {
    NotASubmission,
    Missing(&'static str),
}

impl std::fmt::Display for SubmissionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            SubmissionError::NotASubmission => f.write_str("not an EDGAR submission text file"),
            SubmissionError::Missing(what) => write!(f, "submission header has no {what}"),
        }
    }
}

impl std::error::Error for SubmissionError {}

impl FullSubmission {
    /// The Eastern date the SEC accepted the filing: the day the public could
    /// first know. The header is already Eastern, so no conversion is needed.
    pub fn known_on(&self) -> Option<Date> {
        self.accepted_eastern.parse().ok()
    }
}

pub fn parse_full_submission(text: &str) -> Result<FullSubmission, SubmissionError> {
    if !text.trim_start().starts_with("<SEC-DOCUMENT>") {
        return Err(SubmissionError::NotASubmission);
    }
    // Only the header (before the first <DOCUMENT>) is searched for fields, so
    // nothing inside the filing itself can be mistaken for one.
    let header = text.split("<DOCUMENT>").next().unwrap_or(text);

    let stamp = header
        .lines()
        .find_map(|l| l.trim().strip_prefix("<ACCEPTANCE-DATETIME>"))
        .map(str::trim)
        .ok_or(SubmissionError::Missing("ACCEPTANCE-DATETIME"))?;
    let accepted_eastern = format_stamp(stamp).ok_or(SubmissionError::Missing(
        "readable ACCEPTANCE-DATETIME (expected 14 digits)",
    ))?;

    let field = |key: &'static str| {
        header
            .lines()
            .find_map(|l| l.strip_prefix(key))
            .map(|v| v.trim().to_string())
            .filter(|v| !v.is_empty())
            .ok_or(SubmissionError::Missing(key))
    };

    let ownership_xml = text.find("<ownershipDocument>").and_then(|start| {
        let end_tag = "</ownershipDocument>";
        let end = text[start..].find(end_tag)? + start + end_tag.len();
        Some(format!("<?xml version=\"1.0\"?>\n{}\n", &text[start..end]))
    });

    Ok(FullSubmission {
        accession: field("ACCESSION NUMBER:")?,
        form: field("CONFORMED SUBMISSION TYPE:")?,
        accepted_eastern,
        ownership_xml,
    })
}

/// `20261005184245` → `2026-10-05T18:42:45`, or `None` if it is not 14 digits
/// forming a real date and time.
fn format_stamp(s: &str) -> Option<String> {
    if s.len() != 14 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let date = Date::from_ymd(
        s[..4].parse().ok()?,
        s[4..6].parse().ok()?,
        s[6..8].parse().ok()?,
    )?;
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
            "{}/../../fixtures/edgar/submission_synthetic.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    #[test]
    fn reads_header_and_extracts_the_xml() {
        let s = parse_full_submission(&fixture()).unwrap();
        assert_eq!(s.accession, "0009999901-26-000011");
        assert_eq!(s.form, "4");
        assert_eq!(s.accepted_eastern, "2026-03-11T16:54:50");
        assert_eq!(s.known_on(), Date::from_ymd(2026, 3, 11));
        let xml = s.ownership_xml.unwrap();
        assert!(xml.starts_with("<?xml version=\"1.0\"?>\n<ownershipDocument>"));
        assert!(xml.trim_end().ends_with("</ownershipDocument>"));
        assert!(!xml.contains("<XML>") && !xml.contains("</DOCUMENT>"));
    }

    #[test]
    fn extracted_xml_keeps_its_structure() {
        // The oe-form4 crate is not a dependency here (the CLI tests parse the
        // extracted XML with it); check one issuer and one owner survive.
        let xml = parse_full_submission(&fixture())
            .unwrap()
            .ownership_xml
            .unwrap();
        assert_eq!(xml.matches("<issuerCik>").count(), 1);
        assert_eq!(xml.matches("<reportingOwner>").count(), 1);
    }

    #[test]
    fn late_evening_stays_on_the_eastern_date() {
        let text = fixture().replace("20260311165450", "20251231210916");
        let s = parse_full_submission(&text).unwrap();
        assert_eq!(s.accepted_eastern, "2025-12-31T21:09:16");
        assert_eq!(s.known_on(), Date::from_ymd(2025, 12, 31));
    }

    #[test]
    fn submission_without_ownership_xml_has_none() {
        let text = "<SEC-DOCUMENT>0000000000-26-000001.txt : 20260311\n<SEC-HEADER>x\n<ACCEPTANCE-DATETIME>20260311101500\nACCESSION NUMBER:\t\t0000000000-26-000001\nCONFORMED SUBMISSION TYPE:\t8-K\n</SEC-HEADER>\n<DOCUMENT>\n<TYPE>8-K\n";
        let s = parse_full_submission(text).unwrap();
        assert_eq!(s.form, "8-K");
        assert!(s.ownership_xml.is_none());
    }

    #[test]
    fn rejects_other_text_and_bad_headers() {
        assert_eq!(
            parse_full_submission("<html>Request Rate Threshold Exceeded</html>"),
            Err(SubmissionError::NotASubmission)
        );
        let no_stamp = fixture().replace("<ACCEPTANCE-DATETIME>20260311165450", "");
        assert!(matches!(
            parse_full_submission(&no_stamp),
            Err(SubmissionError::Missing(_))
        ));
        let bad_stamp = fixture().replace("20260311165450", "20261301165450");
        assert!(matches!(
            parse_full_submission(&bad_stamp),
            Err(SubmissionError::Missing(_))
        ));
    }
}
