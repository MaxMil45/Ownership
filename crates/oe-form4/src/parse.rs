//! Turns ownership-document tokens into [`OwnershipDocument`] values.
//!
//! The parser walks the token stream with a stack of open element names and
//! matches on the end of that path, so unknown elements are skipped safely
//! and footnote references inside values are ignored.

use crate::xml::{tokenize, Token, XmlError};
use oe_core::text::{parse_decimal, parse_flag as flag};
use oe_core::{
    AcquiredDisposed, Cik, Date, Issuer, OwnershipDocument, OwnershipNature, Relationship,
    ReportingOwner, Transaction, TransactionCode,
};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    Xml(XmlError),
    NotAnOwnershipDocument,
    Missing(&'static str),
    Invalid { field: &'static str, value: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Xml(e) => write!(f, "{e}"),
            ParseError::NotAnOwnershipDocument => {
                f.write_str("root element is not <ownershipDocument>")
            }
            ParseError::Missing(field) => write!(f, "missing required field: {field}"),
            ParseError::Invalid { field, value } => write!(f, "invalid {field}: {value:?}"),
        }
    }
}

impl std::error::Error for ParseError {}

impl From<XmlError> for ParseError {
    fn from(e: XmlError) -> Self {
        ParseError::Xml(e)
    }
}

#[derive(Default)]
struct OwnerBuilder {
    cik: Option<String>,
    name: Option<String>,
    relationship: Relationship,
}

#[derive(Default)]
struct TxnBuilder {
    is_derivative: bool,
    security_title: Option<String>,
    date: Option<String>,
    code: Option<String>,
    shares: Option<String>,
    price: Option<String>,
    acquired_disposed: Option<String>,
    shares_after: Option<String>,
    ownership: Option<String>,
}

#[derive(Default)]
struct DocBuilder {
    document_type: Option<String>,
    period_of_report: Option<String>,
    date_of_original_submission: Option<String>,
    aff_10b5_1: Option<String>,
    not_subject_to_section16: Option<String>,
    issuer_cik: Option<String>,
    issuer_name: Option<String>,
    issuer_ticker: Option<String>,
    owners: Vec<OwnerBuilder>,
    transactions: Vec<TxnBuilder>,
}

/// Parses one ownership document (the `.xml` file of a Form 3, 4 or 5).
pub fn parse(xml: &str) -> Result<OwnershipDocument, ParseError> {
    let tokens = tokenize(xml)?;
    if !matches!(tokens.first(), Some(Token::Start("ownershipDocument"))) {
        return Err(ParseError::NotAnOwnershipDocument);
    }

    let mut doc = DocBuilder::default();
    let mut stack: Vec<&str> = Vec::new();
    let mut owner: Option<OwnerBuilder> = None;
    let mut txn: Option<TxnBuilder> = None;

    for token in &tokens {
        match token {
            Token::Start(name) => {
                stack.push(name);
                match *name {
                    "reportingOwner" => owner = Some(OwnerBuilder::default()),
                    "nonDerivativeTransaction" => txn = Some(TxnBuilder::default()),
                    "derivativeTransaction" => {
                        txn = Some(TxnBuilder {
                            is_derivative: true,
                            ..TxnBuilder::default()
                        })
                    }
                    _ => {}
                }
            }
            Token::End(name) => {
                stack.pop();
                match *name {
                    "reportingOwner" => doc.owners.extend(owner.take()),
                    "nonDerivativeTransaction" | "derivativeTransaction" => {
                        doc.transactions.extend(txn.take())
                    }
                    _ => {}
                }
            }
            Token::Empty(_) => {}
            Token::Text(text) => {
                let value = Some(text.clone());
                if let Some(t) = txn.as_mut() {
                    assign_txn(t, &stack, value);
                } else if let Some(o) = owner.as_mut() {
                    assign_owner(o, &stack, text);
                } else {
                    assign_doc(&mut doc, &stack, value);
                }
            }
        }
    }

    build(doc)
}

fn ends_with(stack: &[&str], suffix: &[&str]) -> bool {
    stack.len() >= suffix.len() && stack[stack.len() - suffix.len()..] == *suffix
}

fn assign_doc(doc: &mut DocBuilder, stack: &[&str], value: Option<String>) {
    // Top-level fields sit directly under the root.
    if stack.len() == 2 {
        match stack[1] {
            "documentType" => doc.document_type = value,
            "periodOfReport" => doc.period_of_report = value,
            "dateOfOriginalSubmission" => doc.date_of_original_submission = value,
            "aff10b5One" => doc.aff_10b5_1 = value,
            "notSubjectToSection16" => doc.not_subject_to_section16 = value,
            _ => {}
        }
    } else if ends_with(stack, &["issuer", "issuerCik"]) {
        doc.issuer_cik = value;
    } else if ends_with(stack, &["issuer", "issuerName"]) {
        doc.issuer_name = value;
    } else if ends_with(stack, &["issuer", "issuerTradingSymbol"]) {
        doc.issuer_ticker = value;
    }
}

