//! Public officials' financial disclosures (ADR 0010). For now: the House
//! Clerk's yearly index, which lists every disclosure filed with the Clerk in
//! a year, and the addresses of the periodic transaction reports (PTRs) it
//! names. Std-only (ADR 0001); downloading is in `oe-cli`, behind `net`.
//!
//! Checked on the real site, 2026-11-24: the index for a year is
//! `public_disc/financial-pdfs/<year>FD.zip` (about 100 KB, refreshed daily,
//! no sign-in or terms page), holding `<year>FD.txt` (tab-separated, a header
//! row, CRLF) and the same rows as `<year>FD.xml`. A PTR's PDF is
//! `public_disc/ptr-pdfs/<year>/<DocID>.pdf` (55 to 63 KB in the two checked).
//! In 2025 the index had 515 PTRs from 108 filers, every one a sitting member
//! (497 with the prefix "Hon.", the rest one member without it).
//!
//! Only PTRs are read: trades, filed by members. Annual reports (they list
//! assets such as real estate) and candidates' filings are left out (ADR 0010).
//! Using these reports is limited by law (5 U.S.C. 13107, formerly section
//! 105(c) of the Ethics in Government Act): see [`USE_LIMITS`].

pub mod legislators;
pub mod link;

pub use legislators::{
    parse_committees, parse_legislators, parse_memberships, Chamber, Committee, Legislator,
    Membership, Term,
};
pub use link::{link_report, Link};

use oe_core::Date;

pub const HOUSE_BASE: &str = "https://disclosures-clerk.house.gov/public_disc";

/// The legal limits on obtaining or using these reports, shown before any
/// download (ADR 0010, 3.2).
pub const USE_LIMITS: &str = "\
House financial disclosure reports may not be obtained or used:
  - for any unlawful purpose;
  - for any commercial purpose, other than by news and communications media
    for dissemination to the general public;
  - for determining or establishing anyone's credit rating;
  - for soliciting money, directly or indirectly, for political, charitable
    or other purposes.
(Ethics in Government Act, section 105(c); 5 U.S.C. 13107.) The reports are
kept on this machine only: never commit, publish or redistribute them.
Set OE_OFFICIALS_ACK=1 yourself to confirm you have read this.";

/// `FilingType` of a periodic transaction report in the index.
pub const PTR: &str = "P";

/// One row of the House Clerk's yearly index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HouseIndexRow {
    pub prefix: String,
    pub last: String,
    pub first: String,
    pub suffix: String,
    /// One letter: `P` periodic transaction report, `O` annual, `C` candidate, ...
    pub filing_type: String,
    /// State and district, e.g. `MI04`.
    pub state_district: String,
    pub year: i32,
    /// As written (`3/24/2025`), read when it is a valid US date.
    pub filing_date: Option<Date>,
    pub doc_id: String,
}

impl HouseIndexRow {
    pub fn is_ptr(&self) -> bool {
        self.filing_type == PTR
    }

