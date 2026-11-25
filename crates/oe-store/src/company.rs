//! One company as the ledger sees it (GET /v1/company): its name and
//! ticker as filed, its insiders and their roles, its latest open-market
//! trades and its daily buying and selling.

use crate::db::{check_schema, StoreError};
use crate::people::Receipt;
use oe_core::{MAX_PLAUSIBLE_PRICE, MAX_PLAUSIBLE_ROW_VALUE};
use postgres::Client;

#[derive(Debug, Clone, PartialEq)]
pub struct CompanyInsider {
    pub owner_cik: i64,
    pub name: String,
    pub director: bool,
    pub officer: bool,
    pub ten_percent_owner: bool,
    pub other: bool,
    /// Officer titles as filed.
    pub titles: Vec<String>,
    pub first_on: String,
    pub last_on: String,
    pub filings: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct CompanyTrade {
    pub transaction_date: String,
    pub code: String,
    pub owner_name: String,
    pub shares: Option<f64>,
    pub price: Option<f64>,
    pub receipt: Receipt,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Company {
    pub cik: i64,
    pub name: String,
    pub ticker: Option<String>,
    pub filings: i64,
    pub first_on: Option<String>,
    pub last_on: Option<String>,
    /// Most recent first, at most 30.
    pub insiders: Vec<CompanyInsider>,
    /// Latest open-market buys (P) and sales (S), at most 20.
    pub trades: Vec<CompanyTrade>,
    /// Per US Eastern day: (day, buy value, buys, sell value, sells), plausible
    /// amounts only, as in `flow_by_day`.
    pub flow: Vec<(String, f64, i64, f64, i64)>,
}

const DAY: &str = "(f.accepted_at AT TIME ZONE 'America/New_York')::date";

/// `None` if the ledger has no Form 3/4/5 for this company.
pub fn company(client: &mut Client, cik: i64) -> Result<Option<Company>, StoreError> {
    check_schema(client)?;
    let head = client.query_one(
        &format!(
            "SELECT (array_agg(i.issuer_name ORDER BY f.accepted_at DESC))[1],
                    (array_agg(i.ticker ORDER BY f.accepted_at DESC) FILTER (WHERE i.ticker IS NOT NULL))[1],
                    count(*), min({DAY})::text, max({DAY})::text
             FROM ownership_issuer i JOIN filing f USING (accession)
             WHERE i.issuer_cik = $1"
        ),
        &[&cik],
    )?;
    let filings: i64 = head.get(2);
    if filings == 0 {
        return Ok(None);
    }
    let insiders = client
        .query(
            &format!(
                "SELECT r.owner_cik,
                        (array_agg(r.owner_name ORDER BY f.accepted_at DESC))[1],
                        bool_or(r.is_director), bool_or(r.is_officer),
                        bool_or(r.is_ten_percent_owner), bool_or(r.is_other),
                        array_remove(array_agg(DISTINCT r.officer_title), NULL),
                        min({DAY})::text, max({DAY})::text, count(DISTINCT r.accession)
                 FROM ownership_issuer i
                 JOIN filing_owner_role r USING (accession)
                 JOIN filing f USING (accession)
                 WHERE i.issuer_cik = $1
                 GROUP BY r.owner_cik
                 ORDER BY max(f.accepted_at) DESC
                 LIMIT 30"
            ),
            &[&cik],
        )?
        .iter()
        .map(|r| CompanyInsider {
            owner_cik: r.get(0),
            name: r.get(1),
            director: r.get(2),
            officer: r.get(3),
            ten_percent_owner: r.get(4),
            other: r.get(5),
            titles: r.get(6),
            first_on: r.get(7),
            last_on: r.get(8),
            filings: r.get(9),
        })
        .collect();
    let trades = client
        .query(
            &format!(
                "SELECT t.transaction_date::text, t.code, coalesce(r.owner_name, ''),
                        t.shares::float8, t.price_per_share::float8,
                        f.accession, f.filer_cik, {DAY}::text, f.source
                 FROM insider_transaction t
                 JOIN filing f ON f.accession = t.accession
                 LEFT JOIN filing_owner_role r ON r.accession = t.accession AND r.position = 0
                 WHERE t.issuer_cik = $1 AND NOT t.is_derivative AND t.code IN ('P', 'S')
                   AND NOT EXISTS (SELECT 1 FROM insider_transaction n WHERE n.supersedes_id = t.id)
                 ORDER BY f.accepted_at DESC, t.row_in_filing
                 LIMIT 20"
            ),
            &[&cik],
        )?
        .into_iter()
        .map(|r| {
            let accession: String = r.get(5);
            let filer: i64 = r.get(6);
            let source: &str = r.get(8);
            CompanyTrade {
                transaction_date: r.get(0),
                code: r.get(1),
                owner_name: r.get(2),
                shares: r.get(3),
                price: r.get(4),
                receipt: Receipt {
                    url: oe_edgar::filing_folder_url(filer as u64, &accession.replace('-', "")),
                    accession,
                    known_on: r.get(7),
                    filing_date_only: source == "bulk",
                },
            }
        })
        .collect();
    let flow = client
        .query(
            "SELECT (t.known_at AT TIME ZONE 'America/New_York')::date::text,
                    coalesce(sum(t.shares * t.price_per_share) FILTER (WHERE t.code = 'P'), 0)::float8,
                    count(*) FILTER (WHERE t.code = 'P'),
                    coalesce(sum(t.shares * t.price_per_share) FILTER (WHERE t.code = 'S'), 0)::float8,
                    count(*) FILTER (WHERE t.code = 'S')
             FROM insider_transaction t
             WHERE t.issuer_cik = $1 AND NOT t.is_derivative
               AND ((t.code = 'P' AND t.acquired_disposed IS DISTINCT FROM 'D')
                    OR (t.code = 'S' AND t.acquired_disposed IS DISTINCT FROM 'A'))
               AND coalesce(t.price_per_share <= $2::float8::numeric, true)
               AND coalesce(t.shares * t.price_per_share <= $3::float8::numeric, true)
               AND NOT EXISTS (SELECT 1 FROM insider_transaction n WHERE n.supersedes_id = t.id)
             GROUP BY 1 ORDER BY 1",
            &[&cik, &MAX_PLAUSIBLE_PRICE, &MAX_PLAUSIBLE_ROW_VALUE],
        )?
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3), r.get(4)))
        .collect();
    Ok(Some(Company {
        cik,
        name: head.get(0),
        ticker: head.get(1),
        filings,
        first_on: head.get(3),
        last_on: head.get(4),
        insiders,
        trades,
        flow,
    }))
}

