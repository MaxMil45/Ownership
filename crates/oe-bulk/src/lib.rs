//! The SEC's quarterly **Insider Transactions Data Sets**: every Form 3/4/5
//! since 2006 as tab-separated tables, one zip per quarter (ADR 0008).
//!
//! `https://www.sec.gov/data-research/sec-markets-data/insider-transactions-data-sets`
//!
//! This crate turns one quarter's zip into the same [`OwnershipDocument`]s the
//! XML parser produces, so the ledger, the rules and the dashboard treat both
//! sources alike. Checked on 199 random filings of 2026 Q3 against their own
//! XML: same owners, same rows in the same order, same values once rounded to
//! two decimals (the data sets store numbers as `NUMBER(16,2)`), and a filing
//! date equal to the US Eastern acceptance date in every case.
//!
//! What the data sets do **not** have: the acceptance time (only the filing
//! date), and numbers beyond two decimals. Owner street addresses are in the
//! files; this crate never reads those columns.

pub mod zip;

use oe_core::{
    AcquiredDisposed, Cik, Date, Issuer, OwnershipDocument, OwnershipNature, Relationship,
    ReportingOwner, Transaction, TransactionCode,
};
use std::collections::HashMap;
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BulkError(pub String);

impl fmt::Display for BulkError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BulkError {}

impl From<zip::ZipError> for BulkError {
    fn from(e: zip::ZipError) -> Self {
        BulkError(e.to_string())
    }
}

// ---------------------------------------------------------------------------
// Quarters and where they are

/// A calendar quarter, written `2026q3`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Quarter {
    pub year: i32,
    pub q: u8,
}

impl Quarter {
    pub fn next(self) -> Quarter {
        if self.q == 4 {
            Quarter {
                year: self.year + 1,
                q: 1,
            }
        } else {
            Quarter {
                year: self.year,
                q: self.q + 1,
            }
        }
    }

    /// The file name the SEC uses, `2026q3_form345.zip`.
    pub fn file_name(self) -> String {
        format!("{self}_form345.zip")
    }
}

impl fmt::Display for Quarter {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}q{}", self.year, self.q)
    }
}

impl std::str::FromStr for Quarter {
    type Err = BulkError;
    fn from_str(s: &str) -> Result<Quarter, BulkError> {
        let bad = || BulkError(format!("{s:?} is not a quarter such as 2026q3"));
        let (y, q) = s
            .trim()
            .to_ascii_lowercase()
            .split_once('q')
            .map(|(a, b)| (a.to_string(), b.to_string()))
            .ok_or_else(bad)?;
        let year: i32 = y.parse().map_err(|_| bad())?;
        let q: u8 = q.parse().map_err(|_| bad())?;
        if !(1..=4).contains(&q) || !(1990..=2200).contains(&year) {
            return Err(bad());
        }
        Ok(Quarter { year, q })
    }
}

/// The page that lists the quarterly zips.
pub const DATA_SETS_PAGE: &str =
    "https://www.sec.gov/data-research/sec-markets-data/insider-transactions-data-sets";

/// Every `<quarter>_form345.zip` link on the data-set page, as full URLs,
/// oldest first. The folder is read from the page because it changed in 2026
/// (`structureddata` until 2026 Q1, `datastandardsinnovation` after).
pub fn parse_data_set_links(html: &str) -> Vec<(Quarter, String)> {
    let mut out: Vec<(Quarter, String)> = Vec::new();
    for part in html.split("href=\"").skip(1) {
        let Some(href) = part.split('"').next() else {
            continue;
        };
        let Some(file) = href.rsplit('/').next() else {
            continue;
        };
        let Some(q) = file.strip_suffix("_form345.zip") else {
            continue;
        };
        let Ok(q) = q.parse::<Quarter>() else {
            continue;
        };
        let url = if href.starts_with("https://") {
            href.to_string()
        } else if href.starts_with('/') {
            format!("https://www.sec.gov{href}")
        } else {
            continue;
        };
        if !out.iter().any(|(x, _)| *x == q) {
            out.push((q, url));
        }
    }
    out.sort();
    out
}

// ---------------------------------------------------------------------------
// Tables

/// One tab-separated table with a header row. The SEC files have no quoting:
/// every row has the header's number of fields and quotes are literal.
pub struct Table {
    columns: HashMap<String, usize>,
    pub rows: Vec<Vec<String>>,
}

