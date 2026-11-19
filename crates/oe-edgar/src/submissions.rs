//! The SEC submissions API (`data.sec.gov/submissions/CIK##########.json`)
//! and the ticker list (`www.sec.gov/files/company_tickers.json`).
//!
//! Both are free and need no API key, only the declared User-Agent.

use crate::json::{self, Json};
use oe_core::{us_eastern_date, Date};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SubmissionsError {
    Json(json::JsonError),
    Shape(String),
}

impl fmt::Display for SubmissionsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            SubmissionsError::Json(e) => write!(f, "{e}"),
            SubmissionsError::Shape(m) => write!(f, "unexpected SEC JSON layout: {m}"),
        }
    }
}

impl std::error::Error for SubmissionsError {}

impl From<json::JsonError> for SubmissionsError {
    fn from(e: json::JsonError) -> Self {
        SubmissionsError::Json(e)
    }
}

fn shape(m: impl Into<String>) -> SubmissionsError {
    SubmissionsError::Shape(m.into())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FormerName {
    pub name: String,
    pub from: String,
    pub to: String,
}

/// One row of the submissions API's `filings.recent` table.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingRef {
    pub accession: String,
    pub form: String,
    pub filing_date: String,
    pub report_date: String,
    /// When the SEC accepted the filing, e.g. `2026-11-18T16:05:12.000Z`.
    pub acceptance_date_time: String,
    pub primary_document: String,
}

impl FilingRef {
    /// The primary document's file name with any rendering-stylesheet folder
    /// (such as `xslF345X05/`) removed: the raw XML rather than the web page.
    pub fn raw_document(&self) -> &str {
        match self.primary_document.split_once('/') {
            Some((folder, rest)) if folder.starts_with("xsl") => rest,
            _ => &self.primary_document,
        }
    }

    pub fn accession_compact(&self) -> String {
        self.accession.replace('-', "")
    }

    /// Full URL of the raw primary document in the EDGAR archive.
    pub fn raw_document_url(&self, cik: u64) -> String {
        format!(
            "https://www.sec.gov/Archives/edgar/data/{cik}/{}/{}",
            self.accession_compact(),
            self.raw_document()
        )
    }

    /// The US Eastern date the SEC accepted the filing: the day the public
    /// could first know. `acceptanceDateTime` is UTC, so its first ten
    /// characters would be a day late for filings accepted after about 8 pm
    /// in New York; this converts first. Falls back to `filingDate` (already
    /// an Eastern date) if the timestamp is missing.
    pub fn known_on(&self) -> Option<Date> {
        us_eastern_date(&self.acceptance_date_time).or_else(|| self.filing_date.parse().ok())
    }

