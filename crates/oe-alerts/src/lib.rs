//! Alerts: an event turned into a short message with its receipts (the SEC
//! filings it rests on), ready to filter, remember and deliver (ADR 0006).
//!
//! * [`convert`]: one [`Alert`] per event of each rule. Pure.
//! * [`watchlist`]: which issuers and insiders to alert on. Pure.
//! * [`sent`]: the append-only log of alerts already delivered.
//! * [`select`]: fresh, watched, not yet sent. Pure.
//! * [`render`]: plain text (Discord, console) and JSON (webhooks). Pure.
//! * `send` (feature `net`): HTTP delivery.

pub mod convert;
pub mod render;
pub mod select;
pub mod sent;
pub mod watchlist;

#[cfg(feature = "net")]
pub mod send;

use oe_core::{Cik, Date};

/// A filing an alert rests on, with a link a person can open.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub accession: String,
    pub form: Option<String>,
    /// The filing's folder in the EDGAR archive.
    pub url: String,
}

/// A person or entity named in an alert, for insider watchlists.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub cik: Option<Cik>,
    pub name: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Alert {
    /// Stable identity, so the same event is never delivered twice. Built
    /// from the rule ID and the facts that make the event this event (see
    /// each converter).
    pub key: String,
    /// `insider.cluster_buy@1`, ...
    pub rule_id: &'static str,
    /// The event's dollar amount, where it has one: a cluster buy's total, a
    /// planned sale's market value. For charts and sorting.
    pub amount_usd: Option<f64>,
    /// The stake in percent, where the event states one (13D/13G).
    pub percent: Option<f64>,
    /// US Eastern date the event became knowable.
    pub known_on: Option<Date>,
    pub issuer_cik: Option<Cik>,
    pub issuer_name: Option<String>,
    pub ticker: Option<String>,
    /// One line: what happened.
    pub headline: String,
    /// A few short lines of facts.
    pub details: Vec<String>,
    pub people: Vec<Person>,
    pub receipts: Vec<Receipt>,
}

/// URL of a filing's folder, under a CIK that is party to it.
pub fn receipt(cik: Cik, accession: &str, form: Option<&str>) -> Receipt {
    Receipt {
        accession: accession.to_string(),
        form: form.map(str::to_string),
        url: oe_edgar::filing_folder_url(cik.0, &accession.replace('-', "")),
    }
}
