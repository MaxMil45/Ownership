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
    /// For `insider.planned_sale@2`: the seller's history at this company.
    pub history: Option<SellerHistory>,
}

/// Spec: `events/specs/insider.planned_sale.v2.md`. Needs the ledger.
pub const RULE_ID_V2: &str = "insider.planned_sale@2";

/// The look-back for a seller's earlier notices, in days.
pub const HISTORY_DAYS: i32 = 365;

/// A seller with notices in at least this many of the 12 months before is
/// routine (measured on the ledger: 13% of large notices).
pub const ROUTINE_MIN_MONTHS: usize = 6;

/// A price per unit times the units outstanding above this is a filing
/// error. Measured on the ledger's notices over $2 billion: real ones imply at
/// most $2.9 trillion (Amazon); the errors imply $15 trillion to $3,700
/// trillion (a total typed in place of a price, or extra digits).
pub const MAX_PLAUSIBLE_COMPANY_VALUE: f64 = 10_000_000_000_000.0;

/// Whether every securities block's value is believable as filed: a price per
/// unit of at most `oe_core::MAX_PLAUSIBLE_PRICE`, and, for a price over
/// [`SUSPECT_PRICE`], a company value (price times units outstanding) of at
/// most [`MAX_PLAUSIBLE_COMPANY_VALUE`]. Blocks without units or a value are
/// not implausible.
pub fn plausible_notice(form: &Form144) -> bool {
    form.securities.iter().all(|s| {
        let (Some(value), Some(units)) = (s.aggregate_market_value, s.units) else {
            return true;
        };
        if units <= 0.0 {
            return true;
        }
        let price = value / units;
        // A huge company value with an ordinary price means the outstanding
        // count is the typo, not the sale (NVIDIA, June 2025: 244 billion
        // shares typed for 24.4 billion), so only a high price is judged.
        price <= oe_core::MAX_PLAUSIBLE_PRICE
            && (price <= SUSPECT_PRICE
                || s.units_outstanding
                    .is_none_or(|o| price * o <= MAX_PLAUSIBLE_COMPANY_VALUE))
    })
}

/// Over this price per unit, a notice's company value is checked: almost no
/// listed share trades higher (Berkshire Hathaway class A, about $700,000, is
/// the exception, and its company value passes).
pub const SUSPECT_PRICE: f64 = 10_000.0;

/// A Form 144 notice as `insider.planned_sale@2` needs it from the ledger.
#[derive(Debug, Clone, PartialEq)]
pub struct EarlierNotice {
    pub issuer: Cik,
    pub seller: String,
    pub accession: String,
    /// US Eastern date of acceptance.
    pub known_on: Date,
}

/// What the ledger says about a seller before this notice.
#[derive(Debug, Clone, PartialEq)]
pub struct SellerHistory {
    /// Original notices (not 144/A) for the same company in the
    /// [`HISTORY_DAYS`] before, on earlier days.
    pub notices_past_year: usize,
    /// Distinct calendar months among them.
    pub months_past_year: usize,
    /// The latest of them.
    pub previous_notice: Option<(String, Date)>,
    /// Notices in at least [`ROUTINE_MIN_MONTHS`] of those months.
    pub routine: bool,
}