    /// `Hon. Robert B. Aderholt`: the name as the index gives it.
    pub fn display_name(&self) -> String {
        [&self.prefix, &self.first, &self.last, &self.suffix]
            .iter()
            .filter(|s| !s.is_empty())
            .map(|s| s.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    /// Where this filing's PDF is, for a PTR.
    pub fn ptr_pdf_url(&self) -> String {
        ptr_pdf_url(self.year, &self.doc_id)
    }
}

pub fn index_zip_url(year: i32) -> String {
    format!("{HOUSE_BASE}/financial-pdfs/{year}FD.zip")
}

/// The name of the tab-separated index inside the year's zip.
pub fn index_txt_name(year: i32) -> String {
    format!("{year}FD.txt")
}

pub fn ptr_pdf_url(year: i32, doc_id: &str) -> String {
    format!("{HOUSE_BASE}/ptr-pdfs/{year}/{doc_id}.pdf")
}

/// A document id is digits only (it becomes part of a file name and a URL).
fn valid_doc_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 12 && id.bytes().all(|b| b.is_ascii_digit())
}

/// Reads the tab-separated index (`<year>FD.txt`). Rows with the wrong number
/// of fields, a bad year or a document id that is not all digits are counted
/// in the second value, not returned.
pub fn parse_house_index(text: &str) -> Result<(Vec<HouseIndexRow>, usize), String> {
    let mut lines = text.trim_start_matches('\u{feff}').lines();
    let header: Vec<&str> = lines
        .next()
        .ok_or("the index is empty")?
        .trim_end_matches('\r')
        .split('\t')
        .collect();
    const EXPECTED: [&str; 9] = [
        "Prefix",
        "Last",
        "First",
        "Suffix",
        "FilingType",
        "StateDst",
        "Year",
        "FilingDate",
        "DocID",
    ];
    if header != EXPECTED {
        return Err(format!(
            "unexpected index header {header:?}; expected {EXPECTED:?}"
        ));
    }
    let (mut rows, mut skipped) = (Vec::new(), 0usize);
    for line in lines {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            continue;
        }
        let c: Vec<&str> = line.split('\t').map(str::trim).collect();
        let (Ok(year), true) = (c.get(6).unwrap_or(&"").parse::<i32>(), c.len() == 9) else {
            skipped += 1;
            continue;
        };
        if !valid_doc_id(c[8]) {
            skipped += 1;
            continue;
        }
        rows.push(HouseIndexRow {
            prefix: c[0].to_string(),
            last: c[1].to_string(),
            first: c[2].to_string(),
            suffix: c[3].to_string(),
            filing_type: c[4].to_string(),
            state_district: c[5].to_string(),
            year,
            filing_date: oe_core::parse_us_date(c[7]),
            doc_id: c[8].to_string(),
        });
    }
    Ok((rows, skipped))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Invented rows in the real layout (no real reports in the repository).
    const INDEX: &str =
        "\u{feff}Prefix\tLast\tFirst\tSuffix\tFilingType\tStateDst\tYear\tFilingDate\tDocID\r\n\
Hon.\tExample\tAlex B.\t\tP\tZZ01\t2025\t9/10/2025\t20099001\r\n\
\tSample\tJordan\tJr.\tC\tZZ02\t2025\t10/12/2025\t10099002\r\n\
\tTester\tSam\t\tP\tZZ03\t2025\t3/14/2025\t8099003\r\n\
Hon.\tBroken\tRow\t\tP\tZZ04\tnot-a-year\t1/1/2025\t20099004\r\n\
Hon.\tBadId\tRow\t\tP\tZZ05\t2025\t1/1/2025\t../x\r\n\
\r\n";

    #[test]
    fn reads_the_index_and_picks_out_the_transaction_reports() {
        let (rows, skipped) = parse_house_index(INDEX).unwrap();
        assert_eq!((rows.len(), skipped), (3, 2));
        let ptrs: Vec<&HouseIndexRow> = rows.iter().filter(|r| r.is_ptr()).collect();
        assert_eq!(ptrs.len(), 2);
        let a = ptrs[0];
        assert_eq!(a.display_name(), "Hon. Alex B. Example");
        assert_eq!(a.state_district, "ZZ01");
        assert_eq!(a.filing_date, Date::from_ymd(2025, 9, 10));
        assert_eq!(
            a.ptr_pdf_url(),
            "https://disclosures-clerk.house.gov/public_disc/ptr-pdfs/2025/20099001.pdf"
        );
        // A paper-style seven-digit id is addressed the same way.
        assert_eq!(
            ptrs[1].ptr_pdf_url(),
            "https://disclosures-clerk.house.gov/public_disc/ptr-pdfs/2025/8099003.pdf"
        );
        assert_eq!(rows[1].display_name(), "Jordan Sample Jr.");
        assert!(!rows[1].is_ptr(), "a candidate's report is not a PTR");
    }

    /// A real yearly index, outside the repository (real reports are never
    /// committed): `OE_HOUSE_INDEX=<path to 2025FD.txt> cargo test -p oe-officials -- --ignored`
    #[test]
    #[ignore = "needs a downloaded index, run by hand"]
    fn a_real_index_reads_without_skipping_rows() {
        let path = std::env::var("OE_HOUSE_INDEX").expect("set OE_HOUSE_INDEX");
        let text = std::fs::read_to_string(path).unwrap();
        let (rows, skipped) = parse_house_index(&text).unwrap();
        let ptrs = rows.iter().filter(|r| r.is_ptr()).count();
        println!("{} rows, {skipped} skipped, {ptrs} PTRs", rows.len());
        assert_eq!(skipped, 0);
        assert!(ptrs > 0);
        // 2025: every PTR has a filing date; 15 rows of type W have none.
        assert!(rows
            .iter()
            .filter(|r| r.is_ptr())
            .all(|r| r.filing_date.is_some()));
    }

    #[test]
    fn urls_and_a_changed_header_are_caught() {
        assert_eq!(
            index_zip_url(2015),
            "https://disclosures-clerk.house.gov/public_disc/financial-pdfs/2015FD.zip"
        );
        assert_eq!(index_txt_name(2015), "2015FD.txt");
        assert!(parse_house_index("Name\tYear\n").is_err());
        assert!(parse_house_index("").is_err());
        assert!(USE_LIMITS.contains("commercial purpose"));
    }
}
