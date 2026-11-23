//! Form 144, Schedule 13D/13G, contested-proxy headers and Item 408 trading
//! arrangements in the ledger (migration `0005_other_forms`, ADR 0009).
//!
//! Each filing gets its receipt row in `filing`, like a Form 4, and its
//! content in the form's own tables. Loading is idempotent: a filing already
//! stored with the same fingerprint is left alone; one whose source changed is
//! reported and never overwritten. Street addresses and CUSIPs are not stored.

use crate::db::{check_schema, StoreError};
use crate::rows::{FilingRow, PrepareError};
use crate::sha256::sha256_hex;
use oe_core::{Cik, Date, Party};
use postgres::{Client, Transaction};

/// A contested-solicitation filing as its SGML header describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct ContestHeader {
    pub form: String,
    pub subject: Party,
    pub filed_by: Vec<Party>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum OtherDoc {
    Form144(oe_form144::Form144),
    Schedule13(oe_sched13::Schedule13),
    Contest(ContestHeader),
    /// A 10-Q/10-K's Item 408 facts; `report_type` is the form from the sidecar.
    Item408 {
        doc: oe_item408::Item408Doc,
        report_type: Option<String>,
    },
}

impl OtherDoc {
    pub fn kind(&self) -> &'static str {
        match self {
            OtherDoc::Form144(_) => "Form 144",
            OtherDoc::Schedule13(_) => "Schedule 13D/G",
            OtherDoc::Contest(_) => "contest filing",
            OtherDoc::Item408 { .. } => "10-Q/10-K",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct OtherFiling {
    pub filing: FilingRow,
    pub doc: OtherDoc,
}

/// Builds the receipt row. `raw` is the file exactly as saved (it is
/// fingerprinted); `accepted_at` is zoned text as `Sidecar::accepted_at`
/// gives it.
pub fn prepare_other(
    doc: OtherDoc,
    accession: &str,
    accepted_at: String,
    source_url: String,
    raw: &[u8],
) -> Result<OtherFiling, PrepareError> {
    // The filer: the seller's filer, the first submitting filer, the first
    // challenger, or the registrant. Never a guess: an issuer stands in only
    // when the filing names no filer.
    let (form_type, filer): (String, Cik) = match &doc {
        OtherDoc::Form144(f) => (
            f.submission_type.clone(),
            f.filer_cik.unwrap_or(f.issuer_cik),
        ),
        OtherDoc::Schedule13(s) => (
            s.submission_type.clone(),
            s.filer_ciks
                .first()
                .copied()
                .or_else(|| s.persons.iter().find_map(|p| p.cik))
                .unwrap_or(s.issuer_cik),
        ),
        OtherDoc::Contest(c) => (
            c.form.clone(),
            c.filed_by
                .iter()
                .find(|p| p.cik != c.subject.cik)
                .map_or(c.subject.cik, |p| p.cik),
        ),
        OtherDoc::Item408 { doc, report_type } => (
            report_type
                .clone()
                .or_else(|| doc.document_type.clone())
                .unwrap_or_else(|| "10-Q/10-K".into()),
            doc.cik
                .ok_or_else(|| PrepareError::Parse("the report names no registrant CIK".into()))?,
        ),
    };
    Ok(OtherFiling {
        filing: FilingRow {
            accession: accession.to_string(),
            form_type,
            filer_cik: filer.0,
            accepted_at,
            source_url,
            raw_sha256: sha256_hex(raw),
            source: "xml",
        },
        doc,
    })
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct OtherReport {
    pub filings_new: u64,
    pub filings_existing: u64,
    /// Content rows written (all the forms' tables together).
    pub rows_new: u64,
    /// Already stored with a different fingerprint: left as they are.
    pub changed_source: Vec<String>,
}

fn num(v: Option<f64>) -> Option<String> {
    v.filter(|x| x.is_finite()).map(|x| x.to_string())
}

fn day(d: Option<Date>) -> Option<String> {
    d.map(|d| d.to_string())
}

/// Loads the batch in one transaction.
pub fn load_other(client: &mut Client, batch: &[OtherFiling]) -> Result<OtherReport, StoreError> {
    check_schema(client)?;
    let mut tx = client.transaction()?;
    let mut report = OtherReport::default();
    for f in batch {
        let r = &f.filing;
        let inserted = tx.execute(
            "INSERT INTO filing (accession, form_type, filer_cik, accepted_at, source_url, raw_sha256, source)
             VALUES ($1, $2, $3, $4::text::timestamptz, $5, $6, $7)
             ON CONFLICT (accession) DO NOTHING",
            &[
                &r.accession,
                &r.form_type,
                &(r.filer_cik as i64),
                &r.accepted_at,
                &r.source_url,
                &r.raw_sha256,
                &r.source,
            ],
        )?;
        if inserted == 0 {
            report.filings_existing += 1;
            let stored: String = tx
                .query_one(
                    "SELECT raw_sha256 FROM filing WHERE accession = $1",
                    &[&r.accession],
                )?
                .get(0);
            if stored != r.raw_sha256 {
                report.changed_source.push(r.accession.clone());
                continue;
            }
        } else {
            report.filings_new += 1;
        }
        report.rows_new += insert_content(&mut tx, &r.accession, &f.doc)?;
    }
    tx.commit()?;
    Ok(report)
}

/// The form's own rows; `ON CONFLICT DO NOTHING` makes a repeat a no-op.
fn insert_content(tx: &mut Transaction<'_>, acc: &str, doc: &OtherDoc) -> Result<u64, StoreError> {
    let mut n = 0u64;
    match doc {
        OtherDoc::Form144(f) => {
            let sold: f64 = f.sold_past_3_months.iter().filter_map(|p| p.amount).sum();
            let dates: Vec<String> = f
                .plan_adoption_dates
                .iter()
                .map(|d| d.to_string())
                .collect();
            n += tx.execute(
                "INSERT INTO form144_notice (accession, issuer_cik, issuer_name, seller_name, relationships,
                     notice_date, plan_adoption_dates, nothing_sold_past_3_months, sold_past_3_months, remarks)
                 VALUES ($1, $2, $3, $4, $5, $6::text::date, $7::text[]::date[], $8, $9::text::numeric, $10)
                 ON CONFLICT DO NOTHING",
                &[
                    &acc,
                    &(f.issuer_cik.0 as i64),
                    &f.issuer_name,
                    &f.seller_name,
                    &f.relationships,
                    &day(f.notice_date),
                    &dates,
                    &f.nothing_sold_past_3_months,
                    &num((!f.sold_past_3_months.is_empty()).then_some(sold)),
                    &f.remarks,
                ],
            )?;
            for (i, s) in f.securities.iter().enumerate() {
                n += tx.execute(
                    "INSERT INTO form144_security (accession, position, class_title, units, aggregate_market_value,
                         units_outstanding, approx_sale_date, exchange, broker)
                     VALUES ($1, $2, $3, $4::text::numeric, $5::text::numeric, $6::text::numeric,
                             $7::text::date, $8, $9)
                     ON CONFLICT DO NOTHING",
                    &[
                        &acc,
                        &(i as i32),
                        &s.class_title,
                        &num(s.units),
                        &num(s.aggregate_market_value),
                        &num(s.units_outstanding),
                        &day(s.approx_sale_date),
                        &s.exchange,
                        &s.broker,
                    ],
                )?;
            }
        }
        OtherDoc::Schedule13(s) => {
            let schedule = match s.schedule {
                oe_sched13::Schedule::D => "D",
                oe_sched13::Schedule::G => "G",
            };
            n += tx.execute(
                "INSERT INTO schedule13_filing (accession, submission_type, schedule, amendment_no,
                     previous_accession, issuer_cik, issuer_name, class_title, event_date,
                     previously_filed_13g, rules, purpose_of_transaction, signature_date)
                 VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::text::date, $10, $11, $12, $13::text::date)
                 ON CONFLICT DO NOTHING",
                &[
                    &acc,
                    &s.submission_type,
                    &schedule,
                    &s.amendment_no.map(|a| a as i32),
                    &s.previous_accession,
                    &(s.issuer_cik.0 as i64),
                    &s.issuer_name,
                    &s.class_title,
                    &day(s.event_date),
                    &s.previously_filed_13g,
                    &s.rules,
                    &s.purpose_of_transaction,
                    &day(s.signature_date),
                ],
            )?;
            for (i, p) in s.persons.iter().enumerate() {
                n += tx.execute(
                    "INSERT INTO schedule13_person (accession, position, name, cik, member_of_group,
                         citizenship_or_organization, sole_voting, shared_voting, sole_dispositive,
                         shared_dispositive, aggregate_shares, percent_of_class, person_type, fund_type)
                     VALUES ($1, $2, $3, $4, $5, $6, $7::text::numeric, $8::text::numeric, $9::text::numeric,
                             $10::text::numeric, $11::text::numeric, $12::text::numeric, $13, $14)
                     ON CONFLICT DO NOTHING",
                    &[
                        &acc,
                        &(i as i32),
                        &p.name,
                        &p.cik.map(|c| c.0 as i64),
                        &p.member_of_group,
                        &p.citizenship_or_organization,
                        &num(p.sole_voting),
                        &num(p.shared_voting),
                        &num(p.sole_dispositive),
                        &num(p.shared_dispositive),
                        &num(p.aggregate_shares),
                        &num(p.percent_of_class),
                        &p.person_type,
                        &p.fund_type,
                    ],
                )?;
            }
        }
        OtherDoc::Contest(c) => {
            n += tx.execute(
                "INSERT INTO contest_filing (accession, form, subject_cik, subject_name)
                 VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
                &[&acc, &c.form, &(c.subject.cik.0 as i64), &c.subject.name],
            )?;
            for (i, p) in c.filed_by.iter().enumerate() {
                n += tx.execute(
                    "INSERT INTO contest_filer (accession, position, cik, name)
                     VALUES ($1, $2, $3, $4) ON CONFLICT DO NOTHING",
                    &[&acc, &(i as i32), &(p.cik.0 as i64), &p.name],
                )?;
            }
        }
        OtherDoc::Item408 { doc, report_type } => {
            let Some(cik) = doc.cik else { return Ok(n) };
            for (i, a) in doc.arrangements.iter().enumerate() {
                n += tx.execute(
                    "INSERT INTO trading_arrangement (accession, position, registrant_cik, report_type,
                         period_end, arrangement, person, title, rule_10b5_1_adopted, rule_10b5_1_terminated,
                         non_rule_adopted, non_rule_terminated, adoption_date, termination_date,
                         expiration_date, securities_amount, duration_days, terms_text)
                     VALUES ($1, $2, $3, $4, $5::text::date, $6, $7, $8, $9, $10, $11, $12,
                             $13::text::date, $14::text::date, $15::text::date, $16::text::numeric, $17, $18)
                     ON CONFLICT DO NOTHING",
                    &[
                        &acc,
                        &(i as i32),
                        &(cik.0 as i64),
                        &report_type.clone().or_else(|| doc.document_type.clone()),
                        &day(a.period_end.or(doc.period_end)),
                        &a.arrangement,
                        &a.person,
                        &a.title,
                        &a.rule_10b5_1_adopted,
                        &a.rule_10b5_1_terminated,
                        &a.non_rule_adopted,
                        &a.non_rule_terminated,
                        &day(a.adoption_date),
                        &day(a.termination_date),
                        &day(a.expiration_date),
                        &num(a.securities_amount),
                        &a.duration_days.map(|d| d as i32),
                        &a.terms_text,
                    ],
                )?;
            }
        }
    }
    Ok(n)
}

