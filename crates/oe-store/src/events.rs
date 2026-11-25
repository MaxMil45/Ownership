//! The event feed (migration `0008_event_feed`): every alert the rules find,
//! stored for the local API. Derived, not part of the ledger: each build
//! replaces it whole, in one transaction.

use crate::db::{check_schema, StoreError};
use crate::rows::copy_field;
use postgres::types::ToSql;
use postgres::Client;
use std::io::Write;

/// One event, ready to store. `body` is the alert's JSON object.
#[derive(Debug, Clone, PartialEq)]
pub struct EventRow {
    pub key: String,
    pub rule_id: String,
    /// `YYYY-MM-DD`
    pub known_on: Option<String>,
    pub issuer_cik: Option<u64>,
    pub issuer_name: Option<String>,
    pub ticker: Option<String>,
    pub headline: String,
    pub amount_usd: Option<f64>,
    pub percent: Option<f64>,
    pub body: String,
}

fn opt(v: Option<String>) -> String {
    v.map_or_else(|| "\\N".to_string(), |s| copy_field(&s))
}

fn num(v: Option<f64>) -> String {
    match v {
        Some(x) if x.is_finite() => x.to_string(),
        _ => "\\N".to_string(),
    }
}

/// Replaces the whole feed with `rows` (one per key: later duplicates are
/// dropped). Returns how many were stored.
pub fn replace_event_feed(client: &mut Client, rows: &[EventRow]) -> Result<u64, StoreError> {
    check_schema(client)?;
    let mut seen = std::collections::HashSet::new();
    let mut tx = client.transaction()?;
    tx.batch_execute("TRUNCATE event_feed")?;
    let mut stored = 0u64;
    {
        let mut w = tx.copy_in(
            "COPY event_feed (alert_key, rule_id, known_on, issuer_cik, issuer_name, ticker, headline, amount_usd, percent, body) FROM STDIN",
        )?;
        for r in rows {
            if !seen.insert(r.key.as_str()) {
                continue;
            }
            w.write_all(
                format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    copy_field(&r.key),
                    copy_field(&r.rule_id),
                    opt(r.known_on.clone()),
                    r.issuer_cik
                        .map_or_else(|| "\\N".to_string(), |c| c.to_string()),
                    opt(r.issuer_name.clone()),
                    opt(r.ticker.clone()),
                    copy_field(&r.headline),
                    num(r.amount_usd),
                    num(r.percent),
                    copy_field(&r.body),
                )
                .as_bytes(),
            )?;
            stored += 1;
        }
        w.finish()?;
    }
    tx.commit()?;
    Ok(stored)
}

/// What to list. Every field is optional; all given ones must hold.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct EventFilter {
    /// `YYYY-MM-DD`, inclusive.
    pub since: Option<String>,
    pub until: Option<String>,
    /// A rule id, with or without its version: `insider.cluster_buy` matches
    /// every version, `insider.cluster_buy@2` only that one.
    pub rule: Option<String>,
    pub ticker: Option<String>,
    pub issuer_cik: Option<i64>,
    pub min_amount: Option<f64>,
    /// Words that must all appear in the headline or the company name.
    pub text: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

/// A page of the feed: the matching events' JSON objects, newest first, and
/// how many match in all.
#[derive(Debug, Clone, PartialEq)]
pub struct EventPage {
    pub total: i64,
    pub events: Vec<String>,
    /// When the feed was built (`None` if it is empty).
    pub built_at: Option<String>,
}

pub fn query_events(client: &mut Client, f: &EventFilter) -> Result<EventPage, StoreError> {
    check_schema(client)?;
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
    // `?` in a clause becomes the number of the parameter just pushed.
    fn add(
        clauses: &mut Vec<String>,
        params: &mut Vec<Box<dyn ToSql + Sync>>,
        clause: &str,
        v: Box<dyn ToSql + Sync>,
    ) {
        params.push(v);
        clauses.push(clause.replace('?', &format!("${}", params.len())));
    }
    let (c, p) = (&mut clauses, &mut params);
    if let Some(s) = &f.since {
        add(c, p, "known_on >= ?::text::date", Box::new(s.clone()));
    }
    if let Some(u) = &f.until {
        add(c, p, "known_on <= ?::text::date", Box::new(u.clone()));
    }
    if let Some(r) = &f.rule {
        if r.contains('@') {
            add(c, p, "rule_id = ?", Box::new(r.clone()));
        } else {
            add(c, p, "split_part(rule_id, '@', 1) = ?", Box::new(r.clone()));
        }
    }
    if let Some(t) = &f.ticker {
        add(c, p, "upper(ticker) = upper(?)", Box::new(t.clone()));
    }
    if let Some(cik) = f.issuer_cik {
        add(c, p, "issuer_cik = ?", Box::new(cik));
    }
    if let Some(m) = f.min_amount {
        add(c, p, "amount_usd >= ?", Box::new(m));
    }
    if let Some(text) = &f.text {
        for word in text.split_whitespace().take(8) {
            let pattern = format!(
                "%{}%",
                word.replace('\\', "\\\\")
                    .replace('%', "\\%")
                    .replace('_', "\\_")
            );
            add(
                c,
                p,
                "(headline ILIKE ? OR coalesce(issuer_name, '') ILIKE ?)",
                Box::new(pattern),
            );
        }
    }
    let filter = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|b| b.as_ref()).collect();
    let total: i64 = client
        .query_one(&format!("SELECT count(*) FROM event_feed {filter}"), &refs)?
        .get(0);
    let n = refs.len();
    let mut page_refs = refs.clone();
    page_refs.push(&f.limit);
    page_refs.push(&f.offset);
    let events = client
        .query(
            &format!(
                "SELECT body FROM event_feed {filter}
                 ORDER BY known_on DESC NULLS LAST, amount_usd DESC NULLS LAST, alert_key
                 LIMIT ${} OFFSET ${}",
                n + 1,
                n + 2
            ),
            &page_refs,
        )?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let built_at = client
        .query_one(
            "SELECT to_char(max(built_at) AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS\"Z\"') FROM event_feed",
            &[],
        )?
        .get(0);
    Ok(EventPage {
        total,
        events,
        built_at,
    })
}