impl Table {
    pub fn parse(text: &str) -> Result<Table, BulkError> {
        let mut lines = text.lines();
        let header = lines
            .next()
            .ok_or_else(|| BulkError("empty table".into()))?;
        let columns: HashMap<String, usize> = header
            .trim_start_matches('\u{feff}')
            .split('\t')
            .enumerate()
            .map(|(i, h)| (h.trim().to_string(), i))
            .collect();
        let width = columns.len();
        let mut rows = Vec::new();
        for (n, line) in lines.enumerate() {
            if line.is_empty() {
                continue;
            }
            let row: Vec<String> = line.split('\t').map(unquote).collect();
            if row.len() != width {
                return Err(BulkError(format!(
                    "row {} has {} fields, the header {width}",
                    n + 2,
                    row.len()
                )));
            }
            rows.push(row);
        }
        Ok(Table { columns, rows })
    }

    /// The column's index, or an error naming it (older quarters lack some).
    pub fn col(&self, name: &str) -> Result<usize, BulkError> {
        self.columns
            .get(name)
            .copied()
            .ok_or_else(|| BulkError(format!("column {name} is missing")))
    }

    /// The column's index if the table has it.
    pub fn opt_col(&self, name: &str) -> Option<usize> {
        self.columns.get(name).copied()
    }
}

/// A field the SEC wrote CSV-style: `"say ""A"" instead"` → `say "A" instead`.
/// Seen in REMARKS of 2026 Q3. Fields are never split across lines or tabs
/// (every row has the header's width), so only the quotes need undoing.
/// A field that merely contains a quote is kept as it is.
fn unquote(field: &str) -> String {
    match field.strip_prefix('"').and_then(|f| f.strip_suffix('"')) {
        Some(inner) if field.len() >= 2 => inner.replace("\"\"", "\""),
        _ => field.to_string(),
    }
}

/// `28-SEP-2026` → 2026-09-28.
pub fn parse_date(s: &str) -> Option<Date> {
    const M: [&str; 12] = [
        "JAN", "FEB", "MAR", "APR", "MAY", "JUN", "JUL", "AUG", "SEP", "OCT", "NOV", "DEC",
    ];
    let mut p = s.trim().split('-');
    let d: u32 = p.next()?.parse().ok()?;
    let month = p.next()?;
    let m = M.iter().position(|x| x.eq_ignore_ascii_case(month))? as u32 + 1;
    let y: i32 = p.next()?.parse().ok()?;
    if p.next().is_some() {
        return None;
    }
    Date::from_ymd(y, m, d)
}

fn number(s: &str) -> Option<f64> {
    oe_core::text::parse_decimal(s)
}

fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

// ---------------------------------------------------------------------------
// Documents

/// What a bulk row knows about its filing besides the document.
#[derive(Debug, Clone, PartialEq)]
pub struct BulkFiling {
    pub accession: String,
    /// The SEC filing date: the US Eastern date it was accepted (checked on
    /// 199 filings), except for filings accepted after 10 pm, which get the
    /// next business day.
    pub filing_date: Date,
    pub document: OwnershipDocument,
}

/// What reading a quarter skipped, so nothing is dropped silently.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Skipped {
    /// Submissions without a readable filing date, issuer CIK or owner.
    pub filings: Vec<(String, String)>,
    /// Transaction rows without a readable date, or for an unknown accession.
    pub rows: usize,
}

/// Reads one quarter's zip into documents, in SUBMISSION order. Transactions
/// keep the filing's order: Table I (non-derivative) rows by their surrogate
/// key, then Table II rows, which is the order of the XML.
pub fn read_quarter(zip_bytes: &[u8]) -> Result<(Vec<BulkFiling>, Skipped), BulkError> {
    let a = zip::Archive::open(zip_bytes)?;
    let table = |name: &str| -> Result<Table, BulkError> {
        let e = a
            .find(name)
            .ok_or_else(|| BulkError(format!("{name} is not in the archive")))?;
        let bytes = a.read(e)?;
        Table::parse(&String::from_utf8_lossy(&bytes))
            .map_err(|x| BulkError(format!("{name}: {x}")))
    };
    let sub = table("SUBMISSION.tsv")?;
    let own = table("REPORTINGOWNER.tsv")?;
    let ntx = table("NONDERIV_TRANS.tsv")?;
    let dtx = table("DERIV_TRANS.tsv")?;
    build(&sub, &own, &ntx, &dtx)
}

