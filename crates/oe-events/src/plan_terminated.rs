//! `insider.plan_terminated@1`: a director or officer ended a Rule 10b5-1
//! trading plan, according to the Item 408 disclosure in a 10-Q or 10-K.
//!
//! Spec: `events/specs/insider.plan_terminated.v1.md`.

use oe_core::{Cik, Date, FilingMeta};
use oe_item408::{Item408Doc, TradingArrangement};

pub const RULE_ID: &str = "insider.plan_terminated@1";

#[derive(Debug, Clone, PartialEq)]
pub struct PlanTerminatedEvent {
    pub rule_id: &'static str,
    pub issuer: Option<Cik>,
    pub issuer_name: Option<String>,
    pub person: Option<String>,
    pub title: Option<String>,
    /// `ecd:TrdArrTerminationDate` when the filing tags one and it is not after
    /// the end of the period (a plan cannot have been terminated in the future);
    /// never guessed from prose.
    pub termination_date: Option<Date>,
    /// A tagged termination date that was rejected for being after the period
    /// end. Cryoport tagged its plan's scheduled end (12 March 2027) in a report
    /// for the quarter ended 30 September 2026; the real date was only in the text.
    pub rejected_termination_date: Option<Date>,
    /// End of the reporting period: the termination happened by this date.
    pub period_end: Option<Date>,
    /// The same person adopted a plan in the same filing.
    pub replaced_in_filing: bool,
    pub terms_text: Option<String>,
    /// `10-Q`, `10-K`, ... as the report states it.
    pub form: Option<String>,
    pub accession: Option<String>,
    /// US Eastern date the SEC accepted the report, when known.
    pub known_on: Option<Date>,
}

/// One event per arrangement the report tags as terminated (flag `fixed-true`).
pub fn detect_plan_terminations(doc: &Item408Doc, meta: &FilingMeta) -> Vec<PlanTerminatedEvent> {
    doc.arrangements
        .iter()
        .filter(|a| a.rule_10b5_1_terminated)
        .map(|a| {
            let period_end = a.period_end.or(doc.period_end);
            let (termination_date, rejected_termination_date) =
                check_termination_date(a.termination_date, period_end, meta.known_on);
            PlanTerminatedEvent {
                rule_id: RULE_ID,
                issuer: doc.cik,
                issuer_name: doc.registrant.clone(),
                person: a.person.clone(),
                title: a.title.clone(),
                termination_date,
                rejected_termination_date,
                period_end,
                replaced_in_filing: replaced(a, &doc.arrangements),
                terms_text: a.terms_text.clone(),
                form: doc.document_type.clone(),
                accession: meta.accession.as_ref().map(|x| x.to_string()),
                known_on: meta.known_on,
            }
        })
        .collect()
}

/// Splits a tagged termination date into (accepted, rejected). It is rejected
/// when it is after the end of the reporting period, or, if that is unknown,
/// after the day the report was accepted.
fn check_termination_date(
    tagged: Option<Date>,
    period_end: Option<Date>,
    known_on: Option<Date>,
) -> (Option<Date>, Option<Date>) {
    let Some(date) = tagged else {
        return (None, None);
    };
    match period_end.or(known_on) {
        Some(limit) if date > limit => (None, Some(date)),
        _ => (Some(date), None),
    }
}