/// Events per US Eastern day and rule, for charts: (day, rule id, count).
pub fn event_counts(client: &mut Client) -> Result<Vec<(String, String, i64)>, StoreError> {
    check_schema(client)?;
    Ok(client
        .query(
            "SELECT known_on::text, rule_id, count(*) FROM event_feed
             WHERE known_on IS NOT NULL GROUP BY 1, 2 ORDER BY 1, 2",
            &[],
        )?
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect())
}

/// Each issuer's trading symbol as its latest Form 3/4/5 gives it
/// (`ownership_issuer`), for events whose filings name none (Form 144,
/// 13D/G, proxy and 10-Q/10-K events).
pub fn latest_tickers(
    client: &mut Client,
) -> Result<std::collections::HashMap<u64, String>, StoreError> {
    check_schema(client)?;
    Ok(client
        .query(
            "SELECT DISTINCT ON (i.issuer_cik) i.issuer_cik, i.ticker
             FROM ownership_issuer i JOIN filing f USING (accession)
             WHERE i.ticker IS NOT NULL
             ORDER BY i.issuer_cik, f.accepted_at DESC",
            &[],
        )?
        .iter()
        .map(|r| {
            (
                r.get::<_, i64>(0) as u64,
                r.get::<_, String>(1).to_uppercase(),
            )
        })
        .collect())
}

/// One open-market purchase as the ledger holds it now, with who made it
/// and where it was filed: the input of the cluster-buy rule over the whole
/// history.
#[derive(Debug, Clone, PartialEq)]
pub struct LedgerPurchase {
    pub issuer_cik: u64,
    pub ticker: Option<String>,
    pub owner_cik: u64,
    pub owner_name: String,
    pub is_director: bool,
    pub is_officer: bool,
    pub officer_title: Option<String>,
    /// The other owners of a joint filing.
    pub co_owners: Vec<u64>,
    /// `YYYY-MM-DD`
    pub date: String,
    pub shares: Option<f64>,
    pub price: Option<f64>,
    pub plan_10b5_1: Option<bool>,
    /// US Eastern date the filing became known.
    pub known_on: String,
    pub accession: String,
    pub from_amendment: bool,
}

/// What [`ledger_purchases`] left out.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PurchasesLeftOut {
    /// Transaction date before 2015, after the filing's own date, or more
    /// than a year before it: typos as filed (years 0024, 2033, ...).
    pub implausible_dates: i64,
}

/// Every open-market purchase (code P, not disposed, non-derivative) in the
/// ledger as the public can know it now (amended rows once), except those
/// with an implausible transaction date.
pub fn ledger_purchases(
    client: &mut Client,
) -> Result<(Vec<LedgerPurchase>, PurchasesLeftOut), StoreError> {
    check_schema(client)?;
    let day = "(t.known_at AT TIME ZONE 'America/New_York')::date";
    let is_purchase =
        "NOT t.is_derivative AND t.code = 'P' AND t.acquired_disposed IS DISTINCT FROM 'D'";
    let sane = format!(
        "t.transaction_date >= DATE '2015-01-01' AND t.transaction_date <= {day}
         AND t.transaction_date >= {day} - 365"
    );
    let left_out: i64 = client
        .query_one(
            &format!(
                "SELECT count(*) FROM insider_transaction_as_of(now()) t
                 WHERE {is_purchase} AND NOT ({sane})"
            ),
            &[],
        )?
        .get(0);
    let rows = client.query(
        &format!(
            "SELECT t.issuer_cik, i.ticker, t.owner_cik, coalesce(r.owner_name, ''),
                    coalesce(r.is_director, false), coalesce(r.is_officer, false), r.officer_title,
                    (SELECT array_agg(fo.owner_cik) FROM filing_owner fo
                      WHERE fo.accession = t.accession AND fo.owner_cik <> t.owner_cik),
                    t.transaction_date::text, t.shares::float8, t.price_per_share::float8,
                    t.plan_10b5_1, {day}::text, t.accession, t.supersedes_id IS NOT NULL
             FROM insider_transaction_as_of(now()) t
             LEFT JOIN filing_owner_role r ON r.accession = t.accession AND r.position = 0
             LEFT JOIN ownership_issuer i ON i.accession = t.accession
             WHERE {is_purchase} AND {sane}
             ORDER BY t.issuer_cik, t.transaction_date, t.accession, t.row_in_filing"
        ),
        &[],
    )?;
    let items = rows
        .iter()
        .map(|r| LedgerPurchase {
            issuer_cik: r.get::<_, i64>(0) as u64,
            ticker: r.get(1),
            owner_cik: r.get::<_, i64>(2) as u64,
            owner_name: r.get(3),
            is_director: r.get(4),
            is_officer: r.get(5),
            officer_title: r.get(6),
            co_owners: r
                .get::<_, Option<Vec<i64>>>(7)
                .unwrap_or_default()
                .into_iter()
                .map(|c| c as u64)
                .collect(),
            date: r.get(8),
            shares: r.get(9),
            price: r.get(10),
            plan_10b5_1: r.get(11),
            known_on: r.get(12),
            accession: r.get(13),
            from_amendment: r.get(14),
        })
        .collect();
    Ok((
        items,
        PurchasesLeftOut {
            implausible_dates: left_out,
        },
    ))
}
