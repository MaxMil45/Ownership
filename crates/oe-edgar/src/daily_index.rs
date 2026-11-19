//! The EDGAR daily form index: every filing disseminated on one day, sorted
//! by form type (`https://www.sec.gov/Archives/edgar/daily-index/YYYY/QTRn/form.YYYYMMDD.idx`).
//!
//! The file is fixed-width text. Each row is the form type (it may contain\n//! spaces, as in `SC 13D`), the company name, then the CIK, the filing date\n//! (`YYYYMMDD`) and the path of the complete submission text file.\n//!\n//! **The width of the form-type column is not constant.** It is 17 characters\n//! in 2026 files and 12 in a 2023 file, and the header line does not say. The\n//! parser finds the company column from the data itself: the one position where\n//! (nearly) every row starts a word.
//!
//! One Form 4 appears **once per filer**: once for the issuer and once for each
//! reporting owner, all pointing at the same accession number. Callers that
//! want filings, not rows, should use [`FormIndex::unique_filings`].

use oe_core::Date;
use std::collections::HashSet;

/// Width assumed when the data does not show a clear company column.
const DEFAULT_FORM_WIDTH: usize = 17;

/// Range of byte positions searched for the company column.
const WIDTH_SEARCH: std::ops::RangeInclusive<usize> = 6..=30;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IndexEntry {
    pub form: String,
    pub company: String,
    /// The CIK of this row's filer (issuer or reporting owner).
    pub cik: u64,
    pub date_filed: Date,
    /// Path under `https://www.sec.gov/Archives/`, e.g.
    /// `edgar/data/910638/0001628280-26-074826.txt`.
    pub file_name: String,
}

impl IndexEntry {
    /// The accession number (`0001628280-26-074826`), taken from the file name.
    pub fn accession(&self) -> Option<&str> {
        let stem = self.file_name.rsplit('/').next()?.strip_suffix(".txt")?;
        let valid = stem.len() == 20
            && stem.char_indices().all(|(i, c)| {
                if i == 10 || i == 13 {
                    c == '-'
                } else {
                    c.is_ascii_digit()
                }
            });
        valid.then_some(stem)
    }