fn assign_owner(o: &mut OwnerBuilder, stack: &[&str], text: &str) {
    let rel = &mut o.relationship;
    let last = stack.last().copied().unwrap_or("");
    if ends_with(stack, &["reportingOwnerId", last]) {
        match last {
            "rptOwnerCik" => o.cik = Some(text.to_string()),
            "rptOwnerName" => o.name = Some(text.to_string()),
            _ => {}
        }
    } else if ends_with(stack, &["reportingOwnerRelationship", last]) {
        match last {
            "isDirector" => rel.is_director = flag(text).unwrap_or(false),
            "isOfficer" => rel.is_officer = flag(text).unwrap_or(false),
            "isTenPercentOwner" => rel.is_ten_percent_owner = flag(text).unwrap_or(false),
            "isOther" => rel.is_other = flag(text).unwrap_or(false),
            "officerTitle" => rel.officer_title = Some(text.to_string()),
            _ => {}
        }
    }
}

fn assign_txn(t: &mut TxnBuilder, stack: &[&str], value: Option<String>) {
    let slot = if ends_with(stack, &["securityTitle", "value"]) {
        &mut t.security_title
    } else if ends_with(stack, &["transactionDate", "value"]) {
        &mut t.date
    } else if ends_with(stack, &["transactionCoding", "transactionCode"]) {
        &mut t.code
    } else if ends_with(stack, &["transactionAmounts", "transactionShares", "value"]) {
        &mut t.shares
    } else if ends_with(
        stack,
        &["transactionAmounts", "transactionPricePerShare", "value"],
    ) {
        &mut t.price
    } else if ends_with(stack, &["transactionAcquiredDisposedCode", "value"]) {
        &mut t.acquired_disposed
    } else if ends_with(stack, &["sharesOwnedFollowingTransaction", "value"]) {
        &mut t.shares_after
    } else if ends_with(stack, &["directOrIndirectOwnership", "value"]) {
        &mut t.ownership
    } else {
        return;
    };
    *slot = value;
}

/// SEC filings write booleans as `1`/`0` or `true`/`false`.
fn number(field: &'static str, s: Option<String>) -> Result<Option<f64>, ParseError> {
    match s {
        None => Ok(None),
        Some(raw) => parse_decimal(&raw)
            .map(Some)
            .ok_or(ParseError::Invalid { field, value: raw }),
    }
}

fn date(field: &'static str, s: &str) -> Result<Date, ParseError> {
    s.parse().map_err(|_| ParseError::Invalid {
        field,
        value: s.to_string(),
    })
}

fn cik(field: &'static str, s: &str) -> Result<Cik, ParseError> {
    s.parse().map_err(|_| ParseError::Invalid {
        field,
        value: s.to_string(),
    })
}

