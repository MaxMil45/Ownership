//! Reads Schedule 13D / 13G XML tokens into a [`Schedule13`].

use crate::{ReportingPerson, Schedule, Schedule13};
use oe_core::{parse_us_date, Cik, Date};
use oe_form4::xml::{tokenize, Token, XmlError};
use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    Xml(XmlError),
    NotSchedule13,
    Missing(&'static str),
    Invalid { field: &'static str, value: String },
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ParseError::Xml(e) => write!(f, "{e}"),
            ParseError::NotSchedule13 => f.write_str(
                "not a Schedule 13D or 13G (<edgarSubmission> with a 13D/13G submissionType)",
            ),
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

fn number(field: &'static str, text: &str) -> Result<Option<f64>, ParseError> {
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, ',' | '%' | '$'))
        .collect();
    let t = cleaned.trim();
    if t.is_empty() || t.eq_ignore_ascii_case("n/a") {
        return Ok(None);
    }
    t.parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
        .map(Some)
        .ok_or(ParseError::Invalid {
            field,
            value: text.to_string(),
        })
}

fn date(field: &'static str, text: &str) -> Result<Option<Date>, ParseError> {
    let t = text.trim();
    if t.is_empty() {
        return Ok(None);
    }
    parse_us_date(t)
        .or_else(|| t.parse().ok().filter(|_| t.len() == 10))
        .map(Some)
        .ok_or(ParseError::Invalid {
            field,
            value: text.to_string(),
        })
}

fn flag(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "y" | "yes" | "true" | "1" => Some(true),
        "n" | "no" | "false" | "0" => Some(false),
        _ => None,
    }
}

#[derive(Default)]
struct Builder {
    submission_type: Option<String>,
    amendment_no: Option<u32>,
    previous_accession: Option<String>,
    filer_ciks: Vec<Cik>,
    issuer_cik: Option<String>,
    issuer_name: Option<String>,
    cusips: Vec<String>,
    class_title: Option<String>,
    event_date: Option<Date>,
    previously_filed: Option<bool>,
    rules: Vec<String>,
    persons: Vec<ReportingPerson>,
    purpose: Vec<String>,
    signature_date: Option<Date>,
}

/// Parses the XML of one Schedule 13D or 13G (the `primary_doc.xml`).
pub fn parse(xml: &str) -> Result<Schedule13, ParseError> {
    let tokens = tokenize(xml)?;
    if !matches!(tokens.first(), Some(Token::Start("edgarSubmission"))) {
        return Err(ParseError::NotSchedule13);
    }

    let mut b = Builder::default();
    let mut person: Option<ReportingPerson> = None;
    let mut stack: Vec<&str> = Vec::new();

    for token in &tokens {
        match token {
            Token::Start(name) => {
                stack.push(name);
                if matches!(
                    *name,
                    "reportingPersonInfo" | "coverPageHeaderReportingPersonDetails"
                ) {
                    person = Some(ReportingPerson::default());
                }
            }
            Token::End(name) => {
                stack.pop();
                if matches!(
                    *name,
                    "reportingPersonInfo" | "coverPageHeaderReportingPersonDetails"
                ) {
                    b.persons.extend(person.take());
                }
            }
            Token::Empty(_) => {}
            Token::Text(text) => {
                let Some(&name) = stack.last() else { continue };
                let parent = stack.len().checked_sub(2).map(|i| stack[i]).unwrap_or("");
                match person.as_mut() {
                    Some(p) => assign_person(p, name, text)?,
                    None => assign_top(&mut b, name, parent, &stack, text)?,
                }
            }
        }
    }

    let submission_type = b
        .submission_type
        .ok_or(ParseError::Missing("submissionType"))?;
    let schedule = if submission_type.contains("13D") {
        Schedule::D
    } else if submission_type.contains("13G") {
        Schedule::G
    } else {
        return Err(ParseError::NotSchedule13);
    };
    let issuer_cik = b.issuer_cik.ok_or(ParseError::Missing("issuer CIK"))?;
    let issuer_cik = issuer_cik.parse::<Cik>().map_err(|_| ParseError::Invalid {
        field: "issuer CIK",
        value: issuer_cik.clone(),
    })?;

    Ok(Schedule13 {
        submission_type,
        schedule,
        amendment_no: b.amendment_no,
        previous_accession: b.previous_accession,
        filer_ciks: b.filer_ciks,
        issuer_cik,
        issuer_name: b.issuer_name.unwrap_or_default(),
        cusips: b.cusips,
        class_title: b.class_title.unwrap_or_default(),
        event_date: b.event_date,
        previously_filed_13g: b.previously_filed,
        rules: b.rules,
        persons: b.persons,
        purpose_of_transaction: (!b.purpose.is_empty()).then(|| b.purpose.join("\n")),
        signature_date: b.signature_date,
    })
}