/// The same as [`read_quarter`], from tables already read (for tests).
pub fn build(
    sub: &Table,
    own: &Table,
    ntx: &Table,
    dtx: &Table,
) -> Result<(Vec<BulkFiling>, Skipped), BulkError> {
    let mut skipped = Skipped::default();

    // Owners, in file order (the first is the filer, as in the XML).
    let (oa, ocik, oname, orel, otitle) = (
        own.col("ACCESSION_NUMBER")?,
        own.col("RPTOWNERCIK")?,
        own.col("RPTOWNERNAME")?,
        own.col("RPTOWNER_RELATIONSHIP")?,
        own.opt_col("RPTOWNER_TITLE"),
    );
    let mut owners: HashMap<&str, Vec<ReportingOwner>> = HashMap::new();
    for r in &own.rows {
        let Ok(cik) = r[ocik].parse::<Cik>() else {
            continue;
        };
        let rel = r[orel].to_ascii_lowercase();
        let has = |w: &str| rel.split(',').any(|x| x.trim() == w);
        owners
            .entry(r[oa].as_str())
            .or_default()
            .push(ReportingOwner {
                cik,
                name: r[oname].trim().to_string(),
                relationship: Relationship {
                    is_director: has("director"),
                    is_officer: has("officer"),
                    is_ten_percent_owner: has("tenpercentowner"),
                    is_other: has("other"),
                    officer_title: otitle.and_then(|c| non_empty(&r[c])),
                },
            });
    }

    let mut txns: HashMap<&str, Vec<(bool, u64, Transaction)>> = HashMap::new();
    for (t, derivative) in [(ntx, false), (dtx, true)] {
        let (acc, sk_name) = (
            t.col("ACCESSION_NUMBER")?,
            if derivative {
                "DERIV_TRANS_SK"
            } else {
                "NONDERIV_TRANS_SK"
            },
        );
        let (sk, title, date, code, shares, price, ad, after, di) = (
            t.col(sk_name)?,
            t.col("SECURITY_TITLE")?,
            t.col("TRANS_DATE")?,
            t.col("TRANS_CODE")?,
            t.col("TRANS_SHARES")?,
            t.col("TRANS_PRICEPERSHARE")?,
            t.col("TRANS_ACQUIRED_DISP_CD")?,
            t.col("SHRS_OWND_FOLWNG_TRANS")?,
            t.col("DIRECT_INDIRECT_OWNERSHIP")?,
        );
        for r in &t.rows {
            let Some(d) = parse_date(&r[date]) else {
                skipped.rows += 1;
                continue;
            };
            txns.entry(r[acc].as_str()).or_default().push((
                derivative,
                r[sk].trim().parse().unwrap_or(u64::MAX),
                Transaction {
                    is_derivative: derivative,
                    security_title: r[title].trim().to_string(),
                    date: d,
                    code: TransactionCode::parse(r[code].trim()),
                    shares: number(&r[shares]),
                    price_per_share: number(&r[price]),
                    acquired_disposed: match r[ad].trim() {
                        "A" => Some(AcquiredDisposed::Acquired),
                        "D" => Some(AcquiredDisposed::Disposed),
                        _ => None,
                    },
                    shares_owned_after: number(&r[after]),
                    ownership: match r[di].trim() {
                        "D" => Some(OwnershipNature::Direct),
                        "I" => Some(OwnershipNature::Indirect),
                        _ => None,
                    },
                },
            ));
        }
    }

    let c = |n: &str| sub.col(n);
    let (sa, sfd, spr, sorig, stype, sicik, siname, sitick) = (
        c("ACCESSION_NUMBER")?,
        c("FILING_DATE")?,
        c("PERIOD_OF_REPORT")?,
        c("DATE_OF_ORIG_SUB")?,
        c("DOCUMENT_TYPE")?,
        c("ISSUERCIK")?,
        c("ISSUERNAME")?,
        c("ISSUERTRADINGSYMBOL")?,
    );
    let (saff, snot16) = (sub.opt_col("AFF10B5ONE"), sub.opt_col("NOT_SUBJECT_SEC16"));

    let mut out = Vec::with_capacity(sub.rows.len());
    let mut known = std::collections::HashSet::new();
    for r in &sub.rows {
        let acc = r[sa].trim();
        let fail = |why: &str, skipped: &mut Skipped| {
            skipped.filings.push((acc.to_string(), why.to_string()))
        };
        let Some(filing_date) = parse_date(&r[sfd]) else {
            fail("no readable FILING_DATE", &mut skipped);
            continue;
        };
        let Ok(issuer_cik) = r[sicik].parse::<Cik>() else {
            fail("no readable ISSUERCIK", &mut skipped);
            continue;
        };
        let Some(owner_list) = owners.remove(acc) else {
            fail("no reporting owner", &mut skipped);
            continue;
        };
        known.insert(acc.to_string());
        let mut rows = txns.remove(acc).unwrap_or_default();
        rows.sort_by_key(|(d, sk, _)| (*d, *sk));
        out.push(BulkFiling {
            accession: acc.to_string(),
            filing_date,
            document: OwnershipDocument {
                document_type: r[stype].trim().to_string(),
                period_of_report: parse_date(&r[spr]),
                date_of_original_submission: parse_date(&r[sorig]),
                issuer: Issuer {
                    cik: issuer_cik,
                    name: r[siname].trim().to_string(),
                    ticker: non_empty(&r[sitick]),
                },
                owners: owner_list,
                aff_10b5_1: saff.and_then(|i| oe_core::text::parse_flag(&r[i])),
                not_subject_to_section16: snot16.and_then(|i| oe_core::text::parse_flag(&r[i])),
                transactions: rows.into_iter().map(|(_, _, t)| t).collect(),
            },
        });
    }
    // Rows whose accession has no usable submission.
    skipped.rows += txns.values().map(Vec::len).sum::<usize>();
    Ok((out, skipped))
}

