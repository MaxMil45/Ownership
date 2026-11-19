//! Turns a downloaded filing (its XML plus the `.meta.txt` sidecar) into the
//! rows of the `filing` and `insider_transaction` tables. No database here:
//! everything in this module is plain data and is tested offline.
//!
//! Decisions the schema forces, and how they are made:
//!
//! * `filing.filer_cik` is the **first reporting owner** (the person or entity
//!   that filed). The issuer's CIK is on each transaction row.
//! * `insider_transaction.owner_cik` holds one owner. On a joint filing the
//!   co-owners are not stored (the schema has no place for them yet).
//! * `accepted_at` needs a real time. If a sidecar has none, the filing is
//!   rejected rather than given an invented time.
//! * **Amendment rows are not loaded.** A `4/A` and a `5/A` get a `filing`
//!   row (the receipt) but no `insider_transaction` rows: the schema links an
//!   amendment to the row it replaces (`supersedes_id`), a 4/A does not name
//!   the filing it amends, and guessing the link would put both versions in
//!   `insider_transaction_as_of`. The number of rows held back is reported.

use crate::sha256::sha256_hex;
use oe_core::{AcquiredDisposed, OwnershipDocument, OwnershipNature};

/// The fields of a `.meta.txt` sidecar that the loader uses.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Sidecar {
    pub accession: Option<String>,
    pub form: Option<String>,
    /// UTC timestamp written by `oe recent-form4`, e.g. `2026-10-05T22:42:45.000Z`.
    pub accepted_utc: Option<String>,
    /// US Eastern timestamp written by `oe daily-form4` / `oe follow`,
    /// e.g. `2026-10-05T18:42:45`.
    pub accepted_eastern: Option<String>,
    pub url: Option<String>,
}

impl Sidecar {
    pub fn parse(text: &str) -> Sidecar {
        let mut s = Sidecar::default();
        for (key, value) in oe_core::text::key_values(text) {
            let value = Some(value.to_string());
            match key {
                "accession" => s.accession = value,
                "form" => s.form = value,
                "accepted" => s.accepted_utc = value,
                "accepted_eastern" => s.accepted_eastern = value,
                "url" => s.url = value,
                _ => {}
            }
        }
        s
    }

    /// The acceptance time as text PostgreSQL reads as a `timestamptz` with an
    /// explicit zone, so the server (not this code) applies daylight saving.
    pub fn accepted_at(&self) -> Option<String> {
        if let Some(utc) = &self.accepted_utc {
            let (date, rest) = utc.split_once('T')?;
            let time = rest.strip_suffix('Z')?;
            let time = time.split('.').next()?;
            return valid_clock(time).then(|| format!("{date} {time}+00"));
        }
        let (date, time) = self.accepted_eastern.as_deref()?.split_once('T')?;
        valid_clock(time).then(|| format!("{date} {time} America/New_York"))
    }
}

