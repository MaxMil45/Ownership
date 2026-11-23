//! Reads Item 408 disclosures of insider trading arrangements from the Inline
//! XBRL of a 10-Q or 10-K.
//!
//! Item 408(a) of Regulation S-K makes a company say, every quarter, which
//! directors and officers adopted or terminated a Rule 10b5-1 trading plan.
//! Companies tag it with the `ecd` taxonomy: each plan is a set of facts that
//! share an XBRL context naming the plan (`ecd:TradingArrAxis`) and the person
//! (`ecd:IndividualAxis`).
//!
//! A flag fact only means something if its `format` says `fixed-true`. Companies
//! that report "nothing adopted or terminated" tag the same flag with
//! `fixed-false` and no plan, and the visible text still reads "terminated".

pub mod ixbrl;

use ixbrl::{contexts, facts, Context, Fact};
use oe_core::text::parse_decimal;
use oe_core::{parse_us_date, Cik, Date};
use std::collections::BTreeMap;

/// One trading plan and what the filing says about it.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct TradingArrangement {
    /// The plan's XBRL member name, e.g. `pltr:ShyamSankarAugust292025PlanMember`.
    /// `None` when the company tags only the person (`ecd:IndividualAxis`) and
    /// not each plan, which most companies do; then all of one person's plan
    /// facts in the filing share one arrangement.
    pub arrangement: Option<String>,
    pub individual_member: Option<String>,
    /// `ecd:TrdArrIndName`. May be only "Mr. Sankar" on one of a person's plans.
    pub person: Option<String>,
    pub title: Option<String>,
    /// `ecd:Rule10b51ArrAdoptedFlag` is true.
    pub rule_10b5_1_adopted: bool,
    /// `ecd:Rule10b51ArrTrmntdFlag` is true.
    pub rule_10b5_1_terminated: bool,
    pub non_rule_adopted: bool,
    pub non_rule_terminated: bool,
    pub adoption_date: Option<Date>,
    pub termination_date: Option<Date>,
    pub expiration_date: Option<Date>,
    /// `ecd:TrdArrSecuritiesAggAvailAmt`, in shares.
    pub securities_amount: Option<f64>,
    /// `ecd:TrdArrDuration`, in days.
    pub duration_days: Option<u32>,
    /// `ecd:MtrlTermsOfTrdArrTextBlock`, as written.
    pub terms_text: Option<String>,
    /// End of the reporting period of the plan's facts.
    pub period_end: Option<Date>,
}