fn build(doc: DocBuilder) -> Result<OwnershipDocument, ParseError> {
    let issuer = Issuer {
        cik: cik(
            "issuerCik",
            doc.issuer_cik
                .as_deref()
                .ok_or(ParseError::Missing("issuerCik"))?,
        )?,
        name: doc.issuer_name.unwrap_or_default(),
        ticker: doc.issuer_ticker.filter(|t| !t.is_empty() && t != "NONE"),
    };

    let mut owners = Vec::with_capacity(doc.owners.len());
    for o in doc.owners {
        owners.push(ReportingOwner {
            cik: cik(
                "rptOwnerCik",
                o.cik.as_deref().ok_or(ParseError::Missing("rptOwnerCik"))?,
            )?,
            name: o.name.unwrap_or_default(),
            relationship: o.relationship,
        });
    }
    if owners.is_empty() {
        return Err(ParseError::Missing("reportingOwner"));
    }

    let mut transactions = Vec::with_capacity(doc.transactions.len());
    for t in doc.transactions {
        let acquired_disposed = match t.acquired_disposed.as_deref() {
            Some("A") => Some(AcquiredDisposed::Acquired),
            Some("D") => Some(AcquiredDisposed::Disposed),
            Some(other) => {
                return Err(ParseError::Invalid {
                    field: "transactionAcquiredDisposedCode",
                    value: other.to_string(),
                })
            }
            None => None,
        };
        let ownership = match t.ownership.as_deref() {
            Some("D") => Some(OwnershipNature::Direct),
            Some("I") => Some(OwnershipNature::Indirect),
            _ => None,
        };
        transactions.push(Transaction {
            is_derivative: t.is_derivative,
            security_title: t.security_title.unwrap_or_default(),
            date: date(
                "transactionDate",
                t.date
                    .as_deref()
                    .ok_or(ParseError::Missing("transactionDate"))?,
            )?,
            code: TransactionCode::parse(
                t.code
                    .as_deref()
                    .ok_or(ParseError::Missing("transactionCode"))?,
            ),
            shares: number("transactionShares", t.shares)?,
            price_per_share: number("transactionPricePerShare", t.price)?,
            acquired_disposed,
            shares_owned_after: number("sharesOwnedFollowingTransaction", t.shares_after)?,
            ownership,
        });
    }

    Ok(OwnershipDocument {
        document_type: doc
            .document_type
            .ok_or(ParseError::Missing("documentType"))?,
        period_of_report: doc
            .period_of_report
            .as_deref()
            .map(|s| date("periodOfReport", s))
            .transpose()?,
        date_of_original_submission: doc
            .date_of_original_submission
            .as_deref()
            .map(|s| date("dateOfOriginalSubmission", s))
            .transpose()?,
        issuer,
        owners,
        aff_10b5_1: doc.aff_10b5_1.as_deref().and_then(flag),
        not_subject_to_section16: doc.not_subject_to_section16.as_deref().and_then(flag),
        transactions,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture(name: &str) -> String {
        let path = format!(
            "{}/../../fixtures/form4/synthetic/{name}",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    #[test]
    fn parses_a_plain_purchase() {
        let doc = parse(&fixture("01_director_a_buy.xml")).unwrap();
        assert_eq!(doc.document_type, "4");
        assert_eq!(doc.issuer.cik, Cik(9_999_901));
        assert_eq!(doc.issuer.ticker.as_deref(), Some("SYNW"));
        assert_eq!(doc.aff_10b5_1, Some(false));
        assert_eq!(doc.owners.len(), 1);
        assert!(doc.owners[0].relationship.is_director);
        let t = &doc.transactions[0];
        assert!(t.is_open_market_purchase());
        assert_eq!(t.shares, Some(10_000.0));
        assert_eq!(t.value(), Some(125_000.0));
        assert_eq!(t.date, Date::from_ymd(2026, 3, 2).unwrap());
    }

    #[test]
    fn handles_footnotes_entities_and_holdings() {
        let doc = parse(&fixture("02_ceo_buy_with_footnotes.xml")).unwrap();
        assert_eq!(doc.owners[0].name, "Officer Bravo & Family");
        let rel = &doc.owners[0].relationship;
        assert!(rel.is_director && rel.is_officer);
        assert_eq!(
            rel.officer_title.as_deref(),
            Some("Chief Executive Officer")
        );
        // The holding row is not a transaction.
        assert_eq!(doc.transactions.len(), 1);
        let t = &doc.transactions[0];
        assert_eq!(t.price_per_share, Some(12.8125));
        assert_eq!(t.ownership, Some(OwnershipNature::Indirect));
    }

    #[test]
    fn separates_derivative_and_non_derivative_rows() {
        let doc = parse(&fixture("04_planned_sale_and_option_exercise.xml")).unwrap();
        assert_eq!(doc.aff_10b5_1, Some(true));
        assert_eq!(doc.transactions.len(), 3);
        assert_eq!(
            doc.transactions.iter().filter(|t| t.is_derivative).count(),
            1
        );
        assert!(doc
            .transactions
            .iter()
            .all(|t| !t.is_open_market_purchase()));
        assert_eq!(doc.transactions[1].code, TransactionCode::S);
    }

    #[test]
    fn footnote_only_price_is_none_and_amendment_is_flagged() {
        let doc = parse(&fixture("05_late_buy_outside_window.xml")).unwrap();
        assert!(doc.is_amendment());
        assert_eq!(doc.aff_10b5_1, None);
        assert_eq!(doc.transactions[0].price_per_share, None);
        assert_eq!(doc.transactions[0].value(), None);
    }

    #[test]
    fn rejects_other_documents() {
        assert_eq!(
            parse("<html><body/></html>"),
            Err(ParseError::NotAnOwnershipDocument)
        );
        let no_issuer = "<ownershipDocument><documentType>4</documentType></ownershipDocument>";
        assert_eq!(parse(no_issuer), Err(ParseError::Missing("issuerCik")));
    }
}