/// `HH:MM:SS` with plausible values.
fn valid_clock(t: &str) -> bool {
    let p: Vec<&str> = t.split(':').collect();
    p.len() == 3
        && p.iter()
            .all(|x| x.len() == 2 && x.bytes().all(|b| b.is_ascii_digit()))
        && p[0] < "24"
        && p[1] < "60"
        && p[2] < "61"
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FilingRow {
    pub accession: String,
    pub form_type: String,
    pub filer_cik: u64,
    /// Text for a `timestamptz` column, see [`Sidecar::accepted_at`].
    pub accepted_at: String,
    pub source_url: String,
    pub raw_sha256: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct TransactionRow {
    pub accession: String,
    /// Position among the document's transaction rows, starting at 0.
    pub row_in_filing: i32,
    pub issuer_cik: u64,
    pub owner_cik: u64,
    pub is_derivative: bool,
    pub transaction_date: String,
    pub code: String,
    pub shares: Option<f64>,
    pub price_per_share: Option<f64>,
    pub acquired_disposed: Option<char>,
    pub shares_after: Option<f64>,
    pub direct_indirect: Option<char>,
    pub plan_10b5_1: Option<bool>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Prepared {
    pub filing: FilingRow,
    pub transactions: Vec<TransactionRow>,
    /// Transaction rows of an amendment that were not loaded (see module docs).
    pub held_back_amendment_rows: usize,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PrepareError {
    Parse(String),
    NoAccession,
    /// The sidecar has no usable acceptance time.
    NoAcceptanceTime,
    /// The sidecar says one form, the XML another.
    FormMismatch {
        sidecar: String,
        xml: String,
    },
}

impl std::fmt::Display for PrepareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PrepareError::Parse(e) => write!(f, "the parser rejected the XML: {e}"),
            PrepareError::NoAccession => f.write_str("the sidecar has no accession number"),
            PrepareError::NoAcceptanceTime => f.write_str(
                "the sidecar has no readable acceptance time (accepted / accepted_eastern)",
            ),
            PrepareError::FormMismatch { sidecar, xml } => {
                write!(f, "sidecar says form {sidecar:?} but the XML says {xml:?}")
            }
        }
    }
}

impl std::error::Error for PrepareError {}

/// Builds the rows for one filing. `xml` is hashed exactly as given.
pub fn prepare(xml: &str, sidecar: &Sidecar) -> Result<Prepared, PrepareError> {
    let doc = oe_form4::parse(xml).map_err(|e| PrepareError::Parse(e.to_string()))?;
    let accession = sidecar.accession.clone().ok_or(PrepareError::NoAccession)?;
    let accepted_at = sidecar
        .accepted_at()
        .ok_or(PrepareError::NoAcceptanceTime)?;
    if let Some(form) = &sidecar.form {
        if *form != doc.document_type {
            return Err(PrepareError::FormMismatch {
                sidecar: form.clone(),
                xml: doc.document_type,
            });
        }
    }
    let Some(owner) = doc.owners.first() else {
        return Err(PrepareError::Parse("no reporting owner".into()));
    };

    let filing = FilingRow {
        accession: accession.clone(),
        form_type: doc.document_type.clone(),
        filer_cik: owner.cik.0,
        accepted_at,
        source_url: sidecar.url.clone().unwrap_or_else(|| {
            format!(
                "https://www.sec.gov/Archives/edgar/data/{}/{}.txt",
                doc.issuer.cik.0, accession
            )
        }),
        raw_sha256: sha256_hex(xml.as_bytes()),
    };

    if doc.is_amendment() {
        return Ok(Prepared {
            filing,
            transactions: Vec::new(),
            held_back_amendment_rows: doc.transactions.len(),
        });
    }
    let transactions = transaction_rows(&doc, &accession);
    Ok(Prepared {
        filing,
        transactions,
        held_back_amendment_rows: 0,
    })
}

fn transaction_rows(doc: &OwnershipDocument, accession: &str) -> Vec<TransactionRow> {
    let owner = doc.owners[0].cik.0;
    doc.transactions
        .iter()
        .enumerate()
        .map(|(i, t)| TransactionRow {
            accession: accession.to_string(),
            row_in_filing: i as i32,
            issuer_cik: doc.issuer.cik.0,
            owner_cik: owner,
            is_derivative: t.is_derivative,
            transaction_date: t.date.to_string(),
            code: t.code.as_str().to_string(),
            shares: t.shares,
            price_per_share: t.price_per_share,
            acquired_disposed: t.acquired_disposed.map(|a| match a {
                AcquiredDisposed::Acquired => 'A',
                AcquiredDisposed::Disposed => 'D',
            }),
            shares_after: t.shares_owned_after,
            direct_indirect: t.ownership.map(|o| match o {
                OwnershipNature::Direct => 'D',
                OwnershipNature::Indirect => 'I',
            }),
            plan_10b5_1: doc.aff_10b5_1,
        })
        .collect()
}

// ---------------------------------------------------------------------------
// COPY text format (the staging step of the load)

/// Escapes one value for PostgreSQL's `COPY ... FROM STDIN` text format.
pub fn copy_field(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '\\' => out.push_str("\\\\"),
            '\t' => out.push_str("\\t"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            c => out.push(c),
        }
    }
    out
}

fn opt_field<T: ToString>(v: Option<T>) -> String {
    v.map_or_else(|| "\\N".to_string(), |x| copy_field(&x.to_string()))
}

impl FilingRow {
    pub fn copy_line(&self) -> String {
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\n",
            copy_field(&self.accession),
            copy_field(&self.form_type),
            self.filer_cik,
            copy_field(&self.accepted_at),
            copy_field(&self.source_url),
            self.raw_sha256
        )
    }
}

