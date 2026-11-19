//! `activist.new_13d@1` and `activist.passive_to_active@1`: large-holder
//! events from Schedule 13D filings.
//!
//! Specs: `events/specs/activist.new_13d.v1.md` and
//! `events/specs/activist.passive_to_active.v1.md`.

use oe_core::{Cik, Date, FilingMeta};
use oe_sched13::{Schedule, Schedule13};

pub const NEW_13D_RULE_ID: &str = "activist.new_13d@1";
pub const PASSIVE_TO_ACTIVE_RULE_ID: &str = "activist.passive_to_active@1";

/// What `activist.passive_to_active@1` rests on in v1.
pub const PASSIVE_TO_ACTIVE_BASIS: &str = "filer's own previouslyFiledFlag";

#[derive(Debug, Clone, PartialEq)]
pub struct ActivistParams {
    /// Smallest stated percentage of the class that makes `activist.new_13d@1` fire.
    pub min_percent: f64,
}

impl Default for ActivistParams {
    fn default() -> Self {
        ActivistParams { min_percent: 5.0 }
    }
}

/// One event of either rule; `rule_id` says which.
#[derive(Debug, Clone, PartialEq)]
pub struct ActivistEvent {
    pub rule_id: &'static str,
    pub issuer: Cik,
    pub issuer_name: String,
    pub class_title: String,
    /// Every reporting person, as named in the filing.
    pub holders: Vec<String>,
    /// The largest percentage any one reporting person states (joint filers
    /// state the same shares, so they are not added).
    pub largest_percent: Option<f64>,
    pub largest_shares: Option<f64>,
    /// At least one reporting person ticks "member of a group".
    pub is_group: bool,
    pub event_date: Option<Date>,
    pub previously_filed_13g: Option<bool>,
    /// Item 4, "Purpose of Transaction", as written; not classified.
    pub purpose: Option<String>,
    /// For `activist.passive_to_active@1`, what the event rests on.
    pub basis: Option<&'static str>,
    pub accession: Option<String>,
    /// US Eastern date the SEC accepted the filing, when known.
    pub known_on: Option<Date>,
}

fn is_original_13d(s: &Schedule13) -> bool {
    s.schedule == Schedule::D && !s.is_amendment()
}

fn event(rule_id: &'static str, s: &Schedule13, meta: &FilingMeta) -> ActivistEvent {
    ActivistEvent {
        rule_id,
        issuer: s.issuer_cik,
        issuer_name: s.issuer_name.clone(),
        class_title: s.class_title.clone(),
        holders: s.persons.iter().map(|p| p.name.clone()).collect(),
        largest_percent: s.largest_percent(),
        largest_shares: s.largest_aggregate_shares(),
        is_group: s
            .persons
            .iter()
            .any(|p| p.member_of_group.as_deref() == Some("a")),
        event_date: s.event_date,
        previously_filed_13g: s.previously_filed_13g,
        purpose: s.purpose_of_transaction.clone(),
        basis: None,
        accession: meta.accession.as_ref().map(|a| a.to_string()),
        known_on: meta.known_on,
    }
}

/// `activist.new_13d@1`: an original Schedule 13D stating at least
/// `params.min_percent` of the class.
pub fn new_13d_event(
    s: &Schedule13,
    meta: &FilingMeta,
    params: &ActivistParams,
) -> Option<ActivistEvent> {
    if !is_original_13d(s) || s.largest_percent()? < params.min_percent {
        return None;
    }
    Some(event(NEW_13D_RULE_ID, s, meta))
}

/// `activist.passive_to_active@1`: an original Schedule 13D whose cover page
/// says the holding was earlier reported on a Schedule 13G.
pub fn passive_to_active_event(s: &Schedule13, meta: &FilingMeta) -> Option<ActivistEvent> {
    if !is_original_13d(s) || s.previously_filed_13g != Some(true) {
        return None;
    }
    let mut e = event(PASSIVE_TO_ACTIVE_RULE_ID, s, meta);
    e.basis = Some(PASSIVE_TO_ACTIVE_BASIS);
    Some(e)
}

