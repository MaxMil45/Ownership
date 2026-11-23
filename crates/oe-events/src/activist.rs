//! `activist.new_13d@1` and `activist.passive_to_active@1`: large-holder
//! events from Schedule 13D filings.
//!
//! Specs: `events/specs/activist.new_13d.v1.md` and
//! `events/specs/activist.passive_to_active.v1.md`.

use oe_core::{Cik, Date, FilingMeta};
use oe_sched13::{Schedule, Schedule13};

pub const NEW_13D_RULE_ID: &str = "activist.new_13d@1";
pub const PASSIVE_TO_ACTIVE_RULE_ID: &str = "activist.passive_to_active@1";
/// Spec: `events/specs/activist.passive_to_active.v2.md`. Needs the ledger.
pub const PASSIVE_TO_ACTIVE_V2_RULE_ID: &str = "activist.passive_to_active@2";

/// What `activist.passive_to_active@1` rests on in v1.
pub const PASSIVE_TO_ACTIVE_BASIS: &str = "filer's own previouslyFiledFlag";

/// Under this stated percentage, a 13G reports that the holder is no longer a
/// 5% holder (the usual exit amendment).
pub const EXITED_BELOW_PERCENT: f64 = 5.0;

/// A Schedule 13G or 13G/A the ledger holds, as `activist.passive_to_active@2`
/// needs it.
#[derive(Debug, Clone, PartialEq)]
pub struct Earlier13G {
    pub issuer: Cik,
    pub accession: String,
    /// US Eastern date the SEC accepted it.
    pub known_on: Date,
    /// The filer the SEC recorded (first submitting filer).
    pub filer: Cik,
    /// Every reporting person, as named.
    pub holders: Vec<String>,
    pub largest_percent: Option<f64>,
}

/// A name with only its letters and digits, upper case: "Elliott Investment
/// Management L.P." and "ELLIOTT INVESTMENT MANAGEMENT LP" are the same.
fn name_key(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_uppercase())
        .collect()
}

/// The latest 13G in `earlier` that the 13D's holder filed about the same
/// issuer before the 13D was accepted (on an earlier US Eastern date): same
/// filer CIK as one of the 13D's filers or reporting persons, or a reporting
/// person of the same name.
pub fn find_earlier_13g<'a>(
    s: &Schedule13,
    known_on: Date,
    earlier: &'a [Earlier13G],
) -> Option<&'a Earlier13G> {
    let ciks: Vec<Cik> = s
        .filer_ciks
        .iter()
        .copied()
        .chain(s.persons.iter().filter_map(|p| p.cik))
        .collect();
    let names: Vec<String> = s
        .persons
        .iter()
        .map(|p| name_key(&p.name))
        .filter(|k| !k.is_empty())
        .collect();
    earlier
        .iter()
        .filter(|g| g.issuer == s.issuer_cik && g.known_on < known_on)
        .filter(|g| {
            ciks.contains(&g.filer) || g.holders.iter().any(|h| names.contains(&name_key(h)))
        })
        .max_by(|a, b| {
            a.known_on
                .cmp(&b.known_on)
                .then(a.accession.cmp(&b.accession))
        })
}

/// `activist.passive_to_active@2`: an original Schedule 13D whose cover page
/// says the holding was earlier reported on a 13G, or whose holder the ledger
/// shows filing a 13G about the same company before. `earlier` holds the
/// ledger's 13Gs for the issuers concerned.
pub fn passive_to_active_v2_event(
    s: &Schedule13,
    meta: &FilingMeta,
    earlier: &[Earlier13G],
) -> Option<ActivistEvent> {
    if !is_original_13d(s) {
        return None;
    }
    let flagged = s.previously_filed_13g == Some(true);
    // A latest 13G stating under 5% says the holder had left; a 13D after
    // that is a new stake, not a passive holder turning active.
    let found = meta
        .known_on
        .and_then(|d| find_earlier_13g(s, d, earlier))
        .filter(|g| g.largest_percent.is_none_or(|p| p >= EXITED_BELOW_PERCENT));
    let basis = match (flagged, found.is_some()) {
        (true, true) => "filer's own previouslyFiledFlag and an earlier 13G in the ledger",
        (true, false) => PASSIVE_TO_ACTIVE_BASIS,
        (false, true) => "earlier 13G in the ledger (the filer did not flag it)",
        (false, false) => return None,
    };
    let mut e = event(PASSIVE_TO_ACTIVE_V2_RULE_ID, s, meta);
    e.basis = Some(basis);
    if let Some(g) = found {
        e.earlier_13g = Some((g.accession.clone(), g.known_on, g.largest_percent));
    }
    Some(e)
}

