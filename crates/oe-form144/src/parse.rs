//! Turns Form 144 XML tokens into a [`Form144`], walking a stack of open
//! element names (as `oe-form4` does) so unknown elements are skipped.

use crate::{Acquisition, Form144, PriorSale, SecuritiesToSell};
use oe_core::text::{parse_decimal, parse_flag};
use oe_core::{parse_us_date, Cik, Date};
use oe_form4::xml::{tokenize, Token, XmlError};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    Xml(XmlError),
    NotForm144,
    Missing(&'static str),
    Invalid { field: &'static str, value: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Xml(e) => write!(f, "{e}"),
            ParseError::NotForm144 => {
                f.write_str("root element is not <edgarSubmission> with a Form 144 body")
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

/// `MM/DD/YYYY` (what Form 144 uses) or `YYYY-MM-DD`.
fn parse_date(field: &'static str, text: &str) -> Result<Date, ParseError> {
    let t = text.trim();
    parse_us_date(t)
        .or_else(|| t.parse::<Date>().ok().filter(|_| t.len() == 10))
        .ok_or(ParseError::Invalid {
            field,
            value: text.to_string(),
        })
}

fn parse_number(field: &'static str, text: &str) -> Result<f64, ParseError> {
    parse_decimal(text).ok_or(ParseError::Invalid {
        field,
        value: text.to_string(),
    })
}
#[derive(Default)]
struct Builder {
    submission_type: Option<String>,
    issuer_cik: Option<String>,
    issuer_name: Option<String>,
    seller_name: Option<String>,
    relationships: Vec<String>,
    securities: Vec<SecuritiesToSell>,
    acquisitions: Vec<Acquisition>,
    prior: Vec<PriorSale>,
    nothing_sold: Option<bool>,
    notice_date: Option<Date>,
    plan_dates: Vec<Date>,
    remarks: Vec<String>,
}

/// Which repeated block the parser is inside.
enum Block {
    None,
    Securities(SecuritiesToSell),
    Acquisition(Acquisition),
    Prior(PriorSale),
}

/// Parses the XML of one Form 144 (the `primary_doc.xml`).
pub fn parse(xml: &str) -> Result<Form144, ParseError> {
    let tokens = tokenize(xml)?;
    if !matches!(tokens.first(), Some(Token::Start("edgarSubmission"))) {
        return Err(ParseError::NotForm144);
    }

    let mut b = Builder::default();
    let mut block = Block::None;
    let mut stack: Vec<&str> = Vec::new();

    for token in &tokens {
        match token {
            Token::Start(name) => {
                stack.push(name);
                match *name {
                    "securitiesInformation" => {
                        block = Block::Securities(SecuritiesToSell {
                            class_title: String::new(),
                            units: None,
                            aggregate_market_value: None,
                            units_outstanding: None,
                            approx_sale_date: None,
                            exchange: None,
                            broker: None,
                        })
                    }
                    "securitiesToBeSold" => {
                        block = Block::Acquisition(Acquisition {
                            acquired_date: None,
                            nature: String::new(),
                            from_whom: None,
                            is_gift: None,
                            amount: None,
                        })
                    }
                    "securitiesSoldInPast3Months" => {
                        block = Block::Prior(PriorSale {
                            seller: String::new(),
                            date: None,
                            amount: None,
                            gross_proceeds: None,
                        })
                    }
                    _ => {}
                }
            }
            Token::End(name) => {
                stack.pop();
                if matches!(
                    *name,
                    "securitiesInformation" | "securitiesToBeSold" | "securitiesSoldInPast3Months"
                ) {
                    match std::mem::replace(&mut block, Block::None) {
                        Block::Securities(s) => b.securities.push(s),
                        Block::Acquisition(a) => b.acquisitions.push(a),
                        Block::Prior(p) => b.prior.push(p),
                        Block::None => {}
                    }
                }
            }
            Token::Empty(_) => {}
            Token::Text(text) => {
                let Some(&name) = stack.last() else { continue };
                let parent = stack.len().checked_sub(2).map(|i| stack[i]).unwrap_or("");
                match &mut block {
                    Block::Securities(s) => assign_securities(s, name, parent, text)?,
                    Block::Acquisition(a) => assign_acquisition(a, name, text)?,
                    Block::Prior(p) => assign_prior(p, name, parent, text)?,
                    Block::None => assign_top(&mut b, name, parent, text)?,
                }
                // `remarks` can sit in or beside any block.
                if name == "remarks" && !matches!(block, Block::None) {
                    b.remarks.push(text.clone());
                }
            }
        }
    }

    let issuer_cik = b.issuer_cik.ok_or(ParseError::Missing("issuerCik"))?;
    let issuer_cik = issuer_cik.parse::<Cik>().map_err(|_| ParseError::Invalid {
        field: "issuerCik",
        value: issuer_cik.clone(),
    })?;

    Ok(Form144 {
        submission_type: b
            .submission_type
            .ok_or(ParseError::Missing("submissionType"))?,
        issuer_cik,
        issuer_name: b.issuer_name.unwrap_or_default(),
        seller_name: b.seller_name.ok_or(ParseError::Missing(
            "nameOfPersonForWhoseAccountTheSecuritiesAreToBeSold",
        ))?,
        relationships: b.relationships,
        securities: b.securities,
        acquisitions: b.acquisitions,
        sold_past_3_months: b.prior,
        nothing_sold_past_3_months: b.nothing_sold,
        notice_date: b.notice_date,
        plan_adoption_dates: b.plan_dates,
        remarks: (!b.remarks.is_empty()).then(|| b.remarks.join(" ")),
    })
}

fn assign_top(b: &mut Builder, name: &str, parent: &str, text: &str) -> Result<(), ParseError> {
    match (name, parent) {
        ("submissionType", _) => b.submission_type = Some(text.to_string()),
        ("issuerCik", "issuerInfo") => b.issuer_cik = Some(text.to_string()),
        ("issuerName", "issuerInfo") => b.issuer_name = Some(text.to_string()),
        ("nameOfPersonForWhoseAccountTheSecuritiesAreToBeSold", _) => {
            b.seller_name = Some(text.to_string())
        }
        ("relationshipToIssuer", _) => b.relationships.push(text.to_string()),
        ("nothingToReportFlagOnSecuritiesSoldInPast3Months", _) => {
            b.nothing_sold = parse_flag(text)
        }
        ("noticeDate", _) => b.notice_date = Some(parse_date("noticeDate", text)?),
        ("planAdoptionDate", _) => b.plan_dates.push(parse_date("planAdoptionDate", text)?),
        ("remarks", _) => b.remarks.push(text.to_string()),
        _ => {}
    }
    Ok(())
}

fn assign_securities(
    s: &mut SecuritiesToSell,
    name: &str,
    parent: &str,
    text: &str,
) -> Result<(), ParseError> {
    match (name, parent) {
        ("securitiesClassTitle", _) => s.class_title = text.to_string(),
        ("noOfUnitsSold", _) => s.units = Some(parse_number("noOfUnitsSold", text)?),
        ("aggregateMarketValue", _) => {
            s.aggregate_market_value = Some(parse_number("aggregateMarketValue", text)?)
        }
        ("noOfUnitsOutstanding", _) => {
            s.units_outstanding = Some(parse_number("noOfUnitsOutstanding", text)?)
        }
        ("approxSaleDate", _) => s.approx_sale_date = Some(parse_date("approxSaleDate", text)?),
        ("securitiesExchangeName", _) => s.exchange = Some(text.to_string()),
        ("name", "brokerOrMarketmakerDetails") => s.broker = Some(text.to_string()),
        _ => {}
    }
    Ok(())
}

fn assign_acquisition(a: &mut Acquisition, name: &str, text: &str) -> Result<(), ParseError> {
    match name {
        "acquiredDate" => a.acquired_date = Some(parse_date("acquiredDate", text)?),
        "natureOfAcquisitionTransaction" => a.nature = text.to_string(),
        "nameOfPersonfromWhomAcquired" => a.from_whom = Some(text.to_string()),
        "isGiftTransaction" => a.is_gift = parse_flag(text),
        "amountOfSecuritiesAcquired" => {
            a.amount = Some(parse_number("amountOfSecuritiesAcquired", text)?)
        }
        _ => {}
    }
    Ok(())
}

fn assign_prior(p: &mut PriorSale, name: &str, parent: &str, text: &str) -> Result<(), ParseError> {
    match (name, parent) {
        ("name", "sellerDetails") => p.seller = text.to_string(),
        ("saleDate", _) => p.date = Some(parse_date("saleDate", text)?),
        ("amountOfSecuritiesSold", _) => {
            p.amount = Some(parse_number("amountOfSecuritiesSold", text)?)
        }
        ("grossProceeds", _) => p.gross_proceeds = Some(parse_number("grossProceeds", text)?),
        _ => {}
    }
    Ok(())
}
