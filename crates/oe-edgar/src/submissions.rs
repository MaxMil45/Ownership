//! The SEC submissions API (`data.sec.gov/submissions/CIK##########.json`)
//! and the ticker list (`www.sec.gov/files/company_tickers.json`).
//!
//! Both are free and need no API key, only the declared User-Agent.

use crate::json::{self, Json};
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

    /// Acceptance date (`YYYY-MM-DD`): the date the public could first know.
    pub fn known_on(&self) -> Option<&str> {
        self.acceptance_date_time
            .get(..10)
            .or_else(|| self.filing_date.get(..10))
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
    /// Extra history files (older filings) listed under `filings.files`.
    pub older_files: Vec<String>,
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

    let column = |key: &str| -> Result<Vec<&str>, SubmissionsError> {
        recent.get(key).and_then(Json::as_str_vec).ok_or_else(|| {
            shape(format!(
                "filings.recent.{key} is missing or not a string array"
            ))
        })
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

    let recent = (0..n)
        .map(|i| FilingRef {
            accession: accession[i].to_string(),
            form: form[i].to_string(),
            filing_date: filing_date[i].to_string(),
            report_date: report_date[i].to_string(),
            acceptance_date_time: acceptance[i].to_string(),
            primary_document: primary[i].to_string(),
        })
        .collect();

    let older_files = filings
        .get("files")
        .and_then(Json::as_array)
        .unwrap_or_default()
        .iter()
        .map(|f| str_field(f, "name"))
        .filter(|s| !s.is_empty())
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
        assert_eq!(s.older_files, vec!["CIK0009999901-submissions-001.json"]);

        let form4s: Vec<&FilingRef> = s.recent.iter().filter(|f| f.is_insider_form()).collect();
        assert_eq!(form4s.len(), 3);
        let first = form4s[0];
        assert_eq!(first.raw_document(), "wk-form4_1773100000.xml");
        assert_eq!(
            first.raw_document_url(s.cik),
            "https://www.sec.gov/Archives/edgar/data/9999901/000999990126000003/wk-form4_1773100000.xml"
        );
        assert_eq!(first.known_on(), Some("2026-03-11"));
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
        assert_eq!(f.known_on(), Some("2026-02-01"));
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