/// A Schedule 13G or 13G/A in the ledger, for `activist.passive_to_active@2`.
#[derive(Debug, Clone, PartialEq)]
pub struct Stored13G {
    pub issuer_cik: u64,
    pub accession: String,
    /// `YYYY-MM-DD`, the US Eastern date of acceptance.
    pub known_on: String,
    pub filer_cik: u64,
    pub holders: Vec<String>,
    pub largest_percent: Option<f64>,
}

const SCHEDULE13G_HISTORY: &str = "
SELECT s.issuer_cik, s.accession,
       (f.accepted_at AT TIME ZONE 'America/New_York')::date::text,
       f.filer_cik,
       coalesce(array_agg(p.name ORDER BY p.position) FILTER (WHERE p.name IS NOT NULL), '{}'),
       max(p.percent_of_class)::float8
FROM schedule13_filing s
JOIN filing f ON f.accession = s.accession
LEFT JOIN schedule13_person p ON p.accession = s.accession
WHERE s.schedule = 'G' AND s.issuer_cik = ANY($1)
GROUP BY s.issuer_cik, s.accession, f.accepted_at, f.filer_cik";

/// Every Schedule 13G and 13G/A the ledger holds for these issuers.
pub fn schedule13g_history(
    client: &mut Client,
    issuers: &[u64],
) -> Result<Vec<Stored13G>, StoreError> {
    check_schema(client)?;
    let ids: Vec<i64> = issuers.iter().map(|&c| c as i64).collect();
    Ok(client
        .query(SCHEDULE13G_HISTORY, &[&ids])?
        .iter()
        .map(|r| Stored13G {
            issuer_cik: r.get::<_, i64>(0) as u64,
            accession: r.get(1),
            known_on: r.get(2),
            filer_cik: r.get::<_, i64>(3) as u64,
            holders: r.get(4),
            largest_percent: r.get(5),
        })
        .collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn party(cik: u64, name: &str) -> Party {
        Party {
            cik: Cik(cik),
            name: name.into(),
        }
    }

    #[test]
    fn the_filer_is_the_challenger_not_the_company() {
        let c = ContestHeader {
            form: "DFAN14A".into(),
            subject: party(1, "Target Co"),
            filed_by: vec![party(1, "Target Co"), party(7, "Activist LP")],
        };
        let f = prepare_other(
            OtherDoc::Contest(c),
            "0000000007-26-000001",
            "2026-11-20 10:00:00 America/New_York".into(),
            "https://www.sec.gov/x".into(),
            b"<SEC-HEADER>",
        )
        .unwrap();
        assert_eq!(f.filing.filer_cik, 7);
        assert_eq!(f.filing.form_type, "DFAN14A");
        assert_eq!(f.filing.raw_sha256, sha256_hex(b"<SEC-HEADER>"));
        // A company's own contested filing: the company is the filer.
        let own = ContestHeader {
            form: "DEFC14A".into(),
            subject: party(1, "Target Co"),
            filed_by: vec![party(1, "Target Co")],
        };
        let f = prepare_other(
            OtherDoc::Contest(own),
            "a",
            String::new(),
            String::new(),
            b"",
        )
        .unwrap();
        assert_eq!(f.filing.filer_cik, 1);
    }

    #[test]
    fn a_report_without_a_registrant_cik_is_rejected() {
        let doc = OtherDoc::Item408 {
            doc: oe_item408::Item408Doc::default(),
            report_type: Some("10-Q".into()),
        };
        assert!(prepare_other(doc, "a", String::new(), String::new(), b"").is_err());
    }

    #[test]
    fn numbers_and_dates_go_as_text() {
        assert_eq!(num(Some(1234.5)).as_deref(), Some("1234.5"));
        assert_eq!(num(Some(f64::NAN)), None);
        assert_eq!(
            day("2026-11-20".parse().ok()).as_deref(),
            Some("2026-11-20")
        );
    }
}