/// Like [`detect_activist_events`], with `activist.passive_to_active@2` in
/// place of v1.
pub fn detect_activist_events_with_ledger(
    filings: &[(Schedule13, FilingMeta)],
    params: &ActivistParams,
    earlier: &[Earlier13G],
) -> Vec<ActivistEvent> {
    let mut out = Vec::new();
    for (s, meta) in filings {
        out.extend(new_13d_event(s, meta, params));
        out.extend(passive_to_active_v2_event(s, meta, earlier));
    }
    out
}

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
    /// For `activist.passive_to_active@2`, the latest earlier 13G the ledger
    /// found: accession, US Eastern acceptance date, largest stated percent.
    pub earlier_13g: Option<(String, Date, Option<f64>)>,
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
        earlier_13g: None,
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

    fn g(accession: &str, day: u32, filer: u64, holders: &[&str]) -> Earlier13G {
        Earlier13G {
            issuer: Cik(9_999_901),
            accession: accession.into(),
            known_on: Date::from_ymd(2026, 3, day).unwrap(),
            filer: Cik(filer),
            holders: holders.iter().map(|h| h.to_string()).collect(),
            largest_percent: Some(5.6),
        }
    }

    #[test]
    fn v2_finds_an_unflagged_earlier_13g_by_name_or_cik() {
        // Flag says no; the ledger has the same fund's 13G, named differently.
        let s = parse_fixture("01_new_13d_group.xml");
        assert_eq!(s.previously_filed_13g, Some(false));
        let ledger = [
            g(
                "0000000001-26-000001",
                2,
                42,
                &["EXAMPLE OPPORTUNITY FUND, L.P."],
            ),
            g(
                "0000000001-26-000002",
                9,
                42,
                &["Example Opportunity Fund LP"],
            ),
        ];
        let e = passive_to_active_v2_event(&s, &meta(), &ledger).unwrap();
        assert_eq!(e.rule_id, "activist.passive_to_active@2");
        assert_eq!(
            e.basis,
            Some("earlier 13G in the ledger (the filer did not flag it)")
        );
        // The latest earlier one.
        let (acc, day, pct) = e.earlier_13g.unwrap();
        assert_eq!(acc, "0000000001-26-000002");
        assert_eq!(day, Date::from_ymd(2026, 3, 9).unwrap());
        assert_eq!(pct, Some(5.6));

        // By filer CIK, whatever the names.
        // A latest 13G under 5% is an exit: no event, even with an older
        // 13G above 5% before it.
        let mut exit = g(
            "0000000001-26-000009",
            10,
            42,
            &["Example Opportunity Fund LP"],
        );
        exit.largest_percent = Some(0.9);
        let with_exit = [ledger[1].clone(), exit.clone()];
        assert!(passive_to_active_v2_event(&s, &meta(), &with_exit).is_none());
        // Unstated is not an exit; exactly 5% is still a 5% holder.
        exit.largest_percent = None;
        assert!(passive_to_active_v2_event(&s, &meta(), &[exit.clone()]).is_some());
        exit.largest_percent = Some(5.0);
        assert!(passive_to_active_v2_event(&s, &meta(), &[exit]).is_some());

        let mut by_cik = s.clone();
        by_cik.filer_ciks = vec![Cik(42)];
        let other_name = [g("0000000001-26-000003", 5, 42, &["Someone Else"])];
        assert!(passive_to_active_v2_event(&by_cik, &meta(), &other_name).is_some());
        assert!(passive_to_active_v2_event(&s, &meta(), &other_name).is_none());
    }

    #[test]
    fn v2_never_looks_at_13gs_from_the_same_day_or_later_or_other_issuers() {
        let s = parse_fixture("01_new_13d_group.xml");
        let name = ["Example Opportunity Fund LP"];
        // meta() is accepted on 2026-03-11.
        let same_day = [g("0000000001-26-000004", 11, 1, &name)];
        let later = [g("0000000001-26-000005", 12, 1, &name)];
        let mut elsewhere = g("0000000001-26-000006", 2, 1, &name);
        elsewhere.issuer = Cik(1);
        for ledger in [&same_day[..], &later[..], &[elsewhere][..]] {
            assert!(passive_to_active_v2_event(&s, &meta(), ledger).is_none());
        }
        // An undated 13D cannot be placed in time: only its flag counts.
        let undated = FilingMeta {
            known_on: None,
            ..meta()
        };
        let before = [g("0000000001-26-000007", 2, 1, &name)];
        assert!(passive_to_active_v2_event(&s, &undated, &before).is_none());
    }

    #[test]
    fn v2_keeps_v1_s_flag_and_says_when_both_agree() {
        let s = parse_fixture("02_13d_after_13g.xml");
        let alone = passive_to_active_v2_event(&s, &meta(), &[]).unwrap();
        assert_eq!(alone.basis, Some(PASSIVE_TO_ACTIVE_BASIS));
        assert_eq!(alone.earlier_13g, None);
        let names: Vec<&str> = s.persons.iter().map(|p| p.name.as_str()).collect();
        let mut earlier = g("0000000001-26-000008", 3, 1, &names);
        earlier.issuer = s.issuer_cik;
        let both = passive_to_active_v2_event(&s, &meta(), &[earlier]).unwrap();
        assert_eq!(
            both.basis,
            Some("filer's own previouslyFiledFlag and an earlier 13G in the ledger")
        );
        // Amendments and 13Gs never fire, ledger or not.
        let amendment = parse_fixture("03_13d_amendment.xml");
        let passive = parse_fixture("04_13g_passive.xml");
        assert!(passive_to_active_v2_event(&amendment, &meta(), &[]).is_none());
        assert!(passive_to_active_v2_event(&passive, &meta(), &[]).is_none());
        // With the ledger, v2 takes v1's place and new_13d is unchanged.
        let ids: Vec<&str> =
            detect_activist_events_with_ledger(&[(s, meta())], &ActivistParams::default(), &[])
                .iter()
                .map(|e| e.rule_id)
                .collect();
        assert_eq!(ids, ["activist.new_13d@1", "activist.passive_to_active@2"]);
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
