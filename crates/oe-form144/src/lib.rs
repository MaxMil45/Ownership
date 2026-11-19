//! Parser for SEC Form 144, the notice an insider files before selling
//! restricted or control shares under Rule 144.
//!
//! Only the XML era (April 2023 onwards) is covered. The XML uses a namespace
//! (default or prefixed, such as `own:`); the tokenizer from `oe-form4` strips
//! prefixes, so both layouts read the same.
//!
//! ```
//! let xml = std::fs::read_to_string(concat!(
//!     env!("CARGO_MANIFEST_DIR"),
//!     "/../../fixtures/form144/synthetic/01_officer_plan_sale.xml"
//! )).unwrap();
//! let form = oe_form144::parse(&xml).unwrap();
//! assert_eq!(form.submission_type, "144");
//! assert!(form.under_plan());
//! ```

mod parse;

pub use parse::{parse, ParseError};

use oe_core::{Cik, Date};

/// One `securitiesInformation` block: what is to be sold, and through whom.
#[derive(Debug, Clone, PartialEq)]
pub struct SecuritiesToSell {
    pub class_title: String,
    pub units: Option<f64>,
    /// The filer's own aggregate market value, in US dollars.
    pub aggregate_market_value: Option<f64>,
    pub units_outstanding: Option<f64>,
    pub approx_sale_date: Option<Date>,
    pub exchange: Option<String>,
    pub broker: Option<String>,
}

/// One `securitiesToBeSold` block: how the shares were acquired.
#[derive(Debug, Clone, PartialEq)]
pub struct Acquisition {
    pub acquired_date: Option<Date>,
    /// Free text, for example `Stock Option Exercise` or `Restricted Stock Vesting`.
    pub nature: String,
    pub from_whom: Option<String>,
    pub is_gift: Option<bool>,
    pub amount: Option<f64>,
}

/// One `securitiesSoldInPast3Months` block.
#[derive(Debug, Clone, PartialEq)]
pub struct PriorSale {
    pub seller: String,
    pub date: Option<Date>,
    pub amount: Option<f64>,
    pub gross_proceeds: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Form144 {
    /// `144` or `144/A`.
    pub submission_type: String,
    pub issuer_cik: Cik,
    pub issuer_name: String,
    /// `nameOfPersonForWhoseAccountTheSecuritiesAreToBeSold`.
    pub seller_name: String,
    /// Every ticked relationship, as written (`Officer`, `Director`,
    /// `10% Stockholder`, or free text such as `May be deemed affiliate`).
    pub relationships: Vec<String>,
    pub securities: Vec<SecuritiesToSell>,
    pub acquisitions: Vec<Acquisition>,
    pub sold_past_3_months: Vec<PriorSale>,
    /// The "nothing to report" box for the past-three-months section.
    pub nothing_sold_past_3_months: Option<bool>,
    pub notice_date: Option<Date>,
    /// Rule 10b5-1 plan adoption dates the notice lists, if any.
    pub plan_adoption_dates: Vec<Date>,
    pub remarks: Option<String>,
}

impl Form144 {
    pub fn is_amendment(&self) -> bool {
        self.submission_type.ends_with("/A")
    }

    /// Whether the notice names at least one Rule 10b5-1 plan adoption date.
    pub fn under_plan(&self) -> bool {
        !self.plan_adoption_dates.is_empty()
    }

    /// Whether the seller names a current insider role among the relationships.
    ///
    /// The field is free text in practice: `Officer`, `Director`, `10%
    /// Stockholder`, but also `Chief Financial Officer`, `President & CEO`,
    /// `Director/Non-Executive Director`. A relationship counts when, ignoring
    /// case, it contains `officer`, `director`, `ceo`, `cfo`, `coo`,
    /// `president`, `chairman` or `10%` (as in `10% Stockholder`), unless it
    /// starts with `former`. Others (`Affiliate`, `Shareholder`, `See Remarks`,
    /// `May be deemed affiliate`) do not count.
    pub fn is_insider(&self) -> bool {
        self.relationships.iter().any(|r| is_insider_role(r))
    }
    /// Units to be sold, summed over the securities blocks that give a number.
    pub fn total_units(&self) -> Option<f64> {
        sum_known(self.securities.iter().map(|s| s.units))
    }

    /// Aggregate market value, summed over the blocks that give one.
    pub fn total_market_value(&self) -> Option<f64> {
        sum_known(self.securities.iter().map(|s| s.aggregate_market_value))
    }

    /// Units already sold in the past three months, per this notice.
    pub fn units_sold_past_3_months(&self) -> f64 {
        self.sold_past_3_months
            .iter()
            .filter_map(|s| s.amount)
            .sum()
    }
}

/// One relationship text against the insider test (see [`Form144::is_insider`]).
fn is_insider_role(text: &str) -> bool {
    let t = text.trim().to_ascii_lowercase();
    if t.starts_with("former") {
        return false;
    }
    if t.contains("officer")
        || t.contains("director")
        || t.contains("president")
        || t.contains("chairman")
        || t.contains("10%")
    {
        return true;
    }
    // Short titles only count as whole words (`ceo` inside another word does not).
    t.split(|c: char| !c.is_ascii_alphanumeric())
        .any(|w| matches!(w, "ceo" | "cfo" | "coo"))
}

/// Sum of the `Some` values; `None` if there are none.
fn sum_known(values: impl Iterator<Item = Option<f64>>) -> Option<f64> {
    values
        .flatten()
        .fold(None, |acc, v| Some(acc.unwrap_or(0.0) + v))
}