/// Companies whose name or ticker starts with `prefix` (case ignored), most
/// filed-about first: for a company search box. At most `limit`.
pub fn find_companies(
    client: &mut Client,
    prefix: &str,
    limit: i64,
) -> Result<Vec<(i64, String, Option<String>)>, StoreError> {
    check_schema(client)?;
    let p = prefix.trim();
    if p.is_empty() {
        return Ok(Vec::new());
    }
    let pattern = format!(
        "{}%",
        p.replace('\\', "\\\\")
            .replace('%', "\\%")
            .replace('_', "\\_")
    );
    Ok(client
        .query(
            "SELECT issuer_cik, issuer_name, ticker FROM (
                 SELECT DISTINCT ON (issuer_cik) issuer_cik, issuer_name, ticker, n
                 FROM (
                     SELECT e.issuer_cik, e.issuer_name, e.ticker, count(*) OVER (PARTITION BY e.issuer_cik) AS n
                     FROM event_feed e
                     WHERE e.issuer_cik IS NOT NULL
                       AND (e.issuer_name ILIKE $1 OR upper(e.ticker) = upper($2))
                 ) x
                 ORDER BY issuer_cik, (ticker IS NULL), n DESC
             ) y
             ORDER BY n DESC
             LIMIT $3",
            &[&pattern, &p, &limit],
        )?
        .iter()
        .map(|r| (r.get(0), r.get::<_, Option<String>>(1).unwrap_or_default(), r.get(2)))
        .collect())
}

/// Whose trades [`trade_marks`] returns.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TradesOf {
    /// Every insider of this company.
    Issuer(i64),
    /// This reporting owner, at every company (joint filings included).
    Owner(i64),
}

/// One insider's open-market buys (P) or sales (S) of one company on one
/// day, summed: a point on the price chart and the unit the summaries count.
#[derive(Debug, Clone, PartialEq)]
pub struct TradeMark {
    /// Transaction date, `YYYY-MM-DD`.
    pub day: String,
    pub issuer_cik: i64,
    pub issuer_name: String,
    pub ticker: Option<String>,
    pub owner_cik: i64,
    pub owner_name: String,
    /// `P` or `S`.
    pub code: String,
    pub trades: i64,
    pub shares: f64,
    /// Shares × price of the rows whose amounts are plausible (0 if none).
    pub value: f64,
    /// Shares behind `value`, so `value / priced_shares` is the average price.
    pub priced_shares: f64,
    /// Rows from filings with the Rule 10b5-1 box ticked / with the box at
    /// all (it exists from April 2023).
    pub plan_trades: i64,
    pub plan_known: i64,
    /// The first filing reporting them.
    pub receipt: Receipt,
}