/// Events of both rules for a batch of filings, in the order given; for one
/// filing, `new_13d` comes before `passive_to_active`.
pub fn detect_activist_events(
    filings: &[(Schedule13, FilingMeta)],
    params: &ActivistParams,
) -> Vec<ActivistEvent> {
    let mut out = Vec::new();
    for (s, meta) in filings {
        out.extend(new_13d_event(s, meta, params));
        out.extend(passive_to_active_event(s, meta));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn parse_fixture(name: &str) -> Schedule13 {
        let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/sched13/synthetic")
            .join(name);
        oe_sched13::parse(&std::fs::read_to_string(path).unwrap()).unwrap()
    }

    fn meta() -> FilingMeta {
        FilingMeta {
            accession: "0009999921-26-000001".parse().ok(),
            known_on: Date::from_ymd(2026, 3, 11),
            source: None,
        }
    }

    #[test]
    fn a_new_13d_from_a_group_fires_new_13d_only() {
        let s = parse_fixture("01_new_13d_group.xml");
        let e = new_13d_event(&s, &meta(), &ActivistParams::default()).unwrap();
        assert_eq!(e.rule_id, "activist.new_13d@1");
        assert_eq!(e.issuer, Cik(9_999_901));
        assert_eq!(
            e.holders,
            [
                "Example Opportunity Fund LP",
                "Example Capital Management LLC"
            ]
        );
        assert_eq!(e.largest_percent, Some(8.4));
        assert_eq!(e.largest_shares, Some(1_250_000.0));
        assert!(e.is_group);
        assert_eq!(e.event_date, Date::from_ymd(2026, 3, 4));
        assert_eq!(e.previously_filed_13g, Some(false));
        assert!(e.purpose.as_deref().unwrap().contains("board composition"));
        assert_eq!(e.basis, None);
        assert_eq!(e.accession.as_deref(), Some("0009999921-26-000001"));
        assert_eq!(e.known_on, Date::from_ymd(2026, 3, 11));
        assert!(passive_to_active_event(&s, &meta()).is_none());
    }

    #[test]
    fn a_13d_after_a_13g_fires_both_rules() {
        let s = parse_fixture("02_13d_after_13g.xml");
        let events = detect_activist_events(&[(s, meta())], &ActivistParams::default());
        let ids: Vec<&str> = events.iter().map(|e| e.rule_id).collect();
        assert_eq!(ids, ["activist.new_13d@1", "activist.passive_to_active@1"]);
        let pa = &events[1];
        assert_eq!(pa.basis, Some("filer's own previouslyFiledFlag"));
        assert!(!pa.is_group);
        assert_eq!(pa.largest_percent, Some(9.9));
    }

    #[test]
    fn the_percent_threshold_is_inclusive_and_adjustable() {
        let s = parse_fixture("01_new_13d_group.xml");
        let at = |p| ActivistParams { min_percent: p };
        assert!(new_13d_event(&s, &meta(), &at(8.4)).is_some());
        assert!(new_13d_event(&s, &meta(), &at(8.5)).is_none());
        assert!(new_13d_event(&s, &meta(), &at(0.0)).is_some());
    }

    #[test]
    fn passive_to_active_needs_no_percentage() {
        let mut s = parse_fixture("02_13d_after_13g.xml");
        for p in &mut s.persons {
            p.percent_of_class = None;
        }
        assert!(new_13d_event(&s, &meta(), &ActivistParams::default()).is_none());
        assert!(passive_to_active_event(&s, &meta()).is_some());
    }

    #[test]
    fn amendments_and_13g_never_fire() {
        let amendment = parse_fixture("03_13d_amendment.xml");
        assert!(new_13d_event(&amendment, &meta(), &ActivistParams::default()).is_none());
        let mut flagged = amendment.clone();
        flagged.previously_filed_13g = Some(true);
        assert!(passive_to_active_event(&flagged, &meta()).is_none());

        let g = parse_fixture("04_13g_passive.xml");
        assert!(new_13d_event(&g, &meta(), &ActivistParams::default()).is_none());
        assert!(passive_to_active_event(&g, &meta()).is_none());
    }

    #[test]
    fn detect_keeps_input_order() {
        let a = parse_fixture("01_new_13d_group.xml");
        let b = parse_fixture("04_13g_passive.xml");
        let mut c = a.clone();
        c.issuer_name = "SECOND ISSUER".into();
        let events = detect_activist_events(
            &[(a, meta()), (b, meta()), (c, meta())],
            &ActivistParams::default(),
        );
        let names: Vec<&str> = events.iter().map(|e| e.issuer_name.as_str()).collect();
        assert_eq!(names, ["SYNTHETIC WIDGETS INC", "SECOND ISSUER"]);
    }
}