impl TransactionRow {
    pub fn copy_line(&self) -> String {
        // Rust prints an f64 in plain decimal (never 1e6), which PostgreSQL's
        // numeric accepts as text.
        format!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
            copy_field(&self.accession),
            self.row_in_filing,
            self.issuer_cik,
            self.owner_cik,
            if self.is_derivative { "t" } else { "f" },
            self.transaction_date,
            copy_field(&self.code),
            opt_field(self.shares),
            opt_field(self.price_per_share),
            opt_field(self.acquired_disposed),
            opt_field(self.shares_after),
            opt_field(self.direct_indirect),
            opt_field(self.plan_10b5_1.map(|b| if b { "t" } else { "f" })),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn real(name: &str) -> (String, Sidecar) {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/form4/real");
        let xml = std::fs::read_to_string(dir.join(format!("{name}.xml"))).unwrap();
        let meta = std::fs::read_to_string(dir.join(format!("{name}.meta.txt"))).unwrap();
        (xml, Sidecar::parse(&meta))
    }

    #[test]
    fn sidecar_timestamps_become_zoned_text() {
        let utc = Sidecar::parse("accession=a\naccepted=2026-10-05T22:42:45.000Z\n");
        assert_eq!(utc.accepted_at().as_deref(), Some("2026-10-05 22:42:45+00"));
        let east = Sidecar::parse("accession=a\naccepted_eastern=2026-10-05T18:42:45\n");
        assert_eq!(
            east.accepted_at().as_deref(),
            Some("2026-10-05 18:42:45 America/New_York")
        );
        // UTC wins when both are present.
        let both = Sidecar::parse(
            "accepted=2026-10-05T22:42:45.000Z\naccepted_eastern=2026-10-05T18:42:45\n",
        );
        assert_eq!(
            both.accepted_at().as_deref(),
            Some("2026-10-05 22:42:45+00")
        );
        for bad in [
            "",
            "accepted=garbage",
            "accepted=2026-10-05T25:00:00.000Z",
            "accepted_eastern=2026-10-05",
        ] {
            assert_eq!(Sidecar::parse(bad).accepted_at(), None, "{bad:?}");
        }
    }

    #[test]
    fn a_plain_form4_becomes_one_filing_and_its_rows() {
        let (xml, side) = real("aapl_0001140361-26-044773");
        let p = prepare(&xml, &side).unwrap();
        assert_eq!(p.filing.accession, "0001140361-26-044773");
        assert_eq!(p.filing.form_type, "4");
        assert_eq!(p.filing.raw_sha256, sha256_hex(xml.as_bytes()));
        assert_eq!(p.transactions.len(), 3);
        assert_eq!(p.held_back_amendment_rows, 0);
        let rows: Vec<i32> = p.transactions.iter().map(|t| t.row_in_filing).collect();
        assert_eq!(rows, [0, 1, 2]);
        assert!(p.transactions.iter().all(|t| t.issuer_cik == 320193));
        assert!(p.transactions.iter().all(|t| t.plan_10b5_1 == Some(false)));
        let codes: Vec<&str> = p.transactions.iter().map(|t| t.code.as_str()).collect();
        assert!(codes.contains(&"G") && codes.contains(&"S"));
    }

    #[test]
    fn raw_hash_matches_an_independent_sha256_of_the_real_file() {
        // Expected value from PowerShell: (Get-FileHash <file> -Algorithm SHA256).
        let (xml, side) = real("msft_0000789019-26-000213");
        assert_eq!(
            prepare(&xml, &side).unwrap().filing.raw_sha256,
            "5c4bfce50b225532473dff3885188f2ab22e54180dbde07a7238e07289b7655e"
        );
    }

    #[test]
    fn amendments_keep_their_receipt_but_hold_back_the_rows() {
        let (xml, side) = real("nvda_0002152188-26-000007");
        let p = prepare(&xml, &side).unwrap();
        assert_eq!(p.filing.form_type, "4/A");
        assert!(p.transactions.is_empty());
        assert_eq!(p.held_back_amendment_rows, 1);
    }

    #[test]
    fn joint_filing_uses_the_first_owner_as_filer() {
        let (xml, side) = real("brk_0001193125-26-445930");
        let p = prepare(&xml, &side).unwrap();
        assert_eq!(p.filing.filer_cik, 1_067_983); // Berkshire Hathaway, listed first
        assert_eq!(p.transactions.len(), 3);
        assert!(p.transactions.iter().all(|t| t.owner_cik == 1_067_983));
    }

    #[test]
    fn form3_is_a_filing_without_rows() {
        let (xml, side) = real("oxy_0001628280-26-071244");
        let p = prepare(&xml, &side).unwrap();
        assert_eq!(p.filing.form_type, "3");
        assert!(p.transactions.is_empty() && p.held_back_amendment_rows == 0);
    }

    #[test]
    fn every_real_fixture_prepares_and_row_counts_match_the_manifest() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/form4/real");
        let manifest = std::fs::read_to_string(dir.join("MANIFEST.tsv")).unwrap();
        let (mut loaded, mut held) = (0usize, 0usize);
        for line in manifest.lines().skip(1) {
            let c: Vec<&str> = line.split('\t').collect();
            let rows: usize = c[3].parse::<usize>().unwrap() + c[4].parse::<usize>().unwrap();
            let (xml, side) = real(c[0].trim_end_matches(".xml"));
            let p = prepare(&xml, &side).unwrap_or_else(|e| panic!("{}: {e}", c[0]));
            if c[1].ends_with("/A") {
                assert_eq!(
                    (p.transactions.len(), p.held_back_amendment_rows),
                    (0, rows),
                    "{}",
                    c[0]
                );
                held += rows;
            } else {
                assert_eq!(p.transactions.len(), rows, "{}", c[0]);
                loaded += rows;
            }
        }
        // 84 transaction rows in the 24 fixtures (see `oe cluster-buys`), of which
        // the amendments hold 1 + 2 + 3 + 2 + 2 + ... counted from the manifest.
        assert_eq!(loaded + held, 84);
        assert!(held > 0 && loaded > held);
    }