    pub fn is_insider_form(&self) -> bool {
        matches!(self.form.as_str(), "3" | "3/A" | "4" | "4/A" | "5" | "5/A")
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Submissions {
    pub cik: u64,
    pub name: String,
    pub tickers: Vec<String>,
    pub former_names: Vec<FormerName>,
    /// Newest first, as the SEC returns them.
    pub recent: Vec<FilingRef>,
    /// Extra history pages (older filings) listed under `filings.files`.
    pub older_files: Vec<HistoryFileRef>,
}

/// One entry of `filings.files`: an older page of the filing list.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryFileRef {
    /// File name, e.g. `CIK0001318605-submissions-001.json`.
    pub name: String,
    /// `YYYY-MM-DD` of the oldest and newest filing on the page; empty if absent.
    pub filing_from: String,
    pub filing_to: String,
}

fn str_field(v: &Json, key: &str) -> String {
    v.get(key)
        .and_then(Json::as_str)
        .unwrap_or_default()
        .to_string()
}

/// Parses a submissions API response.
pub fn parse_submissions(text: &str) -> Result<Submissions, SubmissionsError> {
    let root = json::parse(text)?;

    let cik = match root.get("cik") {
        Some(Json::String(s)) => s.trim().parse::<u64>().ok(),
        Some(Json::Number(n)) => Some(*n as u64),
        _ => None,
    }
    .ok_or_else(|| shape("missing cik"))?;

    let tickers = root
        .get("tickers")
        .and_then(Json::as_str_vec)
        .unwrap_or_default()
        .into_iter()
        .map(str::to_string)
        .collect();

    let former_names = root
        .get("formerNames")
        .and_then(Json::as_array)
        .unwrap_or_default()
        .iter()
        .map(|f| FormerName {
            name: str_field(f, "name"),
            from: str_field(f, "from"),
            to: str_field(f, "to"),
        })
        .collect();

    let filings = root
        .get("filings")
        .ok_or_else(|| shape("missing filings"))?;
    let recent = filings
        .get("recent")
        .ok_or_else(|| shape("missing filings.recent"))?;
    let recent = parse_filing_table(recent, "filings.recent")?;

    let older_files = filings
        .get("files")
        .and_then(Json::as_array)
        .unwrap_or_default()
        .iter()
        .map(|f| HistoryFileRef {
            name: str_field(f, "name"),
            filing_from: str_field(f, "filingFrom"),
            filing_to: str_field(f, "filingTo"),
        })
        .filter(|f| !f.name.is_empty())
        .collect();

    Ok(Submissions {
        cik,
        name: str_field(&root, "name"),
        tickers,
        former_names,
        recent,
        older_files,
    })
}

/// Reads the column-by-column filing table that `filings.recent` and every
/// older history page share. `place` names the table in error messages.
fn parse_filing_table(table: &Json, place: &str) -> Result<Vec<FilingRef>, SubmissionsError> {
    let column = |key: &str| -> Result<Vec<&str>, SubmissionsError> {
        table
            .get(key)
            .and_then(Json::as_str_vec)
            .ok_or_else(|| shape(format!("{place}.{key} is missing or not a string array")))
    };
    let accession = column("accessionNumber")?;
    let form = column("form")?;
    let filing_date = column("filingDate")?;
    let report_date = column("reportDate")?;
    let acceptance = column("acceptanceDateTime")?;
    let primary = column("primaryDocument")?;

    let n = accession.len();
    for (name, len) in [
        ("form", form.len()),
        ("filingDate", filing_date.len()),
        ("reportDate", report_date.len()),
        ("acceptanceDateTime", acceptance.len()),
        ("primaryDocument", primary.len()),
    ] {
        if len != n {
            return Err(shape(format!(
                "column {name} has {len} rows, accessionNumber has {n}"
            )));
        }
    }

    Ok((0..n)
        .map(|i| FilingRef {
            accession: accession[i].to_string(),
            form: form[i].to_string(),
            filing_date: filing_date[i].to_string(),
            report_date: report_date[i].to_string(),
            acceptance_date_time: acceptance[i].to_string(),
            primary_document: primary[i].to_string(),
        })
        .collect())
}

/// Parses one older history page named in `Submissions::older_files`
/// (`data.sec.gov/submissions/CIK##########-submissions-001.json`). It holds
/// the same table as `filings.recent`, but at the top level of the JSON.
/// Newest first, like the recent table.
pub fn parse_history_page(text: &str) -> Result<Vec<FilingRef>, SubmissionsError> {
    let root = json::parse(text)?;
    parse_filing_table(&root, "history page")
}

/// URL of an older history page, given its `name` from `older_files`.
pub fn history_page_url(name: &str) -> String {
    format!("https://data.sec.gov/submissions/{name}")
}

/// URL of the SEC's ticker-to-CIK list.
pub const COMPANY_TICKERS_URL: &str = "https://www.sec.gov/files/company_tickers.json";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TickerEntry {
    pub cik: u64,
    pub ticker: String,
    pub title: String,
}

/// Parses `company_tickers.json`: `{"0": {"cik_str": 320193, "ticker": "AAPL", "title": "Apple Inc."}, ...}`.
pub fn parse_company_tickers(text: &str) -> Result<Vec<TickerEntry>, SubmissionsError> {
    let root = json::parse(text)?;
    let pairs = root
        .as_object()
        .ok_or_else(|| shape("ticker list is not an object"))?;
    pairs
        .iter()
        .map(|(_, v)| {
            let cik = match v.get("cik_str") {
                Some(Json::Number(n)) => Some(*n as u64),
                Some(Json::String(s)) => s.parse().ok(),
                _ => None,
            }
            .ok_or_else(|| shape("ticker entry without cik_str"))?;
            Ok(TickerEntry {
                cik,
                ticker: str_field(v, "ticker"),
                title: str_field(v, "title"),
            })
        })
        .collect()
}

/// Finds a CIK by ticker, ignoring case.
pub fn find_ticker<'a>(entries: &'a [TickerEntry], ticker: &str) -> Option<&'a TickerEntry> {
    entries
        .iter()
        .find(|e| e.ticker.eq_ignore_ascii_case(ticker.trim()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!("{}/../../fixtures/edgar/{name}", env!("CARGO_MANIFEST_DIR"));
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    #[test]
    fn parses_submissions_sample() {
        let s = parse_submissions(&fixture("submissions_synthetic.json")).unwrap();
        assert_eq!(s.cik, 9_999_901);
        assert_eq!(s.name, "Synthetic Widgets Inc");
        assert_eq!(s.tickers, vec!["SYNW"]);
        assert_eq!(s.former_names[0].name, "Synthetic Gadgets Corp");
        assert_eq!(s.recent.len(), 4);
        assert_eq!(
            s.older_files,
            vec![HistoryFileRef {
                name: "CIK0009999901-submissions-001.json".into(),
                filing_from: "2003-01-02".into(),
                filing_to: "2024-12-31".into(),
            }]
        );

        let form4s: Vec<&FilingRef> = s.recent.iter().filter(|f| f.is_insider_form()).collect();
        assert_eq!(form4s.len(), 3);
        let first = form4s[0];
        assert_eq!(first.raw_document(), "wk-form4_1773100000.xml");
        assert_eq!(
            first.raw_document_url(s.cik),
            "https://www.sec.gov/Archives/edgar/data/9999901/000999990126000003/wk-form4_1773100000.xml"
        );
        assert_eq!(first.known_on(), Date::from_ymd(2026, 3, 11));
    }

    #[test]
    fn known_on_uses_the_eastern_date_not_the_utc_date() {
        // Accepted 02:09 UTC on 1 January = 21:09 in New York on 31 December.
        let late = FilingRef {
            accession: "0000000000-25-000001".into(),
            form: "4".into(),
            filing_date: "2024-12-31".into(),
            report_date: String::new(),
            acceptance_date_time: "2025-01-01T02:09:16.000Z".into(),
            primary_document: "x.xml".into(),
        };
        assert_eq!(late.known_on(), Date::from_ymd(2024, 12, 31));
        // A bad timestamp falls back to the filing date.
        let broken = FilingRef {
            acceptance_date_time: "garbage".into(),
            ..late
        };
        assert_eq!(broken.known_on(), Date::from_ymd(2024, 12, 31));
    }

    #[test]
    fn parses_an_older_history_page() {
        let rows = parse_history_page(&fixture("history_page_synthetic.json")).unwrap();
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].accession, "0009999901-24-000009");
        assert_eq!(rows[0].known_on(), Date::from_ymd(2024, 12, 31));
        assert_eq!(rows[1].form, "5");
        assert!(rows[1].is_insider_form());
        assert_eq!(
            history_page_url("CIK0009999901-submissions-001.json"),
            "https://data.sec.gov/submissions/CIK0009999901-submissions-001.json"
        );
        // A recent-style document is not a history page.
        assert!(parse_history_page(&fixture("submissions_synthetic.json")).is_err());
    }

    #[test]
    fn raw_document_keeps_plain_names() {
        let f = FilingRef {
            accession: "0000000000-26-000001".into(),
            form: "10-K".into(),
            filing_date: "2026-02-01".into(),
            report_date: String::new(),
            acceptance_date_time: String::new(),
            primary_document: "report/main.htm".into(),
        };
        assert_eq!(f.raw_document(), "report/main.htm");
        assert_eq!(f.known_on(), Date::from_ymd(2026, 2, 1));
    }

    #[test]
    fn rejects_ragged_columns() {
        let bad = r#"{"cik":"1","filings":{"recent":{"accessionNumber":["a","b"],"form":["4"],
            "filingDate":["x","y"],"reportDate":["x","y"],"acceptanceDateTime":["x","y"],"primaryDocument":["x","y"]}}}"#;
        assert!(matches!(
            parse_submissions(bad),
            Err(SubmissionsError::Shape(_))
        ));
    }

    #[test]
    fn looks_up_tickers() {
        let entries = parse_company_tickers(&fixture("company_tickers_synthetic.json")).unwrap();
        assert_eq!(entries.len(), 2);
        let e = find_ticker(&entries, "synw").unwrap();
        assert_eq!(e.cik, 9_999_901);
        assert!(find_ticker(&entries, "NOPE").is_none());
    }
}