/// Whether the person behind `terminated` also adopted a plan in this report.
fn replaced(terminated: &TradingArrangement, all: &[TradingArrangement]) -> bool {
    // A person tagged on a single arrangement that is both adopted and terminated.
    if terminated.rule_10b5_1_adopted {
        return true;
    }
    let Some(member) = terminated.individual_member.as_deref() else {
        return false;
    };
    all.iter()
        .any(|o| o.individual_member.as_deref() == Some(member) && o.rule_10b5_1_adopted)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    fn meta() -> FilingMeta {
        FilingMeta {
            accession: "0009999901-26-000048".parse().ok(),
            known_on: Date::from_ymd(2026, 11, 2),
            source: None,
        }
    }

    const REPORT: &str = r#"
<ix:nonNumeric contextRef="c-0" name="dei:DocumentType">10-Q</ix:nonNumeric>
<ix:nonNumeric contextRef="c-0" name="dei:EntityRegistrantName">Example Corp</ix:nonNumeric>
<ix:nonNumeric contextRef="c-0" name="dei:EntityCentralIndexKey">0009999901</ix:nonNumeric>
<xbrli:context id="c-jo"><xbrli:entity><xbrli:segment><xbrldi:explicitMember dimension="ecd:IndividualAxis">ex:JoMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity><xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
<xbrli:context id="c-al-old"><xbrli:entity><xbrli:segment><xbrldi:explicitMember dimension="ecd:TradingArrAxis">ex:AlOldPlan</xbrldi:explicitMember><xbrldi:explicitMember dimension="ecd:IndividualAxis">ex:AlMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity><xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
<xbrli:context id="c-al-new"><xbrli:entity><xbrli:segment><xbrldi:explicitMember dimension="ecd:TradingArrAxis">ex:AlNewPlan</xbrldi:explicitMember><xbrldi:explicitMember dimension="ecd:IndividualAxis">ex:AlMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity><xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
<xbrli:context id="c-co"><xbrli:entity/><xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
<ix:nonNumeric contextRef="c-jo" name="ecd:TrdArrIndName">Jo Jones</ix:nonNumeric>
<ix:nonNumeric contextRef="c-jo" name="ecd:TrdArrIndTitle">Director</ix:nonNumeric>
<ix:nonNumeric contextRef="c-jo" name="ecd:Rule10b51ArrTrmntdFlag" format="ixt:fixed-true">terminated</ix:nonNumeric>
<ix:nonNumeric contextRef="c-jo" name="ecd:MtrlTermsOfTrdArrTextBlock">Jo ended the plan.</ix:nonNumeric>
<ix:nonNumeric contextRef="c-al-old" name="ecd:TrdArrIndName">Al Adams</ix:nonNumeric>
<ix:nonNumeric contextRef="c-al-old" name="ecd:Rule10b51ArrTrmntdFlag" format="ixt:fixed-true">terminated</ix:nonNumeric>
<ix:nonNumeric contextRef="c-al-old" name="ecd:TrdArrTerminationDate">August 6, 2026</ix:nonNumeric>
<ix:nonNumeric contextRef="c-al-new" name="ecd:TrdArrIndName">Mr. Adams</ix:nonNumeric>
<ix:nonNumeric contextRef="c-al-new" name="ecd:TrdArrIndTitle">CEO</ix:nonNumeric>
<ix:nonNumeric contextRef="c-al-new" name="ecd:Rule10b51ArrAdoptedFlag" format="ixt:fixed-true">adopted</ix:nonNumeric>
<ix:nonNumeric contextRef="c-co" name="ecd:Rule10b51ArrTrmntdFlag" format="ixt:fixed-false">terminated</ix:nonNumeric>
"#;

    #[test]
    fn fires_for_each_terminated_plan_and_only_those() {
        let doc = oe_item408::extract(REPORT);
        let events = detect_plan_terminations(&doc, &meta());
        assert_eq!(events.len(), 2);

        let jo = events
            .iter()
            .find(|e| e.person.as_deref() == Some("Jo Jones"))
            .unwrap();
        assert_eq!(jo.rule_id, "insider.plan_terminated@1");
        assert_eq!(jo.issuer, Some(Cik(9_999_901)));
        assert_eq!(jo.issuer_name.as_deref(), Some("Example Corp"));
        assert_eq!(jo.title.as_deref(), Some("Director"));
        assert_eq!(jo.termination_date, None, "no date tag, so no date");
        assert_eq!(jo.period_end, Some(d(2026, 9, 30)));
        assert!(!jo.replaced_in_filing);
        assert_eq!(jo.terms_text.as_deref(), Some("Jo ended the plan."));
        assert_eq!(jo.form.as_deref(), Some("10-Q"));
        assert_eq!(jo.accession.as_deref(), Some("0009999901-26-000048"));
        assert_eq!(jo.known_on, Some(d(2026, 11, 2)));

        let al = events
            .iter()
            .find(|e| e.person.as_deref() == Some("Al Adams"))
            .unwrap();
        assert_eq!(al.termination_date, Some(d(2026, 8, 6)));
        assert!(
            al.replaced_in_filing,
            "Al adopted a new plan in the same report"
        );
        // The title was tagged only on the new plan and is carried over.
        assert_eq!(al.title.as_deref(), Some("CEO"));
    }

    #[test]
    fn a_termination_date_after_the_period_is_rejected_not_reported() {
        // Real case: Cryoport tagged the plan's scheduled end date.
        let html = r#"<xbrli:context id="c"><xbrli:entity><xbrli:segment><xbrldi:explicitMember dimension="ecd:IndividualAxis">ex:SMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity><xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
            <ix:nonNumeric contextRef="c" name="ecd:TrdArrIndName">Sam Shaw</ix:nonNumeric>
            <ix:nonNumeric contextRef="c" name="ecd:Rule10b51ArrTrmntdFlag" format="ixt:fixed-true">would have terminated</ix:nonNumeric>
            <ix:nonNumeric contextRef="c" name="ecd:TrdArrTerminationDate">March 12, 2027</ix:nonNumeric>"#;
        let e = &detect_plan_terminations(&oe_item408::extract(html), &meta())[0];
        assert_eq!(e.termination_date, None);
        assert_eq!(e.rejected_termination_date, Some(d(2027, 3, 12)));

        // A date on the last day of the period is fine; so is one before it.
        for ok in ["September 30, 2026", "August 11, 2026"] {
            let html = html.replace("March 12, 2027", ok);
            let e = &detect_plan_terminations(&oe_item408::extract(&html), &meta())[0];
            assert!(
                e.termination_date.is_some() && e.rejected_termination_date.is_none(),
                "{ok}"
            );
        }
    }

    #[test]
    fn without_a_period_the_acceptance_date_is_the_limit() {
        let (ok, bad) = check_termination_date(
            Date::from_ymd(2026, 11, 2),
            None,
            Date::from_ymd(2026, 11, 2),
        );
        assert_eq!((ok, bad), (Date::from_ymd(2026, 11, 2), None));
        let (ok, bad) = check_termination_date(
            Date::from_ymd(2026, 11, 3),
            None,
            Date::from_ymd(2026, 11, 2),
        );
        assert_eq!((ok, bad), (None, Date::from_ymd(2026, 11, 3)));
        assert_eq!(
            check_termination_date(Date::from_ymd(2030, 1, 1), None, None),
            (Date::from_ymd(2030, 1, 1), None)
        );
        assert_eq!(check_termination_date(None, None, None), (None, None));
    }

    #[test]
    fn a_report_that_says_nothing_was_terminated_fires_nothing() {
        let html = r#"<xbrli:context id="c"><xbrli:entity/><xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
            <ix:nonNumeric contextRef="c" name="ecd:Rule10b51ArrTrmntdFlag" format="ixt:fixed-false">terminated</ix:nonNumeric>"#;
        let doc = oe_item408::extract(html);
        assert!(detect_plan_terminations(&doc, &meta()).is_empty());
        assert!(detect_plan_terminations(&oe_item408::extract(""), &meta()).is_empty());
    }

    #[test]
    fn adopted_and_terminated_on_one_person_level_arrangement_counts_as_replaced() {
        let html = r#"<xbrli:context id="c"><xbrli:entity><xbrli:segment><xbrldi:explicitMember dimension="ecd:IndividualAxis">ex:PMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity><xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
            <ix:nonNumeric contextRef="c" name="ecd:TrdArrIndName">Pat Poe</ix:nonNumeric>
            <ix:nonNumeric contextRef="c" name="ecd:Rule10b51ArrTrmntdFlag" format="ixt:fixed-true">terminated</ix:nonNumeric>
            <ix:nonNumeric contextRef="c" name="ecd:Rule10b51ArrAdoptedFlag" format="ixt:fixed-true">adopted</ix:nonNumeric>"#;
        let events = detect_plan_terminations(&oe_item408::extract(html), &meta());
        assert_eq!(events.len(), 1);
        assert!(events[0].replaced_in_filing);
    }
}
