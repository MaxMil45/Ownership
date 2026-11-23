//! One [`Alert`] per event. Each converter says what the alert's key is made
//! of: the facts that make two detections the same event.

use crate::{receipt, Alert, Person, Receipt};
use oe_core::Cik;
use oe_events::{
    ActivistEvent, ClusterBuyEvent, PlanTerminatedEvent, PlannedSaleEvent, ProxyContestEvent,
};

/// `$1,234`, `$56.8K`, `$1.25M`, `$3.1B`.
pub fn money(v: f64) -> String {
    let a = v.abs();
    let sign = if v < 0.0 { "-" } else { "" };
    if a >= 1e9 {
        format!("{sign}${:.2}B", a / 1e9)
    } else if a >= 1e6 {
        format!("{sign}${:.2}M", a / 1e6)
    } else if a >= 1e4 {
        format!("{sign}${:.1}K", a / 1e3)
    } else {
        format!("{sign}${a:.0}")
    }
}

/// A share count with thousands separators, no decimals unless fractional.
pub fn shares(v: f64) -> String {
    let whole = v.trunc() as i64;
    let digits = whole.abs().to_string();
    let mut out = String::new();
    for (i, c) in digits.chars().enumerate() {
        if i > 0 && (digits.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    let sign = if v < 0.0 { "-" } else { "" };
    let frac = v.fract().abs();
    if frac > 1e-9 {
        let f = format!("{frac:.4}");
        let f = f.trim_start_matches('0').trim_end_matches('0');
        format!("{sign}{out}{f}")
    } else {
        format!("{sign}{out}")
    }
}

fn clip(text: &str, max: usize) -> String {
    let t: String = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if t.chars().count() <= max {
        t
    } else {
        let mut s: String = t.chars().take(max.saturating_sub(1)).collect();
        s.push('…');
        s
    }
}

fn label(name: Option<&str>, ticker: Option<&str>, cik: Option<Cik>) -> String {
    match (name, ticker, cik) {
        (Some(n), Some(t), _) => format!("{n} ({t})"),
        (Some(n), None, _) => n.to_string(),
        (None, Some(t), _) => t.to_string(),
        (None, None, Some(c)) => format!("CIK {c}"),
        (None, None, None) => "an unnamed issuer".to_string(),
    }
}

/// Key: rule, issuer, first day of the window (the same identity as the
/// `event` table's unique key). Insiders who join the window later do not
/// make a new alert.
pub fn cluster_buy(e: &ClusterBuyEvent) -> Alert {
    let who = label(None, e.ticker.as_deref(), Some(e.issuer));
    let mut details = vec![
        format!(
            "{} insiders bought between {} and {}: {} in total{}",
            e.insiders.len(),
            e.window_start,
            e.window_end,
            money(e.total_value),
            if e.trades_without_value > 0 {
                format!(" (plus {} buy(s) without a price)", e.trades_without_value)
            } else {
                String::new()
            }
        ),
        format!(
            "Insiders: {}",
            e.insiders
                .iter()
                .map(|(_, n)| n.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    ];
    for t in e.trades.iter().take(6) {
        details.push(format!(
            "{} {}: {} shares at {}",
            t.date,
            t.owner_name,
            t.shares.map_or("?".into(), shares),
            t.price_per_share.map_or("?".into(), |p| format!("${p}"))
        ));
    }
    if e.trades.len() > 6 {
        details.push(format!("... and {} more buys", e.trades.len() - 6));
    }
    let mut receipts = Vec::new();
    for t in &e.trades {
        if let Some(acc) = &t.accession {
            if !receipts
                .iter()
                .any(|r: &crate::Receipt| &r.accession == acc)
            {
                receipts.push(receipt(e.issuer, acc, Some("4")));
            }
        }
    }
    Alert {
        key: format!("{}|{}|{}", e.rule_id, e.issuer, e.window_start),
        rule_id: e.rule_id,
        amount_usd: Some(e.total_value),
        percent: None,
        known_on: e.known_on,
        issuer_cik: Some(e.issuer),
        issuer_name: None,
        ticker: e.ticker.clone(),
        headline: format!(
            "Cluster buy at {who}: {} insiders, {}",
            e.insiders.len(),
            money(e.total_value)
        ),
        details,
        people: e
            .insiders
            .iter()
            .map(|(cik, name)| Person {
                cik: Some(*cik),
                name: name.clone(),
            })
            .collect(),
        receipts,
    }
}

/// Key: rule and accession number (one filing, one alert).
pub fn activist(e: &ActivistEvent) -> Alert {
    let who = label(Some(&e.issuer_name), None, Some(e.issuer));
    let holders = e.holders.join(", ");
    let pct = e
        .largest_percent
        .map_or("an unstated stake".to_string(), |p| format!("{p:.1}%"));
    let passive_to_active = e.rule_id == oe_events::activist::PASSIVE_TO_ACTIVE_RULE_ID
        || e.rule_id == oe_events::activist::PASSIVE_TO_ACTIVE_V2_RULE_ID;
    let headline = if passive_to_active {
        format!(
            "Passive holder turns active at {who}: {} ({pct})",
            clip(&holders, 80)
        )
    } else {
        format!("New 13D at {who}: {} ({pct})", clip(&holders, 80))
    };
    let mut details = vec![format!(
        "{} of {}{}",
        pct,
        e.class_title,
        e.largest_shares
            .map_or(String::new(), |s| format!(", {} shares", shares(s)))
    )];
    if e.is_group {
        details.push("The holders say they act as a group.".into());
    }
    if let Some(d) = e.event_date {
        details.push(format!("Date of the event that required the filing: {d}"));
    }
    if let Some(b) = e.basis {
        details.push(format!("Basis: {b}"));
    }
    if let Some((_, day, pct)) = &e.earlier_13g {
        details.push(format!(
            "Earlier passive filing: Schedule 13G accepted {day}{}",
            pct.map_or(String::new(), |p| format!(", {p:.1}%"))
        ));
    }
    if let Some(p) = &e.purpose {
        details.push(format!("Purpose (Item 4): {}", clip(p, 300)));
    }
    Alert {
        key: format!(
            "{}|{}",
            e.rule_id,
            e.accession.clone().unwrap_or_else(|| format!(
                "{}|{}",
                e.issuer,
                e.event_date.map_or("?".into(), |d| d.to_string())
            ))
        ),
        rule_id: e.rule_id,
        amount_usd: None,
        percent: e.largest_percent,
        known_on: e.known_on,
        issuer_cik: Some(e.issuer),
        issuer_name: Some(e.issuer_name.clone()),
        ticker: None,
        headline,
        details,
        people: e
            .holders
            .iter()
            .map(|n| Person {
                cik: None,
                name: n.clone(),
            })
            .collect(),
        receipts: e
            .accession
            .iter()
            .map(|a| receipt(e.issuer, a, None))
            .chain(
                e.earlier_13g
                    .iter()
                    .map(|(a, _, _)| receipt(e.issuer, a, Some("SCHEDULE 13G"))),
            )
            .collect(),
    }
}

/// Key: rule and accession number.
pub fn planned_sale(e: &PlannedSaleEvent) -> Alert {
    let who = label(Some(&e.issuer_name), None, Some(e.issuer));
    let mut details = vec![format!(
        "{} plans to sell {}{} worth {}",
        e.seller,
        e.units
            .map_or("units".to_string(), |u| format!("{} units", shares(u))),
        e.percent_of_outstanding
            .map_or(String::new(), |p| format!(" ({p:.2}% of outstanding)")),
        money(e.aggregate_market_value)
    )];
    if !e.relationships.is_empty() {
        details.push(format!("Relationship: {}", e.relationships.join(", ")));
    }
    if let Some(d) = e.approx_sale_date {
        details.push(format!("Approximate sale date: {d}"));
    }
    details.push(match e.plan_adopted_on {
        Some(d) => format!("Under a Rule 10b5-1 plan adopted {d}"),
        None if e.under_plan => "Under a Rule 10b5-1 plan".to_string(),
        None => "Not under a Rule 10b5-1 plan".to_string(),
    });
    if e.sold_past_3_months > 0.0 {
        details.push(format!(
            "Already sold in the past three months: {} units",
            shares(e.sold_past_3_months)
        ));
    }
    let mut receipts: Vec<Receipt> = e
        .accession
        .iter()
        .map(|a| receipt(e.issuer, a, Some("144")))
        .collect();
    let tag = match &e.history {
        None => "",
        Some(h) if h.notices_past_year == 0 => {
            details.push("The seller's first Form 144 for this company in a year.".into());
            " (first in a year)"
        }
        Some(h) => {
            details.push(format!(
                "Seller's earlier Form 144s for this company in the past year: {} in {} month(s){}",
                h.notices_past_year,
                h.months_past_year,
                h.previous_notice
                    .as_ref()
                    .map_or(String::new(), |(_, d)| format!(", the latest on {d}"))
            ));
            if let Some((a, _)) = &h.previous_notice {
                receipts.push(receipt(e.issuer, a, Some("144, previous notice")));
            }
            if h.routine {
                " (routine seller)"
            } else {
                ""
            }
        }
    };
    Alert {
        key: format!(
            "{}|{}",
            e.rule_id,
            e.accession
                .clone()
                .unwrap_or_else(|| format!("{}|{}", e.issuer, e.seller))
        ),
        rule_id: e.rule_id,
        amount_usd: Some(e.aggregate_market_value),
        percent: e.percent_of_outstanding,
        known_on: e.known_on,
        issuer_cik: Some(e.issuer),
        issuer_name: Some(e.issuer_name.clone()),
        ticker: None,
        headline: format!(
            "Planned insider sale at {who}: {}, {}{tag}",
            e.seller,
            money(e.aggregate_market_value)
        ),
        details,
        people: vec![Person {
            cik: None,
            name: e.seller.clone(),
        }],
        receipts,
    }
}

/// Key: rule, accession number and person. A report can terminate plans of
/// several people (one alert each); several plans of one person in one report
/// (ARS Pharmaceuticals tags two per person in its 2026 Q3 10-Q) make one alert.
pub fn plan_terminated(e: &PlanTerminatedEvent) -> Alert {
    let who = label(e.issuer_name.as_deref(), None, e.issuer);
    let person = e.person.clone().unwrap_or_else(|| "An insider".into());
    let mut details = vec![match (e.termination_date, e.period_end) {
        (Some(d), _) => format!("Terminated on {d}"),
        (None, Some(p)) => format!("Terminated by the end of the period, {p} (no date tagged)"),
        (None, None) => "Termination date not given".to_string(),
    }];
    if let Some(t) = &e.title {
        details.insert(0, format!("{person}, {t}"));
    }
    details.push(if e.replaced_in_filing {
        "A new plan was adopted in the same report.".into()
    } else {
        "No new plan adopted in the same report.".into()
    });
    let mut tag = "";
    let mut earlier_report = None;
    if let Some(l) = &e.plan_life {
        let from = match &l.adoption_report {
            None => "tagged in this report".to_string(),
            Some(_) => "disclosed in an earlier report".to_string(),
        };
        details.push(match l.days {
            Some(days) => format!(
                "The plan was adopted {} ({from}): it lasted {}{days} days{}",
                l.adopted_on,
                if l.days_at_most { "at most " } else { "" },
                if l.within_cooling_off {
                    ", within the 90-day cooling-off, so no sale under it was possible"
                } else {
                    ""
                }
            ),
            None => format!(
                "The plan's tagged adoption date, {}, is after its end ({from}); left as tagged",
                l.adopted_on
            ),
        });
        if l.within_cooling_off {
            tag = " (ended before it could be used)";
        }
        earlier_report = l.adoption_report.clone();
    }
    if let Some(t) = &e.terms_text {
        details.push(format!("Terms: {}", clip(t, 300)));
    }
    let acc = e.accession.clone().unwrap_or_else(|| "?".into());
    let mut receipts = match (e.issuer, &e.accession) {
        (Some(cik), Some(a)) => vec![receipt(cik, a, e.form.as_deref())],
        _ => Vec::new(),
    };
    if let (Some(cik), Some(a)) = (e.issuer, &earlier_report) {
        receipts.push(receipt(cik, a, Some("report disclosing the adoption")));
    }
    Alert {
        key: format!("{}|{}|{}", e.rule_id, acc, person),
        rule_id: e.rule_id,
        amount_usd: None,
        percent: None,
        known_on: e.known_on,
        issuer_cik: e.issuer,
        issuer_name: e.issuer_name.clone(),
        ticker: None,
        headline: format!("10b5-1 plan terminated at {who}: {person}{tag}"),
        details,
        people: vec![Person {
            cik: None,
            name: person.clone(),
        }],
        receipts,
    }
}

/// Key: rule, issuer and the challengers' CIKs (sorted), so the same
/// contest is announced once, and again only when a new challenger appears.
/// `known_on` is the latest contest filing's date.
pub fn proxy_contest(e: &ProxyContestEvent) -> Alert {
    let who = label(Some(&e.issuer_name), None, Some(e.issuer));
    let mut ciks: Vec<u64> = e.challengers.iter().map(|p| p.cik.0).collect();
    ciks.sort_unstable();
    ciks.dedup();
    let names: Vec<&str> = e.challengers.iter().map(|p| p.name.as_str()).collect();
    let headline = if names.is_empty() {
        format!("Contested proxy filing by {who}")
    } else {
        format!("Proxy contest at {who}: {}", clip(&names.join(", "), 100))
    };
    let forms: Vec<&str> = e.filings.iter().map(|f| f.form.as_str()).collect();
    // The dates of the filings this alert carries (for v2, e.first_known_on is
    // the contest's start, which may be in the ledger).
    let first = e.filings.iter().filter_map(|f| f.known_on).min();
    let last = e.filings.iter().filter_map(|f| f.known_on).max();
    let span = match (first, last) {
        (Some(a), Some(b)) if a == b => format!("on {a}"),
        (Some(a), Some(b)) => format!("from {a} to {b}"),
        _ => "on an unknown date".into(),
    };
    let mut details = vec![format!(
        "{} contest filing(s) {span}: {}",
        e.filings.len(),
        forms.join(", ")
    )];
    if e.rule_id == oe_events::proxy_contest::RULE_ID_V2 {
        if e.earlier_filings == 0 {
            details.push(
                "A new contest: no contest filing about this company in the 90 days before.".into(),
            );
        } else {
            details.push(format!(
                "The contest started {} ({} earlier filing(s) in the ledger).",
                e.contest_start.map_or("?".into(), |d| d.to_string()),
                e.earlier_filings
            ));
        }
    }
    if e.challenger_proxy_statement {
        details.push("A challenger filed its own proxy statement.".into());
    }
    if e.company_contested_proxy {
        details.push("The company filed a contested proxy statement.".into());
    }
    let challengers = if ciks.is_empty() {
        "company".to_string()
    } else {
        ciks.iter()
            .map(|c| c.to_string())
            .collect::<Vec<_>>()
            .join(",")
    };
    // v1: one alert per company and set of challengers. v2: one per contest,
    // whoever joins it later.
    let key = match (
        e.rule_id == oe_events::proxy_contest::RULE_ID_V2,
        e.contest_start,
    ) {
        (true, Some(start)) => format!("{}|{}|{}", e.rule_id, e.issuer, start),
        _ => format!("{}|{}|{}", e.rule_id, e.issuer, challengers),
    };
    Alert {
        key,
        rule_id: e.rule_id,
        amount_usd: None,
        percent: None,
        known_on: e.last_known_on,
        issuer_cik: Some(e.issuer),
        issuer_name: Some(e.issuer_name.clone()),
        ticker: None,
        headline,
        details,
        people: e
            .challengers
            .iter()
            .map(|p| Person {
                cik: Some(p.cik),
                name: p.name.clone(),
            })
            .collect(),
        receipts: e
            .filings
            .iter()
            .map(|f| receipt(e.issuer, &f.accession, Some(&f.form)))
            .collect(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use oe_core::{Date, Party};
    use oe_events::ContestFiling;

    #[test]
    fn a_contest_alerts_once_however_its_cast_changes_and_a_new_one_alerts_again() {
        let co = Party {
            cik: Cik(1),
            name: "EXAMPLE CORP".into(),
        };
        let fund = |cik: u64| Party {
            cik: Cik(cik),
            name: format!("Fund {cik}"),
        };
        let filing = |day: u32, by: Party| ContestFiling {
            form: "DFAN14A".into(),
            accession: format!("0000000000-26-{day:06}"),
            known_on: Date::from_ymd(2026, 4, day),
            subject: co.clone(),
            filed_by: vec![by],
        };
        let ledger = [filing(1, fund(2))];
        let day = |d: u32, by: Party| {
            let e = oe_events::detect_proxy_contests_with_ledger(&[filing(d, by)], &ledger);
            proxy_contest(&e[0])
        };
        // Same contest, a new fund joining: the same key, so no second alert.
        let (a, b) = (day(3, fund(2)), day(9, fund(5)));
        assert_eq!(a.key, "activist.proxy_contest@2|0000000001|2026-04-01");
        assert_eq!(a.key, b.key);
        assert!(b.details[0].starts_with("1 contest filing(s) on 2026-04-09"));
        assert!(b.details[1].contains("started 2026-04-01 (1 earlier filing"));
        // v1 keys on the challengers, so the same two days were two alerts.
        let v1 = |d: u32, by: Party| {
            proxy_contest(&oe_events::detect_proxy_contests(&[filing(d, by)])[0]).key
        };
        assert_ne!(v1(3, fund(2)), v1(9, fund(5)));
        // A new contest a season later is a new key.
        let later = oe_events::detect_proxy_contests_with_ledger(
            &[ContestFiling {
                known_on: Date::from_ymd(2026, 9, 1),
                ..filing(1, fund(2))
            }],
            &[],
        );
        assert_ne!(proxy_contest(&later[0]).key, a.key);
        assert!(proxy_contest(&later[0]).details[1].starts_with("A new contest"));
    }

    #[test]
    fn a_planned_sale_says_first_or_routine_and_links_the_previous_notice() {
        let form = oe_form144::parse(
            &std::fs::read_to_string(
                std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                    .join("../../fixtures/form144/synthetic/01_officer_plan_sale.xml"),
            )
            .unwrap(),
        )
        .unwrap();
        let meta = oe_core::FilingMeta {
            accession: "0009999911-26-000003".parse().ok(),
            known_on: Date::from_ymd(2026, 3, 11),
            source: None,
        };
        let earlier = |months: u32| -> Vec<oe_events::EarlierNotice> {
            (1..=months)
                .map(|m| oe_events::EarlierNotice {
                    issuer: form.issuer_cik,
                    seller: form.seller_name.clone(),
                    accession: format!("0009999911-25-{m:06}"),
                    known_on: Date::from_ymd(2025, m + 3, 2).unwrap(),
                })
                .collect()
        };
        let alert = |ledger: &[oe_events::EarlierNotice]| {
            let e = oe_events::detect_planned_sales_with_ledger(
                &[(form.clone(), meta.clone())],
                &Default::default(),
                ledger,
            );
            planned_sale(&e[0])
        };
        let first = alert(&[]);
        assert!(
            first.headline.ends_with("(first in a year)"),
            "{}",
            first.headline
        );
        assert_eq!(first.receipts.len(), 1);

        let routine = alert(&earlier(6));
        assert!(routine.headline.ends_with("(routine seller)"));
        assert!(routine
            .details
            .iter()
            .any(|d| d.contains("6 in 6 month(s), the latest on 2025-09-02")));
        assert_eq!(routine.receipts.len(), 2);
        assert_eq!(routine.receipts[1].accession, "0009999911-25-000006");
        assert_eq!(routine.key, "insider.planned_sale@2|0009999911-26-000003");

        let some = alert(&earlier(2));
        assert!(some.headline.ends_with(&money(2_824_614.0)), "neither tag");
    }

    #[test]
    fn a_plan_ended_in_its_cooling_off_says_so_and_links_the_adoption_report() {
        let e = PlanTerminatedEvent {
            rule_id: oe_events::plan_terminated::RULE_ID_V2,
            issuer: Some(Cik(1)),
            issuer_name: Some("EXAMPLE CORP".into()),
            person: Some("Sam Shaw".into()),
            title: Some("CFO".into()),
            termination_date: Date::from_ymd(2026, 8, 26),
            rejected_termination_date: None,
            period_end: Date::from_ymd(2026, 9, 30),
            replaced_in_filing: false,
            terms_text: None,
            form: Some("10-Q".into()),
            accession: Some("0000000001-26-000002".into()),
            known_on: Date::from_ymd(2026, 11, 2),
            plan_life: Some(oe_events::PlanLife {
                adopted_on: Date::from_ymd(2026, 6, 11).unwrap(),
                adoption_report: Some("0000000001-26-000001".into()),
                days: Some(76),
                days_at_most: false,
                within_cooling_off: true,
            }),
        };
        let a = plan_terminated(&e);
        assert!(a.headline.ends_with("(ended before it could be used)"));
        assert!(a.details.iter().any(|d| d
            == "The plan was adopted 2026-06-11 (disclosed in an earlier report): it lasted 76 days, within the 90-day cooling-off, so no sale under it was possible"));
        assert_eq!(a.receipts.len(), 2);
        assert_eq!(a.receipts[1].accession, "0000000001-26-000001");

        // A long plan: no tag, one receipt if tagged in the same report.
        let mut long = e.clone();
        long.plan_life = Some(oe_events::PlanLife {
            adoption_report: None,
            days: Some(213),
            within_cooling_off: false,
            ..e.plan_life.clone().unwrap()
        });
        let a = plan_terminated(&long);
        assert!(a.headline.ends_with("Sam Shaw"));
        assert_eq!(a.receipts.len(), 1);
        assert!(a.details.iter().any(|d| d.ends_with("lasted 213 days")));
    }

    #[test]
    fn money_and_shares_read_like_a_person_wrote_them() {
        assert_eq!(money(950.0), "$950");
        assert_eq!(money(56_789.0), "$56.8K");
        assert_eq!(money(1_250_000.0), "$1.25M");
        assert_eq!(money(3_100_000_000.0), "$3.10B");
        assert_eq!(shares(96_000_000.0), "96,000,000");
        assert_eq!(shares(999.0), "999");
        assert_eq!(shares(1234.5), "1,234.5");
        assert_eq!(shares(0.0), "0");
    }

    #[test]
    fn long_text_is_clipped_on_characters_not_bytes() {
        assert_eq!(clip("a  b\n c", 10), "a b c");
        assert_eq!(clip("éééééé", 4), "ééé…");
    }
}
