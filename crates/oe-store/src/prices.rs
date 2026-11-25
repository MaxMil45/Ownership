//! Prices the user imported and what happened after events and trades
//! (migration `0010_prices`). History, not advice: every number here is a
//! past return measured from the first close after a filing became public.

use crate::db::{check_schema, StoreError};
use crate::rows::copy_field;
use oe_core::Date;
use postgres::Client;
use std::io::Write;

/// The market the outcomes are compared with: SPY, an S&P 500 fund.
pub const BENCHMARK: &str = "spy.us";
/// Prices before this are not kept (the ledger starts in 2015; one year more
/// gives the first events a starting price).
pub const SINCE: &str = "2014-01-01";
/// Horizons in trading days: about 1, 3, 6 and 12 months.
pub const HORIZONS: [i32; 4] = [21, 63, 126, 252];

/// Every company with a ticker in its Form 3/4/5 filings, as the Stooq
/// symbol of its latest ticker.
pub fn wanted_symbols(client: &mut Client) -> Result<Vec<(i64, String)>, StoreError> {
    Ok(crate::events::latest_tickers(client)?
        .into_iter()
        .filter_map(|(cik, t)| oe_prices::stooq_symbol(&t).map(|s| (cik as i64, s)))
        .collect())
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ImportReport {
    pub symbols: u64,
    pub days: u64,
}

/// Replaces every stored price with what `fill` hands over, in one
/// transaction: `fill` gets a sink taking (symbol, days oldest first) and
/// calls it once per series. Days before [`SINCE`] are dropped.
pub fn import_prices(
    client: &mut Client,
    source: &str,
    fill: impl FnOnce(&mut dyn FnMut(&str, &[(Date, f64)]) -> Result<(), String>) -> Result<(), String>,
) -> Result<Result<ImportReport, String>, StoreError> {
    check_schema(client)?;
    let since: Date = SINCE.parse().expect("a valid date");
    let mut tx = client.transaction()?;
    tx.batch_execute("TRUNCATE price_daily")?;
    let mut report = ImportReport::default();
    let mut first: Option<Date> = None;
    let mut last: Option<Date> = None;
    let outcome = {
        let mut w = tx.copy_in("COPY price_daily (symbol, day, close, n) FROM STDIN")?;
        let mut sink = |symbol: &str, days: &[(Date, f64)]| -> Result<(), String> {
            let mut n = 0i32;
            let mut buf = String::new();
            for (d, c) in days.iter().filter(|(d, _)| *d >= since) {
                n += 1;
                buf.push_str(&format!("{}\t{d}\t{c}\t{n}\n", copy_field(symbol)));
                first = Some(first.map_or(*d, |f| f.min(*d)));
                last = Some(last.map_or(*d, |l| l.max(*d)));
            }
            if n > 0 {
                w.write_all(buf.as_bytes()).map_err(|e| e.to_string())?;
                report.symbols += 1;
                report.days += n as u64;
            }
            Ok(())
        };
        let r = fill(&mut sink);
        match r {
            Ok(()) => {
                w.finish()?;
                Ok(())
            }
            Err(e) => Err(e),
        }
    };
    if let Err(e) = outcome {
        // Nothing is kept: the transaction is dropped without a commit.
        return Ok(Err(e));
    }
    tx.execute(
        "INSERT INTO price_import (source, symbols, days, first_day, last_day)
         VALUES ($1, $2, $3, $4::text::date, $5::text::date)",
        &[
            &source,
            &(report.symbols as i32),
            &(report.days as i64),
            &first.map(|d| d.to_string()),
            &last.map(|d| d.to_string()),
        ],
    )?;
    tx.commit()?;
    Ok(Ok(report))
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CheckReport {
    pub ok: i64,
    pub mismatch: i64,
    pub unchecked: i64,
    /// Companies with a ticker but no price series in the file.
    pub no_prices: i64,
}

/// A company's series is used only when its closes agree with what its
/// insiders reported paying or getting on the same days. Each of up to 20
/// latest trades may be off by 20% plus 5 points per year between the trade
/// and the series' last day: the provider subtracts every later dividend from
/// old closes, so a high-dividend stock's 2017 closes sit well below what
/// insiders paid then. The median trade must be within its allowance (half
/// of it for one or two trades). A reused ticker, a series of another
/// company, or a reverse split after the trades (closes several times off)
/// fails.
pub const MAX_MEDIAN_GAP: f64 = 0.20;
pub const GAP_PER_YEAR: f64 = 0.05;

pub fn check_series(client: &mut Client) -> Result<CheckReport, StoreError> {
    check_schema(client)?;
    let wanted = wanted_symbols(client)?;
    let mut tx = client.transaction()?;
    tx.batch_execute(
        "TRUNCATE issuer_price_series;
         CREATE TEMP TABLE stage_wanted (issuer_cik bigint, symbol text) ON COMMIT DROP;",
    )?;
    {
        let mut w = tx.copy_in("COPY stage_wanted FROM STDIN")?;
        for (cik, sym) in &wanted {
            w.write_all(format!("{cik}\t{}\n", copy_field(sym)).as_bytes())?;
        }
        w.finish()?;
    }
    tx.execute(
        "INSERT INTO issuer_price_series (issuer_cik, symbol, compared, median_gap, status)
         SELECT w.issuer_cik, w.symbol, count(c.gap),
                percentile_cont(0.5) WITHIN GROUP (ORDER BY c.gap),
                CASE WHEN count(c.gap) >= 3
                          AND percentile_cont(0.5) WITHIN GROUP (ORDER BY c.score) <= 1 THEN 'ok'
                     WHEN count(c.gap) BETWEEN 1 AND 2 AND max(c.score) <= 0.5 THEN 'ok'
                     WHEN count(c.gap) >= 1 THEN 'mismatch'
                     ELSE 'unchecked' END
         FROM stage_wanted w
         LEFT JOIN LATERAL (
             SELECT abs(p.close / t.price_per_share::float8 - 1) AS gap,
                    -- the gap against this trade's allowance (1 = just within)
                    abs(p.close / t.price_per_share::float8 - 1)
                      / ($1::float8 + $2::float8
                         * ((SELECT max(day) FROM price_daily x WHERE x.symbol = w.symbol)
                            - t.transaction_date)::float8 / 365.25) AS score
             FROM insider_transaction t
             JOIN price_daily p ON p.symbol = w.symbol AND p.day = t.transaction_date
             WHERE t.issuer_cik = w.issuer_cik AND NOT t.is_derivative AND t.code IN ('P', 'S')
               AND t.price_per_share > 0 AND t.price_per_share <= 2000000
             ORDER BY t.transaction_date DESC
             LIMIT 20
         ) c ON true
         WHERE EXISTS (SELECT 1 FROM price_daily p WHERE p.symbol = w.symbol)
         GROUP BY w.issuer_cik, w.symbol",
        &[&MAX_MEDIAN_GAP, &GAP_PER_YEAR],
    )?;
    let counts = tx.query_one(
        "SELECT count(*) FILTER (WHERE status = 'ok'), count(*) FILTER (WHERE status = 'mismatch'),
                count(*) FILTER (WHERE status = 'unchecked') FROM issuer_price_series",
        &[],
    )?;
    tx.commit()?;
    let (ok, mismatch, unchecked): (i64, i64, i64) = (counts.get(0), counts.get(1), counts.get(2));
    Ok(CheckReport {
        ok,
        mismatch,
        unchecked,
        no_prices: wanted.len() as i64 - ok - mismatch - unchecked,
    })
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutcomeReport {
    pub events: i64,
    pub buys: i64,
    pub sales: i64,
    /// Of all, those with a 12-month result (the rest are too recent).
    pub with_12m: i64,
}

const BUILD_OUTCOMES: &str = "
WITH src AS (
    SELECT 'event'::text AS kind, e.alert_key AS ref, e.issuer_cik, NULL::bigint AS owner_cik,
           split_part(e.rule_id, '@', 1) AS family, NULL::date AS traded_on, e.known_on,
           e.amount_usd AS value, NULL::text AS bioguide_id
    FROM event_feed e
    WHERE e.issuer_cik IS NOT NULL AND e.known_on IS NOT NULL
    UNION ALL
    SELECT CASE WHEN t.code = 'P' THEN 'buy' ELSE 'sale' END,
           t.issuer_cik || '|' || t.owner_cik || '|' || t.transaction_date || '|' || t.code,
           t.issuer_cik, t.owner_cik,
           CASE WHEN t.code = 'P' THEN 'insider.buy' ELSE 'insider.sale' END,
           t.transaction_date, min((f.accepted_at AT TIME ZONE 'America/New_York')::date),
           sum(t.shares * t.price_per_share) FILTER (WHERE t.price_per_share > 0
               AND t.price_per_share <= 2000000 AND t.shares * t.price_per_share <= 2000000000)::float8,
           NULL::text
    FROM insider_transaction t
    JOIN filing f ON f.accession = t.accession
    WHERE NOT t.is_derivative
      AND ((t.code = 'P' AND t.acquired_disposed IS DISTINCT FROM 'D')
           OR (t.code = 'S' AND t.acquired_disposed IS DISTINCT FROM 'A'))
      AND t.transaction_date <= (f.accepted_at AT TIME ZONE 'America/New_York')::date
      AND t.transaction_date >= (f.accepted_at AT TIME ZONE 'America/New_York')::date - 365
      AND NOT EXISTS (SELECT 1 FROM insider_transaction n WHERE n.supersedes_id = t.id)
      AND EXISTS (SELECT 1 FROM issuer_price_series s WHERE s.issuer_cik = t.issuer_cik AND s.status = 'ok')
    GROUP BY t.issuer_cik, t.owner_cik, t.transaction_date, t.code
    UNION ALL
    -- Members of Congress: public from the report's filing date (the House
    -- posts reports within days; ADR 0010 3.3).
    SELECT CASE WHEN h.tx_type = 'P' THEN 'official_buy' ELSE 'official_sale' END,
           h.doc_id || '|' || h.line_no, h.issuer_cik, NULL::bigint,
           CASE WHEN h.tx_type = 'P' THEN 'official.buy' ELSE 'official.sale' END,
           h.tx_date, r.filed_on, h.amount_min::float8, l.bioguide_id
    FROM house_transaction h
    JOIN house_report r USING (doc_id)
    JOIN house_report_link l USING (doc_id)
    WHERE h.issuer_cik IS NOT NULL AND h.tx_type IN ('P', 'S', 'S (partial)')
      AND r.filed_on IS NOT NULL AND l.bioguide_id IS NOT NULL
)
INSERT INTO outcome (kind, ref, issuer_cik, owner_cik, family, traded_on, known_on, value, entry_day,
                     r21, r63, r126, r252, x21, x63, x126, x252, bioguide_id)
SELECT s.kind, s.ref, s.issuer_cik, s.owner_cik, s.family, s.traded_on, s.known_on, s.value, en.day,
       p21.close / en.close - 1, p63.close / en.close - 1, p126.close / en.close - 1, p252.close / en.close - 1,
       (p21.close / en.close) - (m21.close / me.close), (p63.close / en.close) - (m63.close / me.close),
       (p126.close / en.close) - (m126.close / me.close), (p252.close / en.close) - (m252.close / me.close),
       s.bioguide_id
FROM src s
JOIN issuer_price_series ips ON ips.issuer_cik = s.issuer_cik AND ips.status = 'ok'
JOIN LATERAL (
    SELECT day, n, close FROM price_daily
    WHERE symbol = ips.symbol AND day > s.known_on ORDER BY day LIMIT 1
) en ON en.day <= s.known_on + 10
JOIN LATERAL (
    SELECT n, close FROM price_daily
    WHERE symbol = $1 AND day >= en.day ORDER BY day LIMIT 1
) me ON true
LEFT JOIN price_daily p21  ON p21.symbol  = ips.symbol AND p21.n  = en.n + 21
LEFT JOIN price_daily p63  ON p63.symbol  = ips.symbol AND p63.n  = en.n + 63
LEFT JOIN price_daily p126 ON p126.symbol = ips.symbol AND p126.n = en.n + 126
LEFT JOIN price_daily p252 ON p252.symbol = ips.symbol AND p252.n = en.n + 252
LEFT JOIN price_daily m21  ON m21.symbol  = $1 AND m21.n  = me.n + 21
LEFT JOIN price_daily m63  ON m63.symbol  = $1 AND m63.n  = me.n + 63
LEFT JOIN price_daily m126 ON m126.symbol = $1 AND m126.n = me.n + 126
LEFT JOIN price_daily m252 ON m252.symbol = $1 AND m252.n = me.n + 252";

/// The yardstick: every company with a checked series, from the first trading
/// day of every month since 2015, measured exactly like an event. A typical
/// single stock trails the index (a few big winners lift it), so a kind of
/// filing is better or worse than this row, not than zero.
const BUILD_BASELINE: &str = "
WITH starts AS (
    SELECT DISTINCT ON (ips.issuer_cik, date_trunc('month', p.day))
           ips.issuer_cik, ips.symbol, p.day, p.n, p.close
    FROM issuer_price_series ips
    JOIN price_daily p ON p.symbol = ips.symbol
    WHERE ips.status = 'ok' AND p.day >= DATE '2015-01-01'
    ORDER BY ips.issuer_cik, date_trunc('month', p.day), p.day
)
INSERT INTO outcome (kind, ref, issuer_cik, family, known_on, entry_day,
                     r21, r63, r126, r252, x21, x63, x126, x252)
SELECT 'event', 'baseline|' || s.issuer_cik || '|' || s.day, s.issuer_cik, 'baseline.any_month',
       s.day - 1, s.day,
       p21.close / s.close - 1, p63.close / s.close - 1, p126.close / s.close - 1, p252.close / s.close - 1,
       (p21.close / s.close) - (m21.close / me.close), (p63.close / s.close) - (m63.close / me.close),
       (p126.close / s.close) - (m126.close / me.close), (p252.close / s.close) - (m252.close / me.close)
FROM starts s
JOIN LATERAL (
    SELECT n, close FROM price_daily WHERE symbol = $1 AND day >= s.day ORDER BY day LIMIT 1
) me ON true
LEFT JOIN price_daily p21  ON p21.symbol  = s.symbol AND p21.n  = s.n + 21
LEFT JOIN price_daily p63  ON p63.symbol  = s.symbol AND p63.n  = s.n + 63
LEFT JOIN price_daily p126 ON p126.symbol = s.symbol AND p126.n = s.n + 126
LEFT JOIN price_daily p252 ON p252.symbol = s.symbol AND p252.n = s.n + 252
LEFT JOIN price_daily m21  ON m21.symbol  = $1 AND m21.n  = me.n + 21
LEFT JOIN price_daily m63  ON m63.symbol  = $1 AND m63.n  = me.n + 63
LEFT JOIN price_daily m126 ON m126.symbol = $1 AND m126.n = me.n + 126
LEFT JOIN price_daily m252 ON m252.symbol = $1 AND m252.n = me.n + 252";

/// Empties and refills `outcome`: every event of the feed and every
/// insider buy or sale day at a company whose price series passed
/// [`check_series`], with SPY as the market.
pub fn build_outcomes(client: &mut Client) -> Result<OutcomeReport, StoreError> {
    check_schema(client)?;
    let mut tx = client.transaction()?;
    tx.batch_execute("TRUNCATE outcome")?;
    tx.execute(BUILD_OUTCOMES, &[&BENCHMARK])?;
    tx.execute(BUILD_BASELINE, &[&BENCHMARK])?;
    let r = tx.query_one(
        "SELECT count(*) FILTER (WHERE kind = 'event' AND family <> 'baseline.any_month'),
                count(*) FILTER (WHERE kind = 'buy'),
                count(*) FILTER (WHERE kind = 'sale'), count(r252) FROM outcome",
        &[],
    )?;
    tx.commit()?;
    Ok(OutcomeReport {
        events: r.get(0),
        buys: r.get(1),
        sales: r.get(2),
        with_12m: r.get(3),
    })
}

/// One horizon's numbers for a group of outcomes.
#[derive(Debug, Clone, PartialEq)]
pub struct HorizonStats {
    pub days: i32,
    /// How many had this horizon (the others are too recent).
    pub n: i64,
    pub median_return: Option<f64>,
    /// Median of the return minus SPY's over the same days.
    pub median_excess: Option<f64>,
    pub mean_excess: Option<f64>,
    /// Share that went up / share that beat SPY.
    pub share_up: Option<f64>,
    pub share_beat: Option<f64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct OutcomeSummary {
    pub kind: String,
    pub family: String,
    pub count: i64,
    pub horizons: Vec<HorizonStats>,
}

pub fn outcome_summary(client: &mut Client) -> Result<Vec<OutcomeSummary>, StoreError> {
    check_schema(client)?;
    let cols: Vec<String> = HORIZONS
        .iter()
        .map(|h| {
            format!(
                "count(r{h}), percentile_cont(0.5) WITHIN GROUP (ORDER BY r{h}),
                 percentile_cont(0.5) WITHIN GROUP (ORDER BY x{h}), avg(x{h}),
                 avg((r{h} > 0)::int)::float8, avg((x{h} > 0)::int)::float8"
            )
        })
        .collect();
    let rows = client.query(
        &format!(
            "SELECT kind, family, count(*), {} FROM outcome GROUP BY 1, 2 ORDER BY 1, 2",
            cols.join(", ")
        ),
        &[],
    )?;
    Ok(rows
        .iter()
        .map(|r| OutcomeSummary {
            kind: r.get(0),
            family: r.get(1),
            count: r.get(2),
            horizons: HORIZONS
                .iter()
                .enumerate()
                .map(|(i, &days)| {
                    let at = 3 + i * 6;
                    HorizonStats {
                        days,
                        n: r.get(at),
                        median_return: r.get(at + 1),
                        median_excess: r.get(at + 2),
                        mean_excess: r.get(at + 3),
                        share_up: r.get(at + 4),
                        share_beat: r.get(at + 5),
                    }
                })
                .collect(),
        })
        .collect())
}

/// An insider's record: what the company's shares did in the year after the
/// insider's open-market buys became public.
#[derive(Debug, Clone, PartialEq)]
pub struct InsiderRecord {
    pub owner_cik: i64,
    pub name: String,
    pub buys: i64,
    /// Buys old enough for a 12-month result.
    pub measured: i64,
    pub companies: i64,
    pub median_return_12m: Option<f64>,
    pub median_excess_12m: Option<f64>,
    pub share_beat_12m: Option<f64>,
    pub last_buy: Option<String>,
}

/// Insiders with at least `min_measured` measured buys, the `limit` best
/// (`best`) or worst by median 12-month return against SPY.
pub fn insider_records(
    client: &mut Client,
    min_measured: i64,
    limit: i64,
    best: bool,
) -> Result<Vec<InsiderRecord>, StoreError> {
    check_schema(client)?;
    let order = if best { "DESC" } else { "ASC" };
    Ok(client
        .query(
            &format!(
                "WITH r AS (
                     SELECT owner_cik, count(*) AS buys, count(x252) AS measured,
                            count(DISTINCT issuer_cik) AS companies,
                            percentile_cont(0.5) WITHIN GROUP (ORDER BY r252) AS mr,
                            percentile_cont(0.5) WITHIN GROUP (ORDER BY x252) AS mx,
                            avg((x252 > 0)::int)::float8 AS beat, max(traded_on)::text AS last
                     FROM outcome WHERE kind = 'buy' GROUP BY owner_cik
                 )
                 SELECT r.owner_cik,
                        coalesce((SELECT o.owner_name FROM filing_owner_role o
                                  WHERE o.owner_cik = r.owner_cik ORDER BY o.accession DESC LIMIT 1), ''),
                        r.buys, r.measured, r.companies, r.mr, r.mx, r.beat, r.last
                 FROM r WHERE r.measured >= $1
                 ORDER BY r.mx {order} NULLS LAST, r.measured DESC, r.owner_cik
                 LIMIT $2"
            ),
            &[&min_measured, &limit],
        )?
        .iter()
        .map(|r| InsiderRecord {
            owner_cik: r.get(0),
            name: r.get(1),
            buys: r.get(2),
            measured: r.get(3),
            companies: r.get(4),
            median_return_12m: r.get(5),
            median_excess_12m: r.get(6),
            share_beat_12m: r.get(7),
            last_buy: r.get(8),
        })
        .collect())
}

/// One measured event or trade, for lists and per-person records.
#[derive(Debug, Clone, PartialEq)]
pub struct OutcomeRow {
    pub kind: String,
    pub reference: String,
    pub family: String,
    pub issuer_cik: i64,
    pub issuer_name: String,
    pub ticker: Option<String>,
    pub owner_cik: Option<i64>,
    pub traded_on: Option<String>,
    pub known_on: String,
    pub value: Option<f64>,
    pub entry_day: String,
    /// r21, r63, r126, r252, then x21 .. x252.
    pub returns: [Option<f64>; 8],
}

/// Which outcomes [`outcomes`] lists.
#[derive(Debug, Clone, PartialEq)]
pub enum OutcomesOf {
    Owner(i64),
    Issuer(i64),
    /// The `limit` events of one family with the highest (`best`) or lowest
    /// 12-month return against SPY.
    Extremes {
        family: String,
        best: bool,
        limit: i64,
    },
}

pub fn outcomes(client: &mut Client, of: &OutcomesOf) -> Result<Vec<OutcomeRow>, StoreError> {
    check_schema(client)?;
    let base = "SELECT o.kind, o.ref, o.family, o.issuer_cik,
                       coalesce((SELECT i.issuer_name FROM ownership_issuer i WHERE i.issuer_cik = o.issuer_cik
                                 ORDER BY i.accession DESC LIMIT 1), ''),
                       (SELECT s.symbol FROM issuer_price_series s WHERE s.issuer_cik = o.issuer_cik),
                       o.owner_cik, o.traded_on::text, o.known_on::text, o.value, o.entry_day::text,
                       o.r21, o.r63, o.r126, o.r252, o.x21, o.x63, o.x126, o.x252
                FROM outcome o";
    let rows = match of {
        OutcomesOf::Owner(c) => client.query(
            &format!("{base} WHERE o.owner_cik = $1 ORDER BY o.known_on"),
            &[c],
        )?,
        OutcomesOf::Issuer(c) => client.query(
            &format!(
                "{base} WHERE o.issuer_cik = $1 AND o.kind = 'event'
                       AND o.family <> 'baseline.any_month' ORDER BY o.known_on"
            ),
            &[c],
        )?,
        OutcomesOf::Extremes {
            family,
            best,
            limit,
        } => client.query(
            &format!(
                "{base} WHERE o.family = $1 AND o.x252 IS NOT NULL
                 ORDER BY o.x252 {} LIMIT $2",
                if *best { "DESC" } else { "ASC" }
            ),
            &[family, limit],
        )?,
    };
    Ok(rows
        .iter()
        .map(|r| OutcomeRow {
            kind: r.get(0),
            reference: r.get(1),
            family: r.get(2),
            issuer_cik: r.get(3),
            issuer_name: r.get(4),
            ticker: r
                .get::<_, Option<String>>(5)
                .map(|s| s.trim_end_matches(".us").to_ascii_uppercase()),
            owner_cik: r.get(6),
            traded_on: r.get(7),
            known_on: r.get(8),
            value: r.get(9),
            entry_day: r.get(10),
            returns: std::array::from_fn(|i| r.get(11 + i)),
        })
        .collect())
}

/// A Stooq symbol and its closes: (`YYYY-MM-DD`, close), oldest first.
pub type PriceSeries = (String, Vec<(String, f64)>);

/// A company's daily closes, when its series passed the check.
pub fn prices_for(client: &mut Client, issuer_cik: i64) -> Result<Option<PriceSeries>, StoreError> {
    check_schema(client)?;
    let Some(row) = client.query_opt(
        "SELECT symbol FROM issuer_price_series WHERE issuer_cik = $1 AND status = 'ok'",
        &[&issuer_cik],
    )?
    else {
        return Ok(None);
    };
    let symbol: String = row.get(0);
    let days = client
        .query(
            "SELECT day::text, close FROM price_daily WHERE symbol = $1 ORDER BY day",
            &[&symbol],
        )?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    Ok(Some((symbol, days)))
}

/// What was imported and checked, for the page's coverage note.
#[derive(Debug, Clone, PartialEq)]
pub struct PriceStatus {
    pub imported_at: Option<String>,
    pub source: Option<String>,
    pub symbols: i64,
    pub first_day: Option<String>,
    pub last_day: Option<String>,
    pub series_ok: i64,
    pub series_mismatch: i64,
    pub series_unchecked: i64,
    pub outcomes: i64,
    pub benchmark: bool,
}

pub fn price_status(client: &mut Client) -> Result<PriceStatus, StoreError> {
    check_schema(client)?;
    let r = client.query_one(
        "SELECT (SELECT to_char(imported_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI') FROM price_import ORDER BY imported_at DESC LIMIT 1),
                (SELECT source FROM price_import ORDER BY imported_at DESC LIMIT 1),
                (SELECT coalesce(max(symbols), 0)::bigint FROM (SELECT symbols FROM price_import ORDER BY imported_at DESC LIMIT 1) x),
                (SELECT first_day::text FROM price_import ORDER BY imported_at DESC LIMIT 1),
                (SELECT last_day::text FROM price_import ORDER BY imported_at DESC LIMIT 1),
                (SELECT count(*) FROM issuer_price_series WHERE status = 'ok'),
                (SELECT count(*) FROM issuer_price_series WHERE status = 'mismatch'),
                (SELECT count(*) FROM issuer_price_series WHERE status = 'unchecked'),
                (SELECT count(*) FROM outcome),
                EXISTS (SELECT 1 FROM price_daily WHERE symbol = $1)",
        &[&BENCHMARK],
    )?;
    Ok(PriceStatus {
        imported_at: r.get(0),
        source: r.get(1),
        symbols: r.get(2),
        first_day: r.get(3),
        last_day: r.get(4),
        series_ok: r.get(5),
        series_mismatch: r.get(6),
        series_unchecked: r.get(7),
        outcomes: r.get(8),
        benchmark: r.get(9),
    })
}