/// A seller's name with only its letters and digits, upper case.
pub fn seller_key(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// The seller's history at `issuer` before `known_on`, from `earlier`.
pub fn seller_history(
    issuer: Cik,
    seller: &str,
    known_on: Date,
    earlier: &[EarlierNotice],
) -> SellerHistory {
    let key = seller_key(seller);
    let mut mine: Vec<&EarlierNotice> = earlier
        .iter()
        .filter(|n| n.issuer == issuer && !key.is_empty() && seller_key(&n.seller) == key)
        .filter(|n| n.known_on < known_on && n.known_on.days_until(known_on) <= HISTORY_DAYS)
        .collect();
    mine.sort_by(|a, b| (a.known_on, &a.accession).cmp(&(b.known_on, &b.accession)));
    mine.dedup_by(|a, b| a.accession == b.accession);
    let mut months: Vec<(i32, u32)> = mine
        .iter()
        .map(|n| (n.known_on.year(), n.known_on.month()))
        .collect();
    months.dedup();
    SellerHistory {
        notices_past_year: mine.len(),
        months_past_year: months.len(),
        previous_notice: mine.last().map(|n| (n.accession.clone(), n.known_on)),
        routine: months.len() >= ROUTINE_MIN_MONTHS,
    }
}

/// `insider.planned_sale@2`: the events of `@1`, each with the seller's
/// history from the ledger (`earlier`) and the batch's own notices.
pub fn detect_planned_sales_with_ledger(
    filings: &[(Form144, FilingMeta)],
    params: &PlannedSaleParams,
    earlier: &[EarlierNotice],
) -> Vec<PlannedSaleEvent> {
    let from_batch = filings
        .iter()
        .filter(|(f, _)| !f.is_amendment())
        .filter_map(|(f, m)| {
            Some(EarlierNotice {
                issuer: f.issuer_cik,
                seller: f.seller_name.clone(),
                accession: m.accession.as_ref()?.to_string(),
                known_on: m.known_on?,
            })
        });
    // Each seller's notices at each company, so every event looks at its own.
    let mut by_seller: std::collections::HashMap<(Cik, String), Vec<EarlierNotice>> =
        std::collections::HashMap::new();
    for n in earlier.iter().cloned().chain(from_batch) {
        by_seller
            .entry((n.issuer, seller_key(&n.seller)))
            .or_default()
            .push(n);
    }
    filings
        .iter()
        .filter(|(form, _)| plausible_notice(form))
        .filter_map(|(form, meta)| {
            let mut e = planned_sale_event(form, meta, params)?;
            e.rule_id = RULE_ID_V2;
            let mine = by_seller
                .get(&(form.issuer_cik, seller_key(&form.seller_name)))
                .map_or(&[][..], Vec::as_slice);
            e.history = meta
                .known_on
                .map(|d| seller_history(form.issuer_cik, &form.seller_name, d, mine));
            Some(e)
        })
        .collect()
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
        history: None,
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

    fn notice(seller: &str, accession: &str, day: Date) -> EarlierNotice {
        EarlierNotice {
            issuer: Cik(9_999_901),
            seller: seller.into(),
            accession: accession.into(),
            known_on: day,
        }
    }

    #[test]
    fn v2_counts_the_sellers_earlier_notices_in_the_year_before() {
        let form = parse_fixture("01_officer_plan_sale.xml"); // OFFICER BRAVO, 2026-03-11
        let ledger = [
            notice("Officer Bravo", "a-1", d(2025, 3, 10)), // over a year before
            notice("OFFICER  BRAVO", "a-2", d(2025, 3, 11)), // exactly 365 days
            notice("officer bravo", "a-3", d(2025, 9, 1)),
            notice("officer bravo", "a-4", d(2025, 9, 20)), // same month as a-3
            notice("OFFICER BRAVO", "a-5", d(2026, 3, 11)), // same day: not earlier
            notice("SOMEONE ELSE", "a-6", d(2026, 1, 5)),
        ];
        let events = detect_planned_sales_with_ledger(
            &[(form, meta())],
            &PlannedSaleParams::default(),
            &ledger,
        );
        let e = &events[0];
        assert_eq!(e.rule_id, "insider.planned_sale@2");
        let h = e.history.as_ref().unwrap();
        assert_eq!(h.notices_past_year, 3);
        assert_eq!(h.months_past_year, 2);
        assert_eq!(h.previous_notice, Some(("a-4".into(), d(2025, 9, 20))));
        assert!(!h.routine);
        // Same as v1 otherwise.
        assert_eq!(e.aggregate_market_value, 2_824_614.0);
    }

    #[test]
    fn v2_six_months_with_notices_is_routine_and_none_is_a_first() {
        let form = parse_fixture("01_officer_plan_sale.xml");
        let monthly: Vec<EarlierNotice> = (4..=9)
            .map(|m| notice("OFFICER BRAVO", &format!("m-{m}"), d(2025, m, 15)))
            .collect();
        let run = |ledger: &[EarlierNotice]| {
            detect_planned_sales_with_ledger(
                &[(form.clone(), meta())],
                &PlannedSaleParams::default(),
                ledger,
            )[0]
            .history
            .clone()
            .unwrap()
        };
        assert!(run(&monthly).routine);
        assert!(!run(&monthly[1..]).routine, "five months is not routine");
        let first = run(&[]);
        assert_eq!((first.notices_past_year, first.previous_notice), (0, None));
    }

    #[test]
    fn v2_the_batch_counts_as_history_once_and_amendments_never() {
        let a = parse_fixture("01_officer_plan_sale.xml");
        let older = FilingMeta {
            accession: "0009999911-26-000001".parse().ok(),
            known_on: Date::from_ymd(2026, 2, 2),
            source: None,
        };
        let mut amended = a.clone();
        amended.submission_type = "144/A".into();
        let amended_meta = FilingMeta {
            accession: "0009999911-26-000002".parse().ok(),
            known_on: Date::from_ymd(2026, 2, 20),
            source: None,
        };
        // The older notice is also in the ledger: counted once.
        let ledger = [notice(
            "OFFICER BRAVO",
            "0009999911-26-000001",
            d(2026, 2, 2),
        )];
        let events = detect_planned_sales_with_ledger(
            &[(a.clone(), older), (amended, amended_meta), (a, meta())],
            &PlannedSaleParams::default(),
            &ledger,
        );
        assert_eq!(events.len(), 2, "the amendment fires nothing");
        assert_eq!(events[0].history.as_ref().unwrap().notices_past_year, 0);
        assert_eq!(events[1].history.as_ref().unwrap().notices_past_year, 1);
        // Without an acceptance date there is no point in time for a history.
        let undated = FilingMeta {
            known_on: None,
            ..meta()
        };
        let e = &detect_planned_sales_with_ledger(
            &[(parse_fixture("01_officer_plan_sale.xml"), undated)],
            &PlannedSaleParams::default(),
            &ledger,
        )[0];
        assert_eq!(e.history, None);
    }

    #[test]
    fn v2_does_not_fire_on_amounts_no_company_could_have() {
        let run = |units: f64, value: f64, outstanding: Option<f64>| {
            let mut form = parse_fixture("01_officer_plan_sale.xml");
            let s = &mut form.securities[0];
            (s.units, s.aggregate_market_value, s.units_outstanding) =
                (Some(units), Some(value), outstanding);
            let v2 = detect_planned_sales_with_ledger(
                &[(form.clone(), meta())],
                &PlannedSaleParams::default(),
                &[],
            );
            let v1 = planned_sale_event(&form, &meta(), &PlannedSaleParams::default());
            assert!(v1.is_some(), "v1 takes any amount as filed");
            !v2.is_empty()
        };
        // Shaped like the Tenable notice: 12,000 shares "worth $4.1 billion",
        // 104.5 million outstanding, so a $35.7 trillion company.
        assert!(!run(12_000.0, 4.1e9, Some(104_507_623.0)));
        // Over $2 million a share, outstanding count or not.
        assert!(!run(655.0, 31.83e9, None));
        // Real: Amazon, 25 million shares at $217 of 10.6 billion outstanding.
        assert!(run(25_000_000.0, 5.43e9, Some(10.6e9)));
        // A Berkshire class A share is about $700,000: plausible.
        assert!(run(2_000.0, 1.4e9, Some(1.44e6)));
        // Shaped like NVIDIA, June 2025: an ordinary price, the outstanding
        // count typed ten times too large. The sale is real.
        assert!(run(1_000_000.0, 142_764_640.0, Some(244e9)));
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
