//! Flattens ownership documents into one row per transaction, the shape the
//! event rules work on.

use oe_core::{Cik, Date, FilingMeta, OwnershipDocument, TransactionCode};

#[derive(Debug, Clone, PartialEq)]
pub struct InsiderTrade {
    pub issuer: Cik,
    pub ticker: Option<String>,
    /// The first reporting owner. Joint filers (for example a fund and its
    /// manager) are listed in `co_owners` so one trade is not counted twice.
    pub owner: Cik,
    pub owner_name: String,
    pub co_owners: Vec<Cik>,
    pub is_director: bool,
    pub is_officer: bool,
    pub officer_title: Option<String>,
    pub date: Date,
    pub code: TransactionCode,
    pub is_open_market_purchase: bool,
    pub shares: Option<f64>,
    pub price_per_share: Option<f64>,
    /// The filing's Rule 10b5-1 checkbox.
    pub plan_10b5_1: Option<bool>,
    pub from_amendment: bool,
    /// SEC acceptance date, when known.
    pub known_on: Option<Date>,
    pub accession: Option<String>,
}

impl InsiderTrade {
    pub fn value(&self) -> Option<f64> {
        Some(self.shares? * self.price_per_share?)
    }
}

/// One `InsiderTrade` per transaction row in the document.
pub fn trades_from_document(doc: &OwnershipDocument, meta: &FilingMeta) -> Vec<InsiderTrade> {
    let Some(primary) = doc.owners.first() else {
        return Vec::new();
    };
    let co_owners: Vec<Cik> = doc.owners.iter().skip(1).map(|o| o.cik).collect();

    doc.transactions
        .iter()
        .map(|t| InsiderTrade {
            issuer: doc.issuer.cik,
            ticker: doc.issuer.ticker.clone(),
            owner: primary.cik,
            owner_name: primary.name.clone(),
            co_owners: co_owners.clone(),
            is_director: primary.relationship.is_director,
            is_officer: primary.relationship.is_officer,
            officer_title: primary.relationship.officer_title.clone(),
            date: t.date,
            code: t.code.clone(),
            is_open_market_purchase: t.is_open_market_purchase(),
            shares: t.shares,
            price_per_share: t.price_per_share,
            plan_10b5_1: doc.aff_10b5_1,
            from_amendment: doc.is_amendment(),
            known_on: meta.known_on,
            accession: meta.accession.as_ref().map(|a| a.to_string()),
        })
        .collect()
}