/// A stable fingerprint text of one bulk filing, for `filing.raw_sha256`:
/// the fields loaded, one per line. The same rows always give the same text.
pub fn canonical_text(f: &BulkFiling) -> String {
    let d = &f.document;
    let mut s = format!(
        "bulk\n{}\n{}\n{}\n{}\n{:?}\n",
        f.accession, f.filing_date, d.document_type, d.issuer.cik, d.date_of_original_submission
    );
    for o in &d.owners {
        s.push_str(&format!("owner\t{}\n", o.cik));
    }
    for t in &d.transactions {
        s.push_str(&format!(
            "row\t{}\t{}\t{}\t{:?}\t{:?}\t{:?}\t{:?}\t{:?}\n",
            t.is_derivative,
            t.date,
            t.code.as_str(),
            t.shares,
            t.price_per_share,
            t.acquired_disposed,
            t.shares_owned_after,
            t.ownership
        ));
    }
    s
}

#[cfg(test)]
mod tests {
    use super::*;

    const SUB: &str = "ACCESSION_NUMBER\tFILING_DATE\tPERIOD_OF_REPORT\tDATE_OF_ORIG_SUB\tNO_SECURITIES_OWNED\tNOT_SUBJECT_SEC16\tFORM3_HOLDINGS_REPORTED\tFORM4_TRANS_REPORTED\tDOCUMENT_TYPE\tISSUERCIK\tISSUERNAME\tISSUERTRADINGSYMBOL\tREMARKS\tAFF10B5ONE\n\
0009999901-26-000001\t31-JUL-2026\t29-JUL-2026\t\t\t0\t\t\t4\t0009999901\tSynthetic Widgets Inc\tSYNW\t\"quoted\" remark\t1\n\
0009999901-26-000002\t05-AUG-2026\t29-JUL-2026\t31-JUL-2026\t\t\t\t\t4/A\t0009999901\tSynthetic Widgets Inc\tSYNW\t\t0\n\
0009999901-26-000003\tbad-date\t29-JUL-2026\t\t\t\t\t\t4\t0009999901\tSynthetic Widgets Inc\tSYNW\t\t0\n";
    // The address columns are present, as in the real files, and ignored.
    const OWN: &str = "ACCESSION_NUMBER\tRPTOWNERCIK\tRPTOWNERNAME\tRPTOWNER_RELATIONSHIP\tRPTOWNER_TITLE\tRPTOWNER_TXT\tRPTOWNER_STREET1\tRPTOWNER_STREET2\tRPTOWNER_CITY\tRPTOWNER_STATE\tRPTOWNER_ZIPCODE\tRPTOWNER_STATE_DESC\tFILE_NUMBER\n\
0009999901-26-000001\t0009999911\tInsider, Alpha\tDirector,Officer\tChief Executive Officer\t\t1 Synthetic Way\t\tTestville\tZZ\t00000\t\t\n\
0009999901-26-000001\t0009999912\tAlpha Holdings LLC\tTenPercentOwner\t\t\t1 Synthetic Way\t\tTestville\tZZ\t00000\t\t\n\
0009999901-26-000002\t0009999911\tInsider, Alpha\tDirector\t\t\t1 Synthetic Way\t\tTestville\tZZ\t00000\t\t\n\
0009999901-26-000003\t0009999911\tInsider, Alpha\tDirector\t\t\t1 Synthetic Way\t\tTestville\tZZ\t00000\t\t\n";
    const NTX: &str = "ACCESSION_NUMBER\tNONDERIV_TRANS_SK\tSECURITY_TITLE\tTRANS_DATE\tTRANS_CODE\tTRANS_SHARES\tTRANS_PRICEPERSHARE\tTRANS_ACQUIRED_DISP_CD\tSHRS_OWND_FOLWNG_TRANS\tDIRECT_INDIRECT_OWNERSHIP\n\
0009999901-26-000001\t20\tCommon Stock\t29-JUL-2026\tS\t500.0\t10.25\tD\t9500.0\tD\n\
0009999901-26-000001\t10\tCommon Stock\t28-JUL-2026\tP\t1,000.0\t9.5\tA\t10000.0\tI\n\
0009999901-26-000002\t30\tCommon Stock\t29-JUL-2026\t\t500.0\t10.25\tD\t9500.0\tD\n\
0009999901-26-000404\t40\tCommon Stock\t29-JUL-2026\tP\t1.0\t1.0\tA\t1.0\tD\n";
    const DTX: &str = "ACCESSION_NUMBER\tDERIV_TRANS_SK\tSECURITY_TITLE\tTRANS_DATE\tTRANS_CODE\tTRANS_SHARES\tTRANS_PRICEPERSHARE\tTRANS_ACQUIRED_DISP_CD\tSHRS_OWND_FOLWNG_TRANS\tDIRECT_INDIRECT_OWNERSHIP\n\
0009999901-26-000001\t5\tStock Option\t28-JUL-2026\tM\t1000.0\t0\tD\t0.0\tD\n";