/// Every open-market buy and sale, as the ledger holds them now (amended rows
/// once), grouped by day, owner and code, oldest first. Transaction dates
/// after the filing or more than a year before it are typos as filed and
/// left out, as in [`crate::ledger_purchases`].
pub fn trade_marks(client: &mut Client, of: TradesOf) -> Result<Vec<TradeMark>, StoreError> {
    check_schema(client)?;
    let (join, filter, id) = match of {
        TradesOf::Issuer(c) => ("", "t.issuer_cik = $1", c),
        TradesOf::Owner(c) => (
            "JOIN filing_owner fo ON fo.accession = t.accession AND fo.owner_cik = $1",
            "true",
            c,
        ),
    };
    let owner = match of {
        TradesOf::Issuer(_) => "t.owner_cik",
        TradesOf::Owner(_) => "$1::bigint",
    };
    let sql = format!(
        "WITH r AS (
             SELECT t.transaction_date AS day, t.issuer_cik, {owner} AS owner_cik, t.code,
                    t.shares, t.price_per_share AS price, t.plan_10b5_1,
                    (t.price_per_share IS NOT NULL AND t.shares IS NOT NULL
                     AND t.price_per_share > 0
                     AND t.price_per_share <= $2::float8::numeric
                     AND t.shares * t.price_per_share <= $3::float8::numeric) AS priced,
                    f.accession, f.filer_cik, f.source, f.accepted_at,
                    i.issuer_name, i.ticker, ro.owner_name
             FROM insider_transaction t
             {join}
             JOIN filing f ON f.accession = t.accession
             LEFT JOIN ownership_issuer i ON i.accession = t.accession
             LEFT JOIN filing_owner_role ro ON ro.accession = t.accession AND ro.owner_cik = {owner}
             WHERE {filter} AND NOT t.is_derivative
               AND ((t.code = 'P' AND t.acquired_disposed IS DISTINCT FROM 'D')
                    OR (t.code = 'S' AND t.acquired_disposed IS DISTINCT FROM 'A'))
               AND t.transaction_date <= (f.accepted_at AT TIME ZONE 'America/New_York')::date
               AND t.transaction_date >= (f.accepted_at AT TIME ZONE 'America/New_York')::date - 365
               AND NOT EXISTS (SELECT 1 FROM insider_transaction n WHERE n.supersedes_id = t.id)
         )
         SELECT day::text, issuer_cik,
                coalesce((array_agg(issuer_name ORDER BY accepted_at DESC) FILTER (WHERE issuer_name IS NOT NULL))[1], ''),
                (array_agg(ticker ORDER BY accepted_at DESC) FILTER (WHERE ticker IS NOT NULL))[1],
                owner_cik,
                coalesce((array_agg(owner_name ORDER BY accepted_at DESC) FILTER (WHERE owner_name IS NOT NULL))[1], ''),
                code, count(*),
                coalesce(sum(shares), 0)::float8,
                coalesce(sum(shares * price) FILTER (WHERE priced), 0)::float8,
                coalesce(sum(shares) FILTER (WHERE priced), 0)::float8,
                count(*) FILTER (WHERE plan_10b5_1), count(*) FILTER (WHERE plan_10b5_1 IS NOT NULL),
                (array_agg(accession ORDER BY accepted_at, accession))[1],
                (array_agg(filer_cik ORDER BY accepted_at, accession))[1],
                (array_agg(source ORDER BY accepted_at, accession))[1],
                (min(accepted_at) AT TIME ZONE 'America/New_York')::date::text
         FROM r
         GROUP BY day, issuer_cik, owner_cik, code
         ORDER BY day, issuer_cik, owner_cik, code"
    );
    Ok(client
        .query(&sql, &[&id, &MAX_PLAUSIBLE_PRICE, &MAX_PLAUSIBLE_ROW_VALUE])?
        .into_iter()
        .map(|r| {
            let accession: String = r.get(13);
            let filer: i64 = r.get(14);
            let source: &str = r.get(15);
            TradeMark {
                day: r.get(0),
                issuer_cik: r.get(1),
                issuer_name: r.get(2),
                ticker: r.get(3),
                owner_cik: r.get(4),
                owner_name: r.get(5),
                code: r.get(6),
                trades: r.get(7),
                shares: r.get(8),
                value: r.get(9),
                priced_shares: r.get(10),
                plan_trades: r.get(11),
                plan_known: r.get(12),
                receipt: Receipt {
                    url: oe_edgar::filing_folder_url(filer as u64, &accession.replace('-', "")),
                    accession,
                    known_on: r.get(16),
                    filing_date_only: source == "bulk",
                },
            }
        })
        .collect())
}
