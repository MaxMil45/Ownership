//! `oe events-build`: runs every rule (their ledger versions, as `oe alerts
//! --db` does) over the downloaded folders and stores the events for the
//! local API (`GET /v1/events`, migration `0008_event_feed`).

use super::alerts::{build_alerts, source_flag, Sources};
use oe_alerts::Alert;

fn connect() -> Result<oe_store::Client, String> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL is not set (see `oe db-status`)".to_string())?;
    oe_store::connect(&url).map_err(|e| e.to_string())
}

pub fn row(a: &Alert) -> oe_store::EventRow {
    oe_store::EventRow {
        key: a.key.clone(),
        rule_id: a.rule_id.to_string(),
        known_on: a.known_on.map(|d| d.to_string()),
        issuer_cik: a.issuer_cik.map(|c| c.0),
        issuer_name: a.issuer_name.clone(),
        ticker: a.ticker.clone(),
        headline: a.headline.clone(),
        amount_usd: a.amount_usd,
        percent: a.percent,
        body: oe_alerts::render::json(a),
    }
}

/// The cluster-buy rule over every open-market purchase in the ledger
/// (2015 on), not just the Form 4 files on disk. The routine-trader filter
/// sees the ledger's trades of the same companies from 2011 on, so events
/// before 2018 had less than the full three years to look back on.
fn ledger_cluster_buys(client: &mut oe_store::Client) -> Result<(Vec<Alert>, String), String> {
    let (buys, left_out) = oe_store::ledger_purchases(client).map_err(|e| e.to_string())?;
    let trades: Vec<oe_events::InsiderTrade> = buys
        .iter()
        .filter_map(|b| {
            let shares = b.shares;
            // A price or value that cannot be real is dropped, not summed.
            let price = b
                .price
                .filter(|p| oe_core::plausible_amounts(Some(*p), shares.map(|s| s * p)));
            Some(oe_events::InsiderTrade {
                issuer: oe_core::Cik(b.issuer_cik),
                ticker: b.ticker.clone(),
                owner: oe_core::Cik(b.owner_cik),
                owner_name: b.owner_name.clone(),
                co_owners: b.co_owners.iter().map(|c| oe_core::Cik(*c)).collect(),
                is_director: b.is_director,
                is_officer: b.is_officer,
                officer_title: b.officer_title.clone(),
                date: b.date.parse().ok()?,
                code: oe_core::TransactionCode::P,
                is_open_market_purchase: true,
                shares,
                price_per_share: price,
                plan_10b5_1: b.plan_10b5_1,
                from_amendment: b.from_amendment,
                known_on: b.known_on.parse().ok(),
                accession: Some(b.accession.clone()),
            })
        })
        .collect();
    let mut issuers: Vec<u64> = buys.iter().map(|b| b.issuer_cik).collect();
    issuers.sort_unstable();
    issuers.dedup();
    let earlier =
        oe_store::trade_history(client, &issuers, "2011-01-01").map_err(|e| e.to_string())?;
    let history_rows = earlier.len();
    let mut history = super::alerts::history_trades(earlier);
    history.extend(trades.iter().cloned());
    let events =
        oe_events::detect_cluster_buys(&trades, &history, &oe_events::ClusterBuyParams::default());
    let note = format!(
        "ledger purchases: {} open-market purchase(s) from {} compan(ies) ({} left out for impossible transaction dates), routine filter saw {history_rows} trade(s), {} cluster buy(s)",
        trades.len(),
        issuers.len(),
        left_out.implausible_dates,
        events.len()
    );
    let alerts = events
        .iter()
        .map(|e| {
            let mut a = oe_alerts::convert::cluster_buy(e);
            if e.total_value > MAX_PLAUSIBLE_CLUSTER_TOTAL {
                hide_dollar_total(&mut a);
            }
            a
        })
        .collect();
    Ok((alerts, note))
}

/// No open-market cluster buy has come near this; the 15 over it in
/// 2015-2026 were prices typed as totals ("1,400,000 shares at $1400").
const MAX_PLAUSIBLE_CLUSTER_TOTAL: f64 = 1_000_000_000.0;

