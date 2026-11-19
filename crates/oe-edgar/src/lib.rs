//! Polite access to SEC EDGAR.
//!
//! The SEC's fair-access policy asks every client to declare who it is in the
//! `User-Agent` header (a name and an email address) and to stay under ten
//! requests per second. This crate enforces both, so no other code has to
//! remember to.

pub mod daily_index;
pub mod feed;
pub mod filing_summary;
pub mod header;
pub mod json;
mod markup;
pub mod ratelimit;
mod sgml;
pub mod submission;
pub mod submissions;

pub use daily_index::{daily_index_url, parse_form_index, FormIndex, IndexEntry};
pub use feed::{
    parse_atom_feed, unique_accessions, FeedEntry, FeedError, LATEST_OWNERSHIP_FEED_URL,
};
pub use filing_summary::{parse_filing_summary, FilingSummary, ReportRef};
pub use header::{parse_header_sgml, FilingHeader, HeaderError};
pub use submission::{parse_full_submission, FullSubmission, SubmissionError};
pub use submissions::{
    find_ticker, history_page_url, parse_company_tickers, parse_history_page, parse_submissions,
    FilingRef, HistoryFileRef, Submissions, COMPANY_TICKERS_URL,
};

#[cfg(feature = "net")]
mod client;
#[cfg(feature = "net")]
pub use client::{EdgarClient, EdgarError};

use std::fmt;

/// Environment variable holding the SEC contact string, e.g.
/// `Jane Doe jane.doe@example.com`.
pub const USER_AGENT_ENV: &str = "OE_USER_AGENT";

/// Requests per second the client allows itself; the SEC limit is ten.
pub const REQUESTS_PER_SECOND: u32 = 8;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct UserAgentError(pub String);

impl fmt::Display for UserAgentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for UserAgentError {}

/// Checks that a User-Agent names a person or organisation and an email.
pub fn validate_user_agent(ua: &str) -> Result<String, UserAgentError> {
    let ua = ua.trim();
    let has_email = ua
        .split_whitespace()
        .any(|w| w.contains('@') && w.contains('.') && !w.starts_with('@'));
    let has_name = ua.split_whitespace().any(|w| !w.contains('@'));
    if has_email && has_name {
        Ok(ua.to_string())
    } else {
        Err(UserAgentError(format!(
            "{USER_AGENT_ENV} must contain a name and an email address, \
             e.g. \"Jane Doe jane.doe@example.com\" (got {ua:?})"
        )))
    }
}

/// Reads and validates the User-Agent from `OE_USER_AGENT`.
pub fn user_agent_from_env() -> Result<String, UserAgentError> {
    let raw = std::env::var(USER_AGENT_ENV).map_err(|_| {
        UserAgentError(format!(
            "set {USER_AGENT_ENV} to your name and email, e.g. \"Jane Doe jane.doe@example.com\""
        ))
    })?;
    validate_user_agent(&raw)
}

/// Turns a response body into text. EDGAR's Atom feed declares ISO-8859-1, and
/// older documents can hold stray non-UTF-8 bytes; bytes that are not valid
/// UTF-8 are read as Latin-1 instead of failing the whole download.
pub fn decode_body(bytes: Vec<u8>) -> String {
    String::from_utf8(bytes)
        .unwrap_or_else(|e| e.into_bytes().into_iter().map(char::from).collect())
}

/// URL of the EDGAR JSON listing every filing for a filer.
pub fn submissions_url(cik: u64) -> String {
    format!("https://data.sec.gov/submissions/CIK{cik:010}.json")
}

/// URL of a filing's folder in the EDGAR archive.
pub fn filing_folder_url(filer_cik: u64, accession_compact: &str) -> String {
    format!("https://www.sec.gov/Archives/edgar/data/{filer_cik}/{accession_compact}/")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn user_agent_needs_name_and_email() {
        assert!(validate_user_agent("Jane Doe jane.doe@example.com").is_ok());
        assert!(validate_user_agent("jane.doe@example.com").is_err());
        assert!(validate_user_agent("Jane Doe").is_err());
        assert!(validate_user_agent("").is_err());
    }

    #[test]
    fn decodes_utf8_and_falls_back_to_latin1() {
        assert_eq!(decode_body("Société".as_bytes().to_vec()), "Société");
        // 0xE9 alone is Latin-1 "é" and not valid UTF-8.
        assert_eq!(decode_body(b"Soci\xe9t\xe9".to_vec()), "Société");
        assert_eq!(decode_body(Vec::new()), "");
    }

    #[test]
    fn builds_urls() {
        assert_eq!(
            submissions_url(320193),
            "https://data.sec.gov/submissions/CIK0000320193.json"
        );
        assert_eq!(
            filing_folder_url(320193, "000032019326000001"),
            "https://www.sec.gov/Archives/edgar/data/320193/000032019326000001/"
        );
    }
}
