//! `insider.planned_sale@1`: an insider files notice (Form 144) that they are
//! about to sell a large block of their company's stock.
//!
//! Spec: `events/specs/insider.planned_sale.v1.md`.

use oe_core::{Cik, Date, FilingMeta};
use oe_form144::Form144;

pub const RULE_ID: &str = "insider.planned_sale@1";

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedSaleParams {
    /// Smallest total aggregate market value, in US dollars, that fires an event.
    pub min_value: f64,
}

impl Default for PlannedSaleParams {
    fn default() -> Self {
        PlannedSaleParams {
            min_value: 1_000_000.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlannedSaleEvent {
    pub rule_id: &'static str,
    pub issuer: Cik,
    pub issuer_name: String,
    pub seller: String,
    pub relationships: Vec<String>,
    pub units: Option<f64>,
    /// Sum of `aggregateMarketValue` over the notice's securities blocks.
    pub aggregate_market_value: f64,
    /// Units to sell as a percentage of the units outstanding; empty unless
    /// the notice covers one share class and gives both numbers.
    pub percent_of_outstanding: Option<f64>,
    pub approx_sale_date: Option<Date>,
    /// The notice lists a Rule 10b5-1 plan adoption date.
    pub under_plan: bool,
    pub plan_adopted_on: Option<Date>,
    /// Units the notice lists as already sold in the past three months.
    pub sold_past_3_months: f64,
    pub accession: Option<String>,
    /// US Eastern date the SEC accepted the notice, when known.
    pub known_on: Option<Date>,
}

/// The event for one Form 144, or `None` if it does not qualify.
pub fn planned_sale_event(
    form: &Form144,
    meta: &FilingMeta,
    params: &PlannedSaleParams,
) -> Option<PlannedSaleEvent> {
    if form.is_amendment() || !form.is_insider() {
        return None;
    }
    let value = form.total_market_value()?;
    if value < params.min_value {
        return None;
    }

    let units = form.total_units();
    Some(PlannedSaleEvent {
        rule_id: RULE_ID,
        issuer: form.issuer_cik,
        issuer_name: form.issuer_name.clone(),
        seller: form.seller_name.clone(),
        relationships: form.relationships.clone(),
        units,
        aggregate_market_value: value,
        percent_of_outstanding: percent_of_outstanding(form, units),
        approx_sale_date: form.securities.first().and_then(|s| s.approx_sale_date),
        under_plan: form.under_plan(),
        plan_adopted_on: form.plan_adoption_dates.iter().min().copied(),
        sold_past_3_months: form.units_sold_past_3_months(),
        accession: meta.accession.as_ref().map(|a| a.to_string()),
        known_on: meta.known_on,
    })
}

fn percent_of_outstanding(form: &Form144, units: Option<f64>) -> Option<f64> {
    let first = form.securities.first()?;
    // Different classes have different outstanding counts: do not mix them.
    if form
        .securities
        .iter()
        .any(|s| s.class_title != first.class_title)
    {
        return None;
    }
    let outstanding = first.units_outstanding.filter(|o| *o > 0.0)?;
    Some(units? / outstanding * 100.0)
}

/// Events for a batch of notices, in the order given.
pub fn detect_planned_sales(
    filings: &[(Form144, FilingMeta)],
    params: &PlannedSaleParams,
) -> Vec<PlannedSaleEvent> {
    filings
        .iter()
        .filter_map(|(form, meta)| planned_sale_event(form, meta, params))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse_fixture(name: &str) -> Form144 {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/form144/synthetic")
            .join(name);
        oe_form144::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn meta() -> FilingMeta {
        FilingMeta {
            accession: "0009999911-26-000003".parse().ok(),
            known_on: Date::from_ymd(2026, 3, 11),
            source: None,
        }
    }

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    #[test]
    fn a_large_officer_sale_under_a_plan_fires() {
        let form = parse_fixture("01_officer_plan_sale.xml");
        let e = planned_sale_event(&form, &meta(), &PlannedSaleParams::default()).unwrap();
        assert_eq!(e.rule_id, "insider.planned_sale@1");
        assert_eq!(e.issuer, Cik(9_999_901));
        assert_eq!(e.seller, "OFFICER BRAVO");
        assert_eq!(e.units, Some(65_000.0));
        assert_eq!(e.aggregate_market_value, 2_824_614.0);
        let pct = e.percent_of_outstanding.unwrap();
        assert!(
            (pct - 65_000.0 / 65_019_350.0 * 100.0).abs() < 1e-9,
            "{pct}"
        );
        assert_eq!(e.approx_sale_date, Some(d(2026, 3, 11)));
        assert!(e.under_plan);
        assert_eq!(e.plan_adopted_on, Some(d(2025, 6, 5)));
        assert_eq!(e.sold_past_3_months, 205_000.0);
        assert_eq!(e.accession.as_deref(), Some("0009999911-26-000003"));
        assert_eq!(e.known_on, Some(d(2026, 3, 11)));
    }

    #[test]
    fn the_value_threshold_is_inclusive_and_adjustable() {
        let form = parse_fixture("01_officer_plan_sale.xml");
        let at = |min| PlannedSaleParams { min_value: min };
        assert!(planned_sale_event(&form, &meta(), &at(2_824_614.0)).is_some());
        assert!(planned_sale_event(&form, &meta(), &at(2_824_614.01)).is_none());
        assert!(planned_sale_event(&form, &meta(), &at(0.0)).is_some());
    }

    #[test]
    fn amendments_never_fire() {
        let form = parse_fixture("02_prefixed_amendment_two_classes.xml");
        assert!(form.is_amendment() && form.is_insider());
        assert!(planned_sale_event(&form, &meta(), &PlannedSaleParams::default()).is_none());
    }

    #[test]
    fn the_same_notice_as_an_original_fires_without_a_plan_and_with_no_percentage() {
        let mut form = parse_fixture("02_prefixed_amendment_two_classes.xml");
        form.submission_type = "144".into();
        let e = planned_sale_event(&form, &meta(), &PlannedSaleParams::default()).unwrap();
        assert!(!e.under_plan);
        assert_eq!(e.plan_adopted_on, None);
        assert_eq!(e.aggregate_market_value, 12_590_000.0);
        assert_eq!(e.units, Some(1_260_000.0));
        // Two share classes: no percentage rather than a misleading one.
        assert_eq!(e.percent_of_outstanding, None);
    }

    #[test]
    fn only_insider_roles_qualify() {
        let mut form = parse_fixture("01_officer_plan_sale.xml");
        form.relationships = vec!["May be deemed affiliate".into()];
        assert!(planned_sale_event(&form, &meta(), &PlannedSaleParams::default()).is_none());
        form.relationships = vec!["10% Stockholder".into()];
        assert!(planned_sale_event(&form, &meta(), &PlannedSaleParams::default()).is_some());
        form.relationships.clear();
        assert!(planned_sale_event(&form, &meta(), &PlannedSaleParams::default()).is_none());
    }

    #[test]
    fn a_notice_without_a_value_cannot_fire() {
        let mut form = parse_fixture("01_officer_plan_sale.xml");
        form.securities[0].aggregate_market_value = None;
        assert!(planned_sale_event(&form, &meta(), &PlannedSaleParams::default()).is_none());
    }

    #[test]
    fn detect_keeps_input_order_and_skips_non_events() {
        let a = parse_fixture("01_officer_plan_sale.xml");
        let b = parse_fixture("02_prefixed_amendment_two_classes.xml"); // amendment: no event
        let mut c = a.clone();
        c.seller_name = "SECOND SELLER".into();
        let events = detect_planned_sales(
            &[(a, meta()), (b, meta()), (c, meta())],
            &PlannedSaleParams::default(),
        );
        let sellers: Vec<&str> = events.iter().map(|e| e.seller.as_str()).collect();
        assert_eq!(sellers, ["OFFICER BRAVO", "SECOND SELLER"]);
    }
}