    fn tables() -> (Table, Table, Table, Table) {
        (
            Table::parse(SUB).unwrap(),
            Table::parse(OWN).unwrap(),
            Table::parse(NTX).unwrap(),
            Table::parse(DTX).unwrap(),
        )
    }

    #[test]
    fn builds_documents_like_the_xml_parser() {
        let (s, o, n, d) = tables();
        let (docs, skipped) = build(&s, &o, &n, &d).unwrap();
        assert_eq!(docs.len(), 2);
        let f = &docs[0];
        assert_eq!(f.accession, "0009999901-26-000001");
        assert_eq!(f.filing_date.to_string(), "2026-07-31");
        let doc = &f.document;
        assert_eq!(doc.document_type, "4");
        assert_eq!(doc.issuer.ticker.as_deref(), Some("SYNW"));
        assert_eq!(doc.aff_10b5_1, Some(true));
        assert_eq!(doc.not_subject_to_section16, Some(false));
        // Joint filing: owners in file order, relationships read from the list.
        assert_eq!(doc.owners.len(), 2);
        assert_eq!(doc.owners[0].cik, Cik(9_999_911));
        assert!(doc.owners[0].relationship.is_director && doc.owners[0].relationship.is_officer);
        assert_eq!(
            doc.owners[0].relationship.officer_title.as_deref(),
            Some("Chief Executive Officer")
        );
        assert!(doc.owners[1].relationship.is_ten_percent_owner);
        // Table I by surrogate key, then Table II.
        let rows: Vec<(bool, &str, Option<f64>)> = doc
            .transactions
            .iter()
            .map(|t| (t.is_derivative, t.code.as_str(), t.shares))
            .collect();
        assert_eq!(
            rows,
            [
                (false, "P", Some(1000.0)),
                (false, "S", Some(500.0)),
                (true, "M", Some(1000.0))
            ]
        );
        assert!(doc.transactions[0].is_open_market_purchase());
        assert_eq!(
            doc.transactions[0].ownership,
            Some(OwnershipNature::Indirect)
        );

        // The amendment names its original's date; its row has no code.
        let a = &docs[1].document;
        assert!(a.is_amendment());
        assert_eq!(
            a.date_of_original_submission
                .map(|d| d.to_string())
                .as_deref(),
            Some("2026-07-31")
        );
        assert_eq!(a.transactions[0].code.as_str(), "");

        // A bad filing date is reported, and so is a row with no submission.
        assert_eq!(skipped.filings.len(), 1);
        assert_eq!(skipped.filings[0].0, "0009999901-26-000003");
        assert_eq!(skipped.rows, 1);
    }

