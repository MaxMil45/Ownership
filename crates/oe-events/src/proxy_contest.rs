//! `activist.proxy_contest@1`: someone other than the board is soliciting
//! shareholders' votes at a company.
//!
//! Spec: `events/specs/activist.proxy_contest.v1.md`.

use oe_core::{Cik, Date, Party};
use std::collections::BTreeMap;

pub const RULE_ID: &str = "activist.proxy_contest@1";

/// Contested-solicitation form types the rule reads.
pub const CONTEST_FORMS: &[&str] = &[
    "PREC14A", "PREC14C", "DEFC14A", "DEFC14C", "PREN14A", "PRRN14A", "DEFN14A", "DFAN14A",
];

/// Of those, the proxy statements (everything but additional materials).
const PROXY_STATEMENTS: &[&str] = &[
    "PREC14A", "PREC14C", "DEFC14A", "DEFC14C", "PREN14A", "PRRN14A", "DEFN14A",
];

/// The company's own contested proxy statements.
const CONTESTED_PROXY: &[&str] = &["PREC14A", "PREC14C", "DEFC14A", "DEFC14C"];

pub fn is_contest_form(form: &str) -> bool {
    CONTEST_FORMS.contains(&form)
}

/// One contest filing, as its SGML header describes it.
#[derive(Debug, Clone, PartialEq)]
pub struct ContestFiling {
    pub form: String,
    pub accession: String,
    /// US Eastern date the SEC accepted it.
    pub known_on: Option<Date>,
    pub subject: Party,
    pub filed_by: Vec<Party>,
}

impl ContestFiling {
    /// Filers who are not the subject company.
    pub fn challengers(&self) -> impl Iterator<Item = &Party> {
        self.filed_by
            .iter()
            .filter(move |p| p.cik != self.subject.cik)
    }