/// The event stays (insiders did buy together), the dollar figure goes.
fn hide_dollar_total(a: &mut Alert) {
    a.amount_usd = None;
    if let Some((head, _)) = a.headline.rsplit_once(", ") {
        a.headline = format!("{head} (dollar total not shown: prices look wrong as filed)");
    }
    if let Some(first) = a.details.first_mut() {
        if let Some((head, _)) = first.split_once(": ") {
            *first = format!("{head}: dollar total not shown, prices look wrong as filed");
        }
    }
}

pub fn events_build(argv: &[String]) -> Result<(), String> {
    let mut only: Option<Sources> = None;
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        if !source_flag(a, &mut it, &mut only)? {
            return Err(format!("unexpected argument {a:?}"));
        }
    }
    // Without folders named, the cluster buys come from the ledger (the
    // whole history); with `--form4 <dir>`, from those files as before.
    let from_ledger = only.is_none();
    let mut sources = only.unwrap_or_else(Sources::defaults);
    if from_ledger {
        sources.form4 = None;
    }
    let started = std::time::Instant::now();
    let (mut alerts, summary) = build_alerts(&sources, true)?;
    for line in &summary {
        println!("{line}");
    }
    let mut client = connect()?;
    if from_ledger {
        let (cluster, note) = ledger_cluster_buys(&mut client)?;
        println!("{note}");
        alerts.extend(cluster);
    }
    // Most filings other than Forms 3/4/5 name no ticker: take the issuer's
    // latest one from its Form 3/4/5 filings, so events can be found by it.
    let tickers = oe_store::latest_tickers(&mut client).map_err(|e| e.to_string())?;
    let mut filled = 0usize;
    for a in &mut alerts {
        if a.ticker.is_none() {
            if let Some(t) = a.issuer_cik.and_then(|c| tickers.get(&c.0)) {
                a.ticker = Some(t.clone());
                filled += 1;
            }
        }
    }
    // One per alert key, as `oe alerts` delivers them.
    let mut seen = std::collections::HashSet::new();
    let rows: Vec<oe_store::EventRow> = alerts
        .iter()
        .filter(|a| seen.insert(a.key.clone()))
        .map(row)
        .collect();
    println!(
        "tickers: {filled} event(s) took their company's latest ticker from its Form 3/4/5 filings"
    );
    let stored = oe_store::replace_event_feed(&mut client, &rows).map_err(|e| e.to_string())?;
    let mut per_rule: std::collections::BTreeMap<&str, usize> = Default::default();
    for r in &rows {
        *per_rule.entry(r.rule_id.as_str()).or_default() += 1;
    }
    let list: Vec<String> = per_rule.iter().map(|(r, n)| format!("{r} {n}")).collect();
    println!(
        "event feed rebuilt in {:.0} s: {stored} event(s) stored ({}); `oe serve` lists them at GET /v1/events",
        started.elapsed().as_secs_f64(),
        list.join(", ")
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_impossible_dollar_total_is_hidden_but_the_event_stays() {
        let mut a = Alert {
            key: "k".into(),
            rule_id: "insider.cluster_buy@1",
            amount_usd: Some(7.62e9),
            percent: None,
            known_on: None,
            issuer_cik: None,
            issuer_name: None,
            ticker: Some("IEECD".into()),
            headline: "Cluster buy at IEECD: 6 insiders, $7.62B".into(),
            details: vec![
                "6 insiders bought between 2016-06-04 and 2016-06-06: $7.62B in total".into(),
                "Insiders: A, B".into(),
            ],
            people: Vec::new(),
            receipts: Vec::new(),
        };
        hide_dollar_total(&mut a);
        assert_eq!(a.amount_usd, None);
        assert_eq!(
            a.headline,
            "Cluster buy at IEECD: 6 insiders (dollar total not shown: prices look wrong as filed)"
        );
        assert!(a.details[0].starts_with(
            "6 insiders bought between 2016-06-04 and 2016-06-06: dollar total not shown"
        ));
        assert!(!a.details[0].contains("7.62"));
        assert_eq!(a.details[1], "Insiders: A, B");
    }
}