fn assign_top(
    b: &mut Builder,
    name: &str,
    parent: &str,
    stack: &[&str],
    text: &str,
) -> Result<(), ParseError> {
    let in_item4 = stack.contains(&"item4");
    match (name, parent) {
        ("submissionType", _) => b.submission_type = Some(text.to_string()),
        ("previousAccessionNumber", _) => b.previous_accession = Some(text.to_string()),
        ("cik", "filerCredentials") => {
            if let Ok(c) = text.parse::<Cik>() {
                b.filer_ciks.push(c);
            }
        }
        ("amendmentNo", _) => b.amendment_no = text.trim().parse().ok(),
        ("securitiesClassTitle", "coverPageHeader") => b.class_title = Some(text.to_string()),
        ("dateOfEvent", _) | ("eventDateRequiresFilingThisStatement", _) => {
            b.event_date = date("event date", text)?
        }
        ("previouslyFiledFlag", _) => b.previously_filed = flag(text),
        // 13D writes issuerCIK, 13G issuerCik.
        ("issuerCIK", "issuerInfo") | ("issuerCik", "issuerInfo") => {
            b.issuer_cik = Some(text.to_string())
        }
        ("issuerName", "issuerInfo") => b.issuer_name = Some(text.to_string()),
        // Most filings list CUSIPs under issuerCusips; some write one issuerCUSIP tag.
        ("issuerCusipNumber", _) | ("issuerCUSIP", _) | ("issuerCusip", _) => {
            b.cusips.push(text.to_string())
        }
        ("designateRulePursuantThisScheduleFiled", _) => b.rules.push(text.to_string()),
        ("transactionPurpose", _) if in_item4 => b.purpose.push(text.to_string()),
        ("date", "signatureDetails") => {
            // The last signature date wins; they are normally all the same.
            b.signature_date = date("signature date", text)?.or(b.signature_date)
        }
        _ => {}
    }
    Ok(())
}

fn assign_person(p: &mut ReportingPerson, name: &str, text: &str) -> Result<(), ParseError> {
    match name {
        "reportingPersonName" => p.name = text.to_string(),
        "reportingPersonCIK" => p.cik = text.parse().ok(),
        "memberOfGroup" => p.member_of_group = Some(text.trim().to_string()),
        "citizenshipOrOrganization" => p.citizenship_or_organization = Some(text.to_string()),
        "soleVotingPower" => p.sole_voting = number("soleVotingPower", text)?,
        "sharedVotingPower" => p.shared_voting = number("sharedVotingPower", text)?,
        "soleDispositivePower" => p.sole_dispositive = number("soleDispositivePower", text)?,
        "sharedDispositivePower" => p.shared_dispositive = number("sharedDispositivePower", text)?,
        // 13D / 13G spellings of the same two values.
        "aggregateAmountOwned" | "reportingPersonBeneficiallyOwnedAggregateNumberOfShares" => {
            p.aggregate_shares = number("aggregate shares", text)?
        }
        "percentOfClass" | "classPercent" => p.percent_of_class = number("percent of class", text)?,
        "typeOfReportingPerson" => p.person_type = Some(text.trim().to_string()),
        "fundType" => p.fund_type = Some(text.trim().to_string()),
        _ => {}
    }
    Ok(())
}
