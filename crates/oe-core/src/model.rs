//! Models for insider ownership filings (Forms 3, 4 and 5).
//!
//! Field names follow the SEC "ownershipDocument" XML schema so the mapping
//! stays obvious. See `docs/field-guide/form-4.md`.

use crate::{Cik, Date};

/// Form 4 transaction codes (General Instructions, item 8).
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TransactionCode {
    /// Open market or private purchase.
    P,
    /// Open market or private sale.
    S,
    /// Grant or award from the company.
    A,
    /// Disposition to the issuer.
    D,
    /// Shares withheld to pay an exercise price or tax.
    F,
    /// Exercise or conversion of an exempt derivative.
    M,
    /// Conversion of a derivative security.
    C,
    /// Exercise of an in-the-money or at-the-money derivative.
    X,
    /// Bona fide gift.
    G,
    /// Other acquisition or disposition (described in a footnote).
    J,
    /// Any other code, kept verbatim.
    Other(String),
}

impl TransactionCode {
    pub fn parse(code: &str) -> TransactionCode {
        match code.trim() {
            "P" => TransactionCode::P,
            "S" => TransactionCode::S,
            "A" => TransactionCode::A,
            "D" => TransactionCode::D,
            "F" => TransactionCode::F,
            "M" => TransactionCode::M,
            "C" => TransactionCode::C,
            "X" => TransactionCode::X,
            "G" => TransactionCode::G,
            "J" => TransactionCode::J,
            other => TransactionCode::Other(other.to_string()),
        }
    }

    pub fn as_str(&self) -> &str {
        match self {
            TransactionCode::P => "P",
            TransactionCode::S => "S",
            TransactionCode::A => "A",
            TransactionCode::D => "D",
            TransactionCode::F => "F",
            TransactionCode::M => "M",
            TransactionCode::C => "C",
            TransactionCode::X => "X",
            TransactionCode::G => "G",
            TransactionCode::J => "J",
            TransactionCode::Other(s) => s,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AcquiredDisposed {
    Acquired,
    Disposed,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnershipNature {
    Direct,
    Indirect,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Relationship {
    pub is_director: bool,
    pub is_officer: bool,
    pub is_ten_percent_owner: bool,
    pub is_other: bool,
    pub officer_title: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportingOwner {
    pub cik: Cik,
    pub name: String,
    pub relationship: Relationship,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Issuer {
    pub cik: Cik,
    pub name: String,
    pub ticker: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Transaction {
    pub is_derivative: bool,
    pub security_title: String,
    pub date: Date,
    pub code: TransactionCode,
    pub shares: Option<f64>,
    /// `None` when the price is blank or given only as a footnote.
    pub price_per_share: Option<f64>,
    pub acquired_disposed: Option<AcquiredDisposed>,
    pub shares_owned_after: Option<f64>,
    pub ownership: Option<OwnershipNature>,
}

/// Dollar amounts above either limit are treated as filing errors as filed,
/// typically the total typed into the price field (40,000,000 shares "at
/// $525,600,000") or round placeholder numbers (10,000,000 shares at $1,000).
/// Berkshire Hathaway class A, the dearest US share, trades under $1,000,000.
/// Measured on the 2015-2026 Forms 4 (open-market rows): 376 over $10 billion;
/// between $2 and $10 billion almost all were round numbers or came from two
/// filer agents (one penny stock got 198 such rows, another agent 191 rows
/// over 108 issuers); 139 other rows were over $2 billion, most of them
/// errors too, a few real block trades. Those few are lost from the totals.
/// Such rows stay in the ledger; they are only left out of dollar totals.
pub const MAX_PLAUSIBLE_PRICE: f64 = 2_000_000.0;
pub const MAX_PLAUSIBLE_ROW_VALUE: f64 = 2_000_000_000.0;
/// One issuer's open-market buying or selling on one day above this is
/// treated as a filing error even when every row passes: Notis Global, a
/// penny stock, got $37.5 and $20.2 billion of "purchases" on 17-18 November
/// 2016 from one filer. Real large days measured 2015-2026: Tesla $6.9 billion
/// (2022-08-09), a fund's block sales of $6-9 billion; one $12.1 billion day
/// (likely related funds each reporting the same shares) is lost.
pub const MAX_PLAUSIBLE_ISSUER_DAY: f64 = 10_000_000_000.0;

/// Whether a row's price and value are believable (see [`MAX_PLAUSIBLE_PRICE`]).
/// Missing numbers are not implausible.
pub fn plausible_amounts(price_per_share: Option<f64>, value: Option<f64>) -> bool {
    price_per_share.map_or(true, |p| p <= MAX_PLAUSIBLE_PRICE)
        && value.map_or(true, |v| v <= MAX_PLAUSIBLE_ROW_VALUE)
}

impl Transaction {
    /// Open-market purchase of non-derivative stock: the classic insider "buy".
    pub fn is_open_market_purchase(&self) -> bool {
        !self.is_derivative
            && self.code == TransactionCode::P
            && self.acquired_disposed != Some(AcquiredDisposed::Disposed)
    }

    pub fn value(&self) -> Option<f64> {
        Some(self.shares? * self.price_per_share?)
    }
}

/// One parsed ownership document (Form 3, 4, 5 or an amendment).
#[derive(Debug, Clone, PartialEq)]
pub struct OwnershipDocument {
    /// "3", "4", "5", "4/A", ...
    pub document_type: String,
    pub period_of_report: Option<Date>,
    /// On an amendment: the filing date of the report it amends
    /// (`dateOfOriginalSubmission`). The XML names no accession number.
    pub date_of_original_submission: Option<Date>,
    pub issuer: Issuer,
    /// More than one owner means a joint filing.
    pub owners: Vec<ReportingOwner>,
    /// The Rule 10b5-1 checkbox (added in 2023). `None` on older filings.
    pub aff_10b5_1: Option<bool>,
    pub not_subject_to_section16: Option<bool>,
    pub transactions: Vec<Transaction>,
}

impl OwnershipDocument {
    pub fn is_amendment(&self) -> bool {
        self.document_type.ends_with("/A")
    }
}

/// A filer as EDGAR names it: a CIK and a conformed name.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Party {
    pub cik: Cik,
    pub name: String,
}

/// Where and when a document became public. Kept apart from the document
/// itself because the XML does not contain it: it comes from the EDGAR index.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FilingMeta {
    pub accession: Option<crate::Accession>,
    /// Date the SEC accepted the filing: the earliest the public could know.
    pub known_on: Option<Date>,
    pub source: Option<String>,
}

#[cfg(test)]
mod plausibility_tests {
    use super::*;

    #[test]
    fn typed_totals_are_implausible_and_berkshire_is_not() {
        // A real filing error: 40,000,000 shares "at $525,600,000" a share.
        assert!(!plausible_amounts(Some(525_600_000.0), Some(2.1e16)));
        // Berkshire Hathaway class A at about $750,000 a share.
        assert!(plausible_amounts(Some(750_000.0), Some(75_000_000.0)));
        assert!(!plausible_amounts(Some(20.0), Some(2e10)));
        // 10,000,000 shares at $1,000: a round placeholder, $10 billion.
        assert!(!plausible_amounts(Some(1_000.0), Some(1e10)));
        assert!(plausible_amounts(Some(45.0), Some(1.5e9)));
        assert!(plausible_amounts(None, None));
    }
}