    /// Filed by the company itself (or with no `FILED-BY` at all).
    pub fn by_company(&self) -> bool {
        self.challengers().next().is_none()
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ProxyContestEvent {
    pub rule_id: &'static str,
    pub issuer: Cik,
    pub issuer_name: String,
    /// Every challenger in the batch, in order of first appearance.
    pub challengers: Vec<Party>,
    pub first_known_on: Option<Date>,
    pub last_known_on: Option<Date>,
    /// A challenger filed a proxy statement (not just additional materials).
    pub challenger_proxy_statement: bool,
    /// The company filed a contested proxy statement under its own CIK.
    pub company_contested_proxy: bool,
    /// The batch's contest filings for this company, in the order given.
    pub filings: Vec<ContestFiling>,
}

/// One event per subject company with at least one contest filing in `filings`.
/// Filings of other form types are ignored. Events come in issuer-CIK order.
pub fn detect_proxy_contests(filings: &[ContestFiling]) -> Vec<ProxyContestEvent> {
    let mut by_issuer: BTreeMap<Cik, ProxyContestEvent> = BTreeMap::new();
    for f in filings.iter().filter(|f| is_contest_form(&f.form)) {
        let e = by_issuer
            .entry(f.subject.cik)
            .or_insert_with(|| ProxyContestEvent {
                rule_id: RULE_ID,
                issuer: f.subject.cik,
                issuer_name: f.subject.name.clone(),
                challengers: Vec::new(),
                first_known_on: None,
                last_known_on: None,
                challenger_proxy_statement: false,
                company_contested_proxy: false,
                filings: Vec::new(),
            });
        for c in f.challengers() {
            if !e.challengers.iter().any(|x| x.cik == c.cik) {
                e.challengers.push(c.clone());
            }
        }
        if let Some(d) = f.known_on {
            e.first_known_on = Some(e.first_known_on.map_or(d, |x| x.min(d)));
            e.last_known_on = Some(e.last_known_on.map_or(d, |x| x.max(d)));
        }
        let form = f.form.as_str();
        if f.by_company() {
            e.company_contested_proxy |= CONTESTED_PROXY.contains(&form);
        } else {
            e.challenger_proxy_statement |= PROXY_STATEMENTS.contains(&form);
        }
        e.filings.push(f.clone());
    }
    by_issuer.into_values().collect()
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

    fn filing(form: &str, day: u32, subject: &Party, filed_by: &[&Party]) -> ContestFiling {
        ContestFiling {
            form: form.into(),
            accession: format!("0000000000-26-{day:06}"),
            known_on: Date::from_ymd(2026, 4, day),
            subject: subject.clone(),
            filed_by: filed_by.iter().map(|p| (*p).clone()).collect(),
        }
    }

    #[test]
    fn a_challenger_and_the_company_make_one_event() {
        // Shaped like lululemon 2026: the company's PREC14A, a founder's DFAN14A.
        let co = party(1, "EXAMPLE APPAREL INC");
        let founder = party(2, "Founder Frank");
        let events = detect_proxy_contests(&[
            filing("DFAN14A", 10, &co, &[&founder]),
            filing("PREC14A", 3, &co, &[&co]),
            filing("DFAN14A", 12, &co, &[&founder]),
        ]);
        assert_eq!(events.len(), 1);
        let e = &events[0];
        assert_eq!(e.rule_id, "activist.proxy_contest@1");
        assert_eq!(e.issuer, Cik(1));
        assert_eq!(e.challengers, [founder]);
        assert_eq!(e.first_known_on, Date::from_ymd(2026, 4, 3));
        assert_eq!(e.last_known_on, Date::from_ymd(2026, 4, 12));
        assert!(e.company_contested_proxy);
        assert!(
            !e.challenger_proxy_statement,
            "only additional materials so far"
        );
        assert_eq!(e.filings.len(), 3);
    }

    #[test]
    fn the_form_type_does_not_say_who_filed() {
        // Shaped like Fermi 2026: a PREC14A and a PREN14A filed by a shareholder.
        let co = party(1, "EXAMPLE ENERGY INC");
        let holder = party(3, "Holder Hana");
        let e = &detect_proxy_contests(&[
            filing("PREC14A", 5, &co, &[&holder]),
            filing("PREN14A", 5, &co, &[&holder]),
        ])[0];
        assert!(e.challenger_proxy_statement);
        assert!(
            !e.company_contested_proxy,
            "the company filed nothing itself"
        );
        assert_eq!(e.challengers, [holder]);
    }

    #[test]
    fn rival_bidders_are_each_a_challenger() {
        // Shaped like Two Harbors 2026: two bidders soliciting against the board.
        let co = party(1, "EXAMPLE REIT");
        let (a, b) = (party(4, "Bidder A"), party(5, "Bidder B"));
        let e = &detect_proxy_contests(&[
            filing("DFAN14A", 1, &co, &[&a]),
            filing("DFAN14A", 2, &co, &[&b]),
            filing("DEFC14A", 3, &co, &[&b]),
        ])[0];
        assert_eq!(e.challengers, [a, b]);
        assert!(e.challenger_proxy_statement);
    }

    #[test]
    fn a_company_only_contested_proxy_still_fires_with_no_challenger_named() {
        let co = party(1, "EXAMPLE PHARMA");
        let e = &detect_proxy_contests(&[filing("DEFC14A", 8, &co, &[&co])])[0];
        assert!(e.challengers.is_empty() && e.company_contested_proxy);
        // No FILED-BY at all counts as the company too.
        let e = &detect_proxy_contests(&[filing("PREC14A", 8, &co, &[])])[0];
        assert!(e.company_contested_proxy && e.challengers.is_empty());
    }

    #[test]
    fn other_forms_are_ignored_and_issuers_stay_apart() {
        let (x, y) = (party(7, "X CORP"), party(6, "Y CORP"));
        let fund = party(9, "Fund");
        let events = detect_proxy_contests(&[
            filing("DEFA14A", 1, &x, &[&x]),
            filing("PX14A6G", 1, &x, &[&fund]),
            filing("DFAN14A", 2, &x, &[&fund]),
            filing("DFAN14A", 3, &y, &[&fund]),
        ]);
        let issuers: Vec<u64> = events.iter().map(|e| e.issuer.0).collect();
        assert_eq!(issuers, [6, 7], "issuer-CIK order, one event each");
        assert!(events.iter().all(|e| e.filings.len() == 1));
        assert!(detect_proxy_contests(&[]).is_empty());
    }
}