    #[test]
    fn rejects_bad_sidecars() {
        let (xml, side) = real("msft_0000789019-26-000213");
        let no_time = Sidecar {
            accepted_utc: None,
            accepted_eastern: None,
            ..side.clone()
        };
        assert_eq!(prepare(&xml, &no_time), Err(PrepareError::NoAcceptanceTime));
        let no_acc = Sidecar {
            accession: None,
            ..side.clone()
        };
        assert_eq!(prepare(&xml, &no_acc), Err(PrepareError::NoAccession));
        let wrong = Sidecar {
            form: Some("5".into()),
            ..side
        };
        assert!(matches!(
            prepare(&xml, &wrong),
            Err(PrepareError::FormMismatch { .. })
        ));
        assert!(matches!(
            prepare("<html/>", &Sidecar::default()),
            Err(PrepareError::Parse(_))
        ));
    }

    #[test]
    fn copy_text_escapes_and_marks_nulls() {
        assert_eq!(copy_field("a\tb\\c\nd\re"), "a\\tb\\\\c\\nd\\re");
        let row = TransactionRow {
            accession: "0000000000-26-000001".into(),
            row_in_filing: 2,
            issuer_cik: 5,
            owner_cik: 6,
            is_derivative: true,
            transaction_date: "2026-03-02".into(),
            code: "S".into(),
            shares: Some(10000.0),
            price_per_share: Some(12.8125),
            acquired_disposed: Some('D'),
            shares_after: None,
            direct_indirect: Some('I'),
            plan_10b5_1: None,
        };
        assert_eq!(
            row.copy_line(),
            "0000000000-26-000001\t2\t5\t6\tt\t2026-03-02\tS\t10000\t12.8125\tD\t\\N\tI\t\\N\n"
        );
        // Large values stay plain decimals, never exponent notation.
        assert_eq!(opt_field(Some(96_000_000.0_f64)), "96000000");
        assert_eq!(opt_field(Some(0.0000001_f64)), "0.0000001");
    }

    #[test]
    fn filing_copy_line_has_six_tab_separated_fields() {
        let (xml, side) = real("msft_0000789019-26-000213");
        let line = prepare(&xml, &side).unwrap().filing.copy_line();
        assert_eq!(line.matches('\t').count(), 5);
        assert!(line.ends_with('\n'));
    }
}