/// The company-level yes/no statements (no plan in their context). `Some(false)`
/// is the usual "nothing adopted / terminated this quarter".
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct CompanyFlags {
    pub rule_10b5_1_adopted: Option<bool>,
    pub rule_10b5_1_terminated: Option<bool>,
    pub non_rule_adopted: Option<bool>,
    pub non_rule_terminated: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct Item408Doc {
    /// `dei:DocumentType`, e.g. `10-Q`.
    pub document_type: Option<String>,
    pub registrant: Option<String>,
    pub cik: Option<Cik>,
    pub period_end: Option<Date>,
    /// Whether the document carries any `ecd:` fact at all.
    pub uses_ecd_tags: bool,
    pub arrangements: Vec<TradingArrangement>,
    pub company_flags: CompanyFlags,
}

/// The facts [`extract`] reads.
fn wanted(n: &str) -> bool {
    n.starts_with("ecd:")
        || matches!(
            n,
            "dei:DocumentType"
                | "dei:EntityRegistrantName"
                | "dei:EntityCentralIndexKey"
                | "dei:DocumentPeriodEndDate"
        )
}

/// The part of a report that [`extract`] reads: every `dei` and `ecd` fact it
/// uses, as written, and the contexts those facts point to. A 2 MB report
/// becomes a few kilobytes, and `extract(&excerpt(html)) == extract(html)`.
pub fn excerpt(html: &str) -> String {
    let spans = ixbrl::fact_spans(html, wanted);
    let contexts = ixbrl::context_spans(html);
    // Every wanted fact's context, nested ones included (ACADIA tags its whole
    // plan inside the terms text block).
    let mut ids: Vec<String> = facts(html, wanted)
        .into_iter()
        .map(|f| f.context_ref)
        .collect();
    ids.sort_unstable();
    ids.dedup();
    let mut out = String::from(
        "<!-- Excerpt of an SEC 10-Q/10-K: its dei and ecd Inline XBRL facts and their contexts, as filed. The full report is at the url in the .meta.txt beside this file. -->\n<html><body>\n",
    );
    for id in &ids {
        if let Some(&(a, b)) = contexts.get(id) {
            out.push_str(&html[a..b]);
            out.push('\n');
        }
    }
    for (a, b, _) in &spans {
        out.push_str(&html[*a..*b]);
        out.push('\n');
    }
    out.push_str("</body></html>\n");
    out
}

/// Reads one iXBRL report. Never fails: a document with no `ecd` facts gives
/// an empty result with `uses_ecd_tags == false`.
pub fn extract(html: &str) -> Item408Doc {
    let all = facts(html, wanted);
    let ctx = contexts(html);

    let mut doc = Item408Doc::default();
    let mut by_plan: BTreeMap<(Option<String>, Option<String>), TradingArrangement> =
        BTreeMap::new();

    for f in &all {
        if let Some(name) = f.name.strip_prefix("dei:") {
            let value = (!f.text.is_empty() && !f.nil).then(|| f.text.clone());
            match name {
                "DocumentType" => doc.document_type = doc.document_type.take().or(value),
                "EntityRegistrantName" => doc.registrant = doc.registrant.take().or(value),
                "EntityCentralIndexKey" => {
                    doc.cik = doc.cik.or_else(|| value.and_then(|v| v.parse().ok()))
                }
                "DocumentPeriodEndDate" => {
                    doc.period_end = doc
                        .period_end
                        .or_else(|| value.and_then(|v| parse_date(&v)))
                }
                _ => {}
            }
            continue;
        }
        doc.uses_ecd_tags = true;
        let context = ctx.get(&f.context_ref);
        let plan = context.and_then(|c| c.arrangement.clone());
        let individual = context.and_then(|c| c.individual.clone());
        // A fact belongs to a plan if its context names a plan or a person. A
        // fact with neither is a company-level statement.
        if plan.is_none() && individual.is_none() {
            apply_company_flag(&mut doc.company_flags, f);
            continue;
        }
        let entry = by_plan
            .entry((plan.clone(), individual.clone()))
            .or_insert_with(|| TradingArrangement {
                arrangement: plan,
                individual_member: individual,
                ..TradingArrangement::default()
            });
        apply_to_arrangement(entry, f, context);
    }
    let mut arrangements: Vec<TradingArrangement> = by_plan.into_values().collect();
    fill_from_same_person(&mut arrangements);
    doc.arrangements = arrangements;
    doc
}

fn flag_value(f: &Fact) -> Option<bool> {
    if f.nil {
        None
    } else if f.format.ends_with("fixed-true") || f.format.ends_with("booleantrue") {
        Some(true)
    } else if f.format.ends_with("fixed-false") || f.format.ends_with("booleanfalse") {
        Some(false)
    } else {
        None
    }
}

fn apply_company_flag(flags: &mut CompanyFlags, f: &Fact) {
    let value = flag_value(f);
    match f.name.as_str() {
        "ecd:Rule10b51ArrAdoptedFlag" => flags.rule_10b5_1_adopted = value,
        "ecd:Rule10b51ArrTrmntdFlag" => flags.rule_10b5_1_terminated = value,
        "ecd:NonRule10b51ArrAdoptedFlag" => flags.non_rule_adopted = value,
        "ecd:NonRule10b51ArrTrmntdFlag" => flags.non_rule_terminated = value,
        _ => {}
    }
}

fn apply_to_arrangement(a: &mut TradingArrangement, f: &Fact, context: Option<&Context>) {
    let text = (!f.nil && !f.text.is_empty()).then(|| f.text.clone());
    if a.period_end.is_none() {
        a.period_end = context.and_then(|c| c.end);
    }
    match f.name.as_str() {
        "ecd:Rule10b51ArrAdoptedFlag" => a.rule_10b5_1_adopted |= flag_value(f) == Some(true),
        "ecd:Rule10b51ArrTrmntdFlag" => a.rule_10b5_1_terminated |= flag_value(f) == Some(true),
        "ecd:NonRule10b51ArrAdoptedFlag" => a.non_rule_adopted |= flag_value(f) == Some(true),
        "ecd:NonRule10b51ArrTrmntdFlag" => a.non_rule_terminated |= flag_value(f) == Some(true),
        "ecd:TrdArrIndName" => a.person = a.person.take().or(text),
        "ecd:TrdArrIndTitle" => a.title = a.title.take().or(text),
        "ecd:TrdArrAdoptionDate" => {
            a.adoption_date = a
                .adoption_date
                .or_else(|| text.and_then(|t| parse_date(&t)))
        }
        "ecd:TrdArrTerminationDate" => {
            a.termination_date = a
                .termination_date
                .or_else(|| text.and_then(|t| parse_date(&t)))
        }
        "ecd:TrdArrExpirationDate" => {
            a.expiration_date = a
                .expiration_date
                .or_else(|| text.and_then(|t| parse_date(&t)))
        }
        "ecd:TrdArrSecuritiesAggAvailAmt" => {
            a.securities_amount = a
                .securities_amount
                .or_else(|| text.and_then(|t| parse_number(&t, f)))
        }
        "ecd:TrdArrDuration" => {
            a.duration_days = a
                .duration_days
                .or_else(|| text.and_then(|t| parse_number(&t, f)).map(|d| d as u32))
        }
        "ecd:MtrlTermsOfTrdArrTextBlock" => a.terms_text = a.terms_text.take().or(text),
        _ => {}
    }
}

/// A person's name or title is often tagged on only one of their plans (the
/// new one, say). Copy it to their other plans in the same filing, and prefer
/// a full name over a bare surname such as "Mr. Sankar".
fn fill_from_same_person(arrangements: &mut [TradingArrangement]) {
    let snapshot = arrangements.to_vec();
    for a in arrangements.iter_mut() {
        let Some(member) = a.individual_member.as_deref() else {
            continue;
        };
        let same = snapshot
            .iter()
            .filter(|o| o.individual_member.as_deref() == Some(member));
        if a.title.is_none() {
            a.title = same.clone().find_map(|o| o.title.clone());
        }
        if a.person.is_none() {
            a.person = same.clone().find_map(|o| o.person.clone());
        }
    }
}

/// Whether SEC's rendered "Insider Trading Arrangements" page (`R21.htm` and
/// the like, 10 to 60 KB) shows a Rule 10b5-1 arrangement as terminated.
///
/// This is a cheap pre-check, so a crawl can skip the 2 MB report when the
/// answer is no. The rendered page prints each flag as a row such as
/// `Rule 10b5-1 Arrangement Terminated | true`; the company-level "nothing
/// terminated" statement prints `false`. Only `true` counts. It looks at the
/// Rule 10b5-1 label, not `Non-Rule 10b5-1 Arrangement Terminated`.
pub fn report_page_shows_termination(html: &str) -> bool {
    page_shows_true(&ixbrl::text_of(html), "Rule 10b5-1 Arrangement Terminated")
}

/// Whether the rendered page shows a Rule 10b5-1 arrangement as adopted
/// (`Rule 10b5-1 Arrangement Adopted | true`), like
/// [`report_page_shows_termination`].
pub fn report_page_shows_adoption(html: &str) -> bool {
    page_shows_true(&ixbrl::text_of(html), "Rule 10b5-1 Arrangement Adopted")
}

/// What the page check found: (shows a termination, shows an adoption).
pub fn report_page_flags(html: &str) -> (bool, bool) {
    let text = ixbrl::text_of(html);
    (
        page_shows_true(&text, "Rule 10b5-1 Arrangement Terminated"),
        page_shows_true(&text, "Rule 10b5-1 Arrangement Adopted"),
    )
}

fn page_shows_true(text: &str, label: &str) -> bool {
    text.match_indices(label).any(|(at, _)| {
        // Skip "Non-Rule 10b5-1 Arrangement ...".
        let before = &text[..at];
        !before.ends_with("Non-")
            && text[at + label.len()..]
                .trim_start()
                .to_ascii_lowercase()
                .starts_with("true")
    })
}
/// `August 6, 2026`, `Aug. 6, 2026`, `2026-08-06` or `08/06/2026`.
pub fn parse_date(text: &str) -> Option<Date> {
    let t = text.trim().trim_end_matches('.');
    if let Ok(d) = t.parse::<Date>() {
        if t.len() == 10 {
            return Some(d);
        }
    }
    if t.contains('/') {
        return parse_us_date(t);
    }
    let mut words = t
        .split(|c: char| c.is_whitespace() || c == ',')
        .filter(|w| !w.is_empty());
    let month = month_number(words.next()?)?;
    let day: u32 = words
        .next()?
        .trim_end_matches(|c: char| c.is_alphabetic())
        .parse()
        .ok()?;
    let year: i32 = words.next()?.parse().ok()?;
    if words.next().is_some() {
        return None;
    }
    Date::from_ymd(year, month, day)
}

fn month_number(word: &str) -> Option<u32> {
    let w = word.trim_end_matches('.').to_ascii_lowercase();
    const NAMES: [&str; 12] = [
        "january",
        "february",
        "march",
        "april",
        "may",
        "june",
        "july",
        "august",
        "september",
        "october",
        "november",
        "december",
    ];
    NAMES
        .iter()
        .position(|n| *n == w || (w.len() == 3 && n.starts_with(&w)))
        .map(|i| i as u32 + 1)
}

/// The numeric value of an `ix:nonFraction` fact, with its `scale` and `sign`.
fn parse_number(text: &str, f: &Fact) -> Option<f64> {
    let mut v = parse_decimal(&text.replace(' ', ""))?;
    v *= 10f64.powi(f.scale);
    Some(if f.negative { -v } else { v })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    #[test]
    fn the_rendered_report_page_precheck_reads_only_true_rule_rows() {
        // Shapes taken from a real rendered page: label and value in adjacent cells.
        let yes = "<tr><td class=\"pl\">Rule 10b5-1 Arrangement Adopted</td><td class=\"text\">true</td></tr>\
                   <tr><td class=\"pl\">Rule 10b5-1 Arrangement Terminated</td><td class=\"text\">true</td></tr>";
        assert!(report_page_shows_termination(yes));
        let no = "<tr><td>Rule 10b5-1 Arrangement Terminated</td><td>false</td></tr>";
        assert!(!report_page_shows_termination(no));
        // The non-Rule 10b5-1 flag is a different statement.
        let non = "<tr><td>Non-Rule 10b5-1 Arrangement Terminated</td><td>true</td></tr>";
        assert!(!report_page_shows_termination(non));
        let both = format!("{non}{yes}");
        assert!(report_page_shows_termination(&both));
        assert!(!report_page_shows_termination(""));
        assert!(!report_page_shows_termination(
            "<p>Rule 10b5-1 Arrangement Terminated</p>"
        ));
        // Adoptions, the same way.
        assert!(report_page_shows_adoption(yes));
        let adopted_only = "<tr><td>Rule 10b5-1 Arrangement Adopted</td><td>true</td></tr>\
                            <tr><td>Rule 10b5-1 Arrangement Terminated</td><td>false</td></tr>";
        assert_eq!(report_page_flags(adopted_only), (false, true));
        assert_eq!(report_page_flags(yes), (true, true));
        let non = "<tr><td>Non-Rule 10b5-1 Arrangement Adopted</td><td>true</td></tr>";
        assert!(!report_page_shows_adoption(non));
    }
    #[test]
    fn parses_the_date_styles_companies_use() {
        assert_eq!(parse_date("August 6, 2026"), Some(d(2026, 8, 6)));
        assert_eq!(parse_date("Aug. 6, 2026"), Some(d(2026, 8, 6)));
        assert_eq!(parse_date("May 11, 2027"), Some(d(2027, 5, 11)));
        assert_eq!(parse_date("September 1st, 2026"), Some(d(2026, 9, 1)));
        assert_eq!(parse_date("2026-08-06"), Some(d(2026, 8, 6)));
        assert_eq!(parse_date("08/06/2026"), Some(d(2026, 8, 6)));
        for bad in [
            "",
            "On August 6, 2026",
            "sometime in August",
            "February 30, 2026",
            "13/45/2026",
        ] {
            assert_eq!(parse_date(bad), None, "{bad:?}");
        }
    }

    const DOC: &str = r#"
<ix:nonNumeric contextRef="c-0" name="dei:DocumentType">10-Q</ix:nonNumeric>
<ix:nonNumeric contextRef="c-0" name="dei:EntityRegistrantName">Example Corp</ix:nonNumeric>
<ix:nonNumeric contextRef="c-0" name="dei:EntityCentralIndexKey">0009999901</ix:nonNumeric>
<ix:nonNumeric contextRef="c-0" name="dei:DocumentPeriodEndDate" format="ixt:date-monthname-day-year-en">September 30, 2026</ix:nonNumeric>
<xbrli:context id="c-old"><xbrli:entity><xbrli:segment>
  <xbrldi:explicitMember dimension="ecd:TradingArrAxis">ex:OldPlanMember</xbrldi:explicitMember>
  <xbrldi:explicitMember dimension="ecd:IndividualAxis">ex:JaneMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity>
  <xbrli:period><xbrli:startDate>2026-07-01</xbrli:startDate><xbrli:endDate>2026-09-30</xbrli:endDate></xbrli:period></xbrli:context>
<xbrli:context id="c-new"><xbrli:entity><xbrli:segment>
  <xbrldi:explicitMember dimension="ecd:TradingArrAxis">ex:NewPlanMember</xbrldi:explicitMember>
  <xbrldi:explicitMember dimension="ecd:IndividualAxis">ex:JaneMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity>
  <xbrli:period><xbrli:startDate>2026-07-01</xbrli:startDate><xbrli:endDate>2026-09-30</xbrli:endDate></xbrli:period></xbrli:context>
<xbrli:context id="c-co"><xbrli:entity/><xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
<ix:nonNumeric contextRef="c-old" name="ecd:TrdArrIndName">Jane Doe</ix:nonNumeric>
<ix:nonNumeric contextRef="c-old" name="ecd:Rule10b51ArrTrmntdFlag" format="ixt:fixed-true">terminated</ix:nonNumeric>
<ix:nonNumeric contextRef="c-old" name="ecd:TrdArrTerminationDate">August 6, 2026</ix:nonNumeric>
<ix:nonNumeric contextRef="c-new" name="ecd:TrdArrIndName">Ms. Doe</ix:nonNumeric>
<ix:nonNumeric contextRef="c-new" name="ecd:TrdArrIndTitle">Chief Executive Officer</ix:nonNumeric>
<ix:nonNumeric contextRef="c-new" name="ecd:Rule10b51ArrAdoptedFlag" format="ixt:fixed-true">adopted</ix:nonNumeric>
<ix:nonFraction unitRef="shares" contextRef="c-new" name="ecd:TrdArrSecuritiesAggAvailAmt" scale="3" format="ixt:num-dot-decimal">1,450</ix:nonFraction>
<ix:nonNumeric contextRef="c-new" name="ecd:TrdArrExpirationDate">May 11, 2027</ix:nonNumeric>
<ix:nonNumeric contextRef="c-co" name="ecd:NonRule10b51ArrTrmntdFlag" format="ixt:fixed-false">terminated</ix:nonNumeric>
<ix:nonNumeric contextRef="c-co" name="ecd:Rule10b51ArrAdoptedFlag" format="ixt:fixed-false">adopted</ix:nonNumeric>
"#;

    #[test]
    fn groups_facts_by_plan_and_person() {
        let doc = extract(DOC);
        assert_eq!(doc.document_type.as_deref(), Some("10-Q"));
        assert_eq!(doc.registrant.as_deref(), Some("Example Corp"));
        assert_eq!(doc.cik, Some(Cik(9_999_901)));
        assert_eq!(doc.period_end, Some(d(2026, 9, 30)));
        assert!(doc.uses_ecd_tags);
        assert_eq!(doc.arrangements.len(), 2);

        let old = doc
            .arrangements
            .iter()
            .find(|a| a.arrangement.as_deref() == Some("ex:OldPlanMember"))
            .unwrap();
        assert!(old.rule_10b5_1_terminated && !old.rule_10b5_1_adopted);
        assert_eq!(old.termination_date, Some(d(2026, 8, 6)));
        assert_eq!(old.person.as_deref(), Some("Jane Doe"));
        assert_eq!(old.period_end, Some(d(2026, 9, 30)));
        // The title is tagged only on her other plan; it is carried over.
        assert_eq!(old.title.as_deref(), Some("Chief Executive Officer"));

        let new = doc
            .arrangements
            .iter()
            .find(|a| a.arrangement.as_deref() == Some("ex:NewPlanMember"))
            .unwrap();
        assert!(new.rule_10b5_1_adopted && !new.rule_10b5_1_terminated);
        assert_eq!(new.securities_amount, Some(1_450_000.0));
        assert_eq!(new.expiration_date, Some(d(2027, 5, 11)));
    }

    #[test]
    fn false_company_level_flags_are_statements_not_plans() {
        let doc = extract(DOC);
        assert_eq!(doc.company_flags.non_rule_terminated, Some(false));
        assert_eq!(doc.company_flags.rule_10b5_1_adopted, Some(false));
        // Their text says "terminated" / "adopted", yet they create no plan.
        assert_eq!(doc.arrangements.len(), 2);
    }

    #[test]
    fn a_report_without_ecd_tags_is_empty_not_an_error() {
        let doc = extract(
            "<ix:nonNumeric contextRef=\"c\" name=\"dei:DocumentType\">10-K</ix:nonNumeric>",
        );
        assert_eq!(doc.document_type.as_deref(), Some("10-K"));
        assert!(!doc.uses_ecd_tags && doc.arrangements.is_empty());
        assert_eq!(extract(""), Item408Doc::default());
    }

    #[test]
    fn companies_that_tag_only_the_person_still_give_one_arrangement_per_person() {
        // Real filings (Coinbase, Robinhood, Carvana, Snap) use IndividualAxis alone.
        let html = r#"<xbrli:context id="c-a"><xbrli:entity><xbrli:segment>
            <xbrldi:explicitMember dimension="ecd:IndividualAxis">ex:AMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity>
            <xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
            <xbrli:context id="c-b"><xbrli:entity><xbrli:segment>
            <xbrldi:explicitMember dimension="ecd:IndividualAxis">ex:BMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity>
            <xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
            <ix:nonNumeric contextRef="c-a" name="ecd:TrdArrIndName">Alex Able</ix:nonNumeric>
            <ix:nonNumeric contextRef="c-a" name="ecd:TrdArrIndTitle">CFO</ix:nonNumeric>
            <ix:nonNumeric contextRef="c-a" name="ecd:Rule10b51ArrTrmntdFlag" format="ixt:fixed-true">terminated</ix:nonNumeric>
            <ix:nonNumeric contextRef="c-b" name="ecd:TrdArrIndName">Bo Baker</ix:nonNumeric>
            <ix:nonNumeric contextRef="c-b" name="ecd:Rule10b51ArrAdoptedFlag" format="ixt:fixed-true">entered</ix:nonNumeric>
            <ix:nonNumeric contextRef="c-b" name="ecd:TrdArrAdoptionDate">8/11/2026</ix:nonNumeric>"#;
        let doc = extract(html);
        assert_eq!(doc.arrangements.len(), 2);
        assert!(doc.arrangements.iter().all(|a| a.arrangement.is_none()));
        let find = |name: &str| {
            doc.arrangements
                .iter()
                .find(|a| a.person.as_deref() == Some(name))
                .unwrap()
        };
        let alex = find("Alex Able");
        assert!(alex.rule_10b5_1_terminated && !alex.rule_10b5_1_adopted);
        assert_eq!(alex.title.as_deref(), Some("CFO"));
        let bo = find("Bo Baker");
        assert!(bo.rule_10b5_1_adopted);
        assert_eq!(bo.adoption_date, Date::from_ymd(2026, 8, 11));
    }
    #[test]
    fn booleantrue_is_another_spelling_of_true_and_company_level_flags_stay_company_level() {
        // Real filings: one company tags a person-level flag ixt:booleantrue; Twin Vee tags
        // a company-level "terminated" flag ixt:booleantrue while its text says nothing ended.
        let html = r#"<xbrli:context id="c-p"><xbrli:entity><xbrli:segment><xbrldi:explicitMember dimension="ecd:IndividualAxis">ex:PMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity><xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
            <xbrli:context id="c-co"><xbrli:entity/><xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
            <ix:nonNumeric contextRef="c-p" name="ecd:Rule10b51ArrTrmntdFlag" format="ixt:booleantrue">terminated</ix:nonNumeric>
            <ix:nonNumeric contextRef="c-co" name="ecd:Rule10b51ArrTrmntdFlag" format="ixt:booleantrue">terminated</ix:nonNumeric>
            <ix:nonNumeric contextRef="c-co" name="ecd:Rule10b51ArrAdoptedFlag" format="ixt:booleanfalse">adopted</ix:nonNumeric>"#;
        let doc = extract(html);
        assert_eq!(doc.arrangements.len(), 1);
        assert!(doc.arrangements[0].rule_10b5_1_terminated);
        // The company-level one is a statement, not a plan: no arrangement is made for it.
        assert_eq!(doc.company_flags.rule_10b5_1_terminated, Some(true));
        assert_eq!(doc.company_flags.rule_10b5_1_adopted, Some(false));
    }

    #[test]
    fn a_flag_with_no_fixed_format_counts_for_nothing() {
        let html = r#"<xbrli:context id="c"><xbrli:entity><xbrli:segment>
            <xbrldi:explicitMember dimension="ecd:TradingArrAxis">ex:PlanMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity>
            <xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
            <ix:nonNumeric contextRef="c" name="ecd:Rule10b51ArrTrmntdFlag">terminated</ix:nonNumeric>"#;
        let doc = extract(html);
        assert_eq!(doc.arrangements.len(), 1);
        assert!(!doc.arrangements[0].rule_10b5_1_terminated);
    }
}
