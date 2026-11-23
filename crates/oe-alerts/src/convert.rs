//! One [`Alert`] per event. Each converter says what the alert's key is made
//! of: the facts that make two detections the same event.

use crate::{receipt, Alert, Person};
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
            "Planned insider sale at {who}: {}, {}",
            e.seller,
            money(e.aggregate_market_value)
        ),
        details,
        people: vec![Person {
            cik: None,
            name: e.seller.clone(),
        }],
        receipts: e
            .accession
            .iter()
            .map(|a| receipt(e.issuer, a, Some("144")))
            .collect(),
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
    if let Some(t) = &e.terms_text {
        details.push(format!("Terms: {}", clip(t, 300)));
    }
    let acc = e.accession.clone().unwrap_or_else(|| "?".into());
    Alert {
        key: format!("{}|{}|{}", e.rule_id, acc, person),
        rule_id: e.rule_id,
        amount_usd: None,
        percent: None,
        known_on: e.known_on,
        issuer_cik: e.issuer,
        issuer_name: e.issuer_name.clone(),
        ticker: None,
        headline: format!("10b5-1 plan terminated at {who}: {person}"),
        details,
        people: vec![Person {
            cik: None,
            name: person.clone(),
        }],
        receipts: match (e.issuer, &e.accession) {
            (Some(cik), Some(a)) => vec![receipt(cik, a, e.form.as_deref())],
            _ => Vec::new(),
        },
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
    let mut details = vec![format!(
        "{} contest filing(s) from {} to {}: {}",
        e.filings.len(),
        e.first_known_on.map_or("?".into(), |d| d.to_string()),
        e.last_known_on.map_or("?".into(), |d| d.to_string()),
        forms.join(", ")
    )];
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
    Alert {
        key: format!("{}|{}|{}", e.rule_id, e.issuer, challengers),
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