    /// Full URL of the complete submission text file.
    pub fn url(&self) -> String {
        format!("https://www.sec.gov/Archives/{}", self.file_name)
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FormIndex {
    pub entries: Vec<IndexEntry>,
    /// The detected width of the form-type column (where the company name starts).
    pub form_width: usize,
    /// Rows after the header that could not be read. Should be zero; the
    /// caller decides whether to warn.
    pub skipped: usize,
}

impl FormIndex {
    /// One entry per accession number whose form is in `forms`, in file
    /// order, keeping the first row seen (the file lists a filing once for
    /// each filer).
    pub fn unique_filings(&self, forms: &[&str]) -> Vec<&IndexEntry> {
        let mut seen = HashSet::new();
        self.entries
            .iter()
            .filter(|e| forms.contains(&e.form.as_str()))
            .filter(|e| e.accession().is_some_and(|a| seen.insert(a)))
            .collect()
    }
}

/// URL of the form index for one filing day. The quarter folder is the
/// calendar quarter of `date`.
pub fn daily_index_url(date: Date) -> String {
    let (y, m, d) = date.ymd();
    let quarter = (m - 1) / 3 + 1;
    format!(
        "https://www.sec.gov/Archives/edgar/daily-index/{y}/QTR{quarter}/form.{y}{m:02}{d:02}.idx"
    )
}

/// Parses a `form.YYYYMMDD.idx` file. Header lines (everything up to the row
/// of dashes) are ignored; a file without the dashed line yields no entries.
pub fn parse_form_index(text: &str) -> FormIndex {
    let mut index = FormIndex::default();
    let mut rows: Vec<&str> = Vec::new();
    let mut in_rows = false;
    for line in text.lines() {
        if !in_rows {
            in_rows = line.starts_with("-----");
            continue;
        }
        if !line.trim().is_empty() {
            rows.push(line);
        }
    }
    index.form_width = detect_form_width(&rows);
    for line in rows {
        match parse_row(line, index.form_width) {
            Some(entry) => index.entries.push(entry),
            None => index.skipped += 1,
        }
    }
    index
}

/// The byte position where the company name starts: the position at which the
/// most rows begin a word, searched in `WIDTH_SEARCH`. Falls back to 17 unless
/// at least 80% of the rows agree.
fn detect_form_width(rows: &[&str]) -> usize {
    let mut counts = [0usize; 31];
    for row in rows {
        let b = row.as_bytes();
        for pos in WIDTH_SEARCH {
            if pos < b.len() && b[pos - 1] == b' ' && b[pos] != b' ' {
                counts[pos] += 1;
            }
        }
    }
    // Highest count wins; on a tie the smaller position (first found).
    let mut best = DEFAULT_FORM_WIDTH;
    let mut best_count = 0;
    for pos in WIDTH_SEARCH {
        if counts[pos] > best_count {
            best = pos;
            best_count = counts[pos];
        }
    }
    if best_count * 5 >= rows.len() * 4 && best_count > 0 {
        best
    } else {
        DEFAULT_FORM_WIDTH
    }
}

fn parse_row(line: &str, form_width: usize) -> Option<IndexEntry> {
    let form = line.get(..form_width)?.trim();
    let rest = line.get(form_width..)?.trim_end();
    // Read the three fixed fields from the right; what is left is the company.
    let (rest, file_name) = rest.rsplit_once(char::is_whitespace)?;
    let (rest, date) = rest.trim_end().rsplit_once(char::is_whitespace)?;
    let (company, cik) = rest.trim_end().rsplit_once(char::is_whitespace)?;
    let date_filed = parse_compact_date(date)?;
    Some(IndexEntry {
        form: form.to_string(),
        company: company.trim().to_string(),
        cik: cik.parse().ok()?,
        date_filed,
        file_name: file_name.to_string(),
    })
    .filter(|e| !e.form.is_empty() && !e.company.is_empty() && e.file_name.starts_with("edgar/"))
}

/// `20261118` → 2026-11-18.
fn parse_compact_date(s: &str) -> Option<Date> {
    if s.len() != 8 || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    Date::from_ymd(
        s[..4].parse().ok()?,
        s[4..6].parse().ok()?,
        s[6..].parse().ok()?,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        let path = format!(
            "{}/../../fixtures/edgar/form_index_synthetic.idx",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    #[test]
    fn parses_rows_including_spaced_form_types() {
        let idx = parse_form_index(&fixture());
        assert_eq!(idx.skipped, 0);
        assert_eq!(idx.entries.len(), 8);
        let first = &idx.entries[0];
        assert_eq!(first.form, "10-K");
        assert_eq!(first.company, "Synthetic Widgets Inc");
        assert_eq!(first.cik, 9_999_901);
        assert_eq!(first.date_filed, Date::from_ymd(2026, 3, 11).unwrap());
        assert_eq!(first.accession(), Some("0009999901-26-000010"));
        assert_eq!(
            first.url(),
            "https://www.sec.gov/Archives/edgar/data/9999901/0009999901-26-000010.txt"
        );
        let spaced: Vec<&str> = idx
            .entries
            .iter()
            .map(|e| e.form.as_str())
            .filter(|f| f.contains(' '))
            .collect();
        assert_eq!(spaced, ["DEF 14A", "SC 13D"]);
    }

    #[test]
    fn company_names_keep_inner_spacing_and_digits() {
        let idx = parse_form_index(&fixture());
        let numeric = idx.entries.iter().find(|e| e.cik == 9_999_904).unwrap();
        assert_eq!(numeric.company, "99 Cents  Only Stores 2");
    }

    #[test]
    fn form4_appears_once_per_filer_but_is_one_filing() {
        let idx = parse_form_index(&fixture());
        let rows = idx.entries.iter().filter(|e| e.form == "4").count();
        assert_eq!(rows, 4);
        let filings = idx.unique_filings(&["4"]);
        let accessions: Vec<&str> = filings.iter().filter_map(|e| e.accession()).collect();
        assert_eq!(accessions, ["0009999901-26-000011", "0009999902-26-000007"]);
        // Amendments only when asked for.
        assert_eq!(idx.unique_filings(&["4", "4/A"]).len(), 3);
    }

    #[test]
    fn detects_the_form_column_width_in_both_layouts() {
        let new = parse_form_index(&fixture());
        assert_eq!(new.form_width, 17);

        let path = format!(
            "{}/../../fixtures/edgar/form_index_synthetic_2023.idx",
            env!("CARGO_MANIFEST_DIR")
        );
        let old = parse_form_index(&std::fs::read_to_string(path).unwrap());
        assert_eq!(old.form_width, 12);
        assert_eq!(old.skipped, 0);
        assert_eq!(old.entries.len(), 8);
        let forms: Vec<&str> = old.entries.iter().map(|e| e.form.as_str()).collect();
        assert_eq!(
            forms,
            [
                "1-A",
                "1-A POS",
                "144",
                "144",
                "144/A",
                "4",
                "DEF 14A",
                "NT 10-Q/A"
            ]
        );
        // With a 17-wide assumption these 144 rows would read as "144         Syn...".
        let filings = old.unique_filings(&["144", "144/A"]);
        let accs: Vec<&str> = filings.iter().filter_map(|e| e.accession()).collect();
        assert_eq!(accs, ["0009999911-23-000003", "0009999912-23-000004"]);
        assert_eq!(old.entries[2].company, "Synthetic Widgets Inc");
    }

    #[test]
    fn unclear_layouts_fall_back_to_the_current_width() {
        // Too few agreeing rows: no detection, the 17-wide default applies.
        assert_eq!(detect_form_width(&[]), 17);
        assert_eq!(detect_form_width(&["x"]), 17);
        let ragged = ["4  A  1 2 3", "10-K   B   2 3 4", "8-K      C    4 5 6"];
        assert_eq!(detect_form_width(&ragged), 17);
    }

    #[test]
    fn counts_rows_it_cannot_read() {
        let text = "junk\n-----\n4                Good Co    123    20260101    edgar/data/123/0000000123-26-000001.txt\nnot a row at all\n";
        let idx = parse_form_index(text);
        assert_eq!(idx.entries.len(), 1);
        assert_eq!(idx.skipped, 1);
        assert!(parse_form_index("no dashed line here").entries.is_empty());
    }

    #[test]
    fn builds_the_daily_index_url_with_the_right_quarter() {
        let url = |y, m, d| daily_index_url(Date::from_ymd(y, m, d).unwrap());
        assert_eq!(
            url(2026, 11, 18),
            "https://www.sec.gov/Archives/edgar/daily-index/2026/QTR4/form.20261118.idx"
        );
        assert!(url(2026, 1, 2).contains("/2026/QTR1/form.20260102.idx"));
        assert!(url(2026, 3, 31).contains("/QTR1/"));
        assert!(url(2026, 4, 1).contains("/QTR2/"));
        assert!(url(2026, 9, 30).contains("/QTR3/"));
    }
}