    #[test]
    fn reads_a_quarter_from_a_zip() {
        let z = crate::zip::tests::make_zip(&[
            ("SUBMISSION.tsv", SUB.as_bytes(), true),
            ("REPORTINGOWNER.tsv", OWN.as_bytes(), true),
            ("NONDERIV_TRANS.tsv", NTX.as_bytes(), true),
            ("DERIV_TRANS.tsv", DTX.as_bytes(), false),
        ]);
        let (docs, _) = read_quarter(&z).unwrap();
        assert_eq!(docs.len(), 2);
        let missing = crate::zip::tests::make_zip(&[("SUBMISSION.tsv", SUB.as_bytes(), false)]);
        assert!(read_quarter(&missing)
            .unwrap_err()
            .0
            .contains("REPORTINGOWNER"));
    }

    #[test]
    fn canonical_text_is_stable_and_changes_with_the_data() {
        let (s, o, n, d) = tables();
        let (a, _) = build(&s, &o, &n, &d).unwrap();
        let (b, _) = build(&s, &o, &n, &d).unwrap();
        assert_eq!(canonical_text(&a[0]), canonical_text(&b[0]));
        let mut c = a[0].clone();
        c.document.transactions[0].shares = Some(1001.0);
        assert_ne!(canonical_text(&a[0]), canonical_text(&c));
        assert!(
            !canonical_text(&a[0]).contains("Synthetic Way"),
            "no addresses"
        );
    }

    #[test]
    fn dates_quarters_and_tables() {
        assert_eq!(parse_date("28-SEP-2026").unwrap().to_string(), "2026-09-28");
        assert_eq!(parse_date("01-jan-2015").unwrap().to_string(), "2015-01-01");
        assert!(parse_date("2026-09-28").is_none());
        assert!(parse_date("31-FEB-2026").is_none());
        let q: Quarter = "2026Q3".parse().unwrap();
        assert_eq!(q.to_string(), "2026q3");
        assert_eq!(q.next().to_string(), "2026q4");
        assert_eq!(q.next().next().to_string(), "2027q1");
        assert_eq!(q.file_name(), "2026q3_form345.zip");
        assert!("2026q5".parse::<Quarter>().is_err());
        assert!(Table::parse("A\tB\n1\n").is_err());
    }

    #[test]
    fn csv_style_quoting_is_undone() {
        assert_eq!(
            unquote(r#""correct the code, ""A"" instead of ""P""""#),
            r#"correct the code, "A" instead of "P""#
        );
        assert_eq!(unquote("Smith \"Jr\""), "Smith \"Jr\"");
        assert_eq!(unquote("\""), "\"");
        assert_eq!(unquote("\"\""), "");
        assert_eq!(unquote("plain"), "plain");
    }

    #[test]
    fn finds_quarter_links_in_both_folders() {
        let html = r#"<a href="/files/datastandardsinnovation/data/insider-transactions-data-sets/2026q3_form345.zip">x</a>
            <a href="/files/structureddata/data/insider-transactions-data-sets/2026q1_form345.zip">y</a>
            <a href="/files/insider_transactions_readme.pdf">readme</a>
            <a href="/files/structureddata/data/insider-transactions-data-sets/2026q1_form345.zip">again</a>"#;
        let links = parse_data_set_links(html);
        assert_eq!(links.len(), 2);
        assert_eq!(links[0].0.to_string(), "2026q1");
        assert_eq!(
            links[1].1,
            "https://www.sec.gov/files/datastandardsinnovation/data/insider-transactions-data-sets/2026q3_form345.zip"
        );
    }
}
