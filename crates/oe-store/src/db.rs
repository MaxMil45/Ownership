//! The PostgreSQL side: apply the schema, load prepared filings, report status.
//!
//! Loading uses `COPY` into temporary staging tables, then one `INSERT ...
//! SELECT ... ON CONFLICT DO NOTHING` per table, so a load is fast, atomic per
//! batch, and safe to repeat: filings and rows already present are left
//! exactly as they are (the ledger is append-only, ADR 0002).
//!
//! After each batch, amendments are linked to the report they amend
//! ([`link_amendments`], ADR 0005).
//!
//! Connections use no TLS, so point `DATABASE_URL` at a local or trusted-network
//! server only.

use crate::amend::{compare, RowShape, Verdict};
use crate::rows::Prepared;
use postgres::{Client, NoTls};
use std::fmt;
use std::io::Write;

/// Schema migrations in order. The SQL is compiled in, so the binary does not
/// depend on where it is run from.
pub const MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_init",
        include_str!("../../../migrations/0001_init.sql"),
    ),
    (
        "0002_amendments",
        include_str!("../../../migrations/0002_amendments.sql"),
    ),
    (
        "0003_bulk",
        include_str!("../../../migrations/0003_bulk.sql"),
    ),
    (
        "0004_link_index",
        include_str!("../../../migrations/0004_link_index.sql"),
    ),
    (
        "0005_other_forms",
        include_str!("../../../migrations/0005_other_forms.sql"),
    ),
    (
        "0006_officials",
        include_str!("../../../migrations/0006_officials.sql"),
    ),
    (
        "0007_people",
        include_str!("../../../migrations/0007_people.sql"),
    ),
    (
        "0008_event_feed",
        include_str!("../../../migrations/0008_event_feed.sql"),
    ),
    (
        "0009_lookup_indexes",
        include_str!("../../../migrations/0009_lookup_indexes.sql"),
    ),
];

#[derive(Debug)]
pub enum StoreError {
    Db(postgres::Error),
    Io(std::io::Error),
    /// The schema is missing or older than this program (run the migrations).
    NoSchema,
}

impl fmt::Display for StoreError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // postgres::Error's Display is only "db error"; its source has the message.
            StoreError::Db(e) => match std::error::Error::source(e) {
                Some(cause) => write!(f, "database error: {cause}"),
                None => write!(f, "database error: {e}"),
            },
            StoreError::Io(e) => write!(f, "I/O error while talking to the database: {e}"),
            StoreError::NoSchema => f.write_str(
                "the ledger tables are missing or out of date; run `oe db-migrate` first",
            ),
        }
    }
}

impl std::error::Error for StoreError {}

impl From<postgres::Error> for StoreError {
    fn from(e: postgres::Error) -> Self {
        StoreError::Db(e)
    }
}

impl From<std::io::Error> for StoreError {
    fn from(e: std::io::Error) -> Self {
        StoreError::Io(e)
    }
}

pub fn connect(url: &str) -> Result<Client, StoreError> {
    Ok(Client::connect(url, NoTls)?)
}

/// Applies every migration that has not been applied yet, each in its own
/// transaction, and returns the versions applied by this call.
pub fn migrate(client: &mut Client) -> Result<Vec<&'static str>, StoreError> {
    client.batch_execute(
        "CREATE TABLE IF NOT EXISTS schema_migration (
             version    TEXT PRIMARY KEY,
             applied_at TIMESTAMPTZ NOT NULL DEFAULT now()
         )",
    )?;
    let mut applied = Vec::new();
    for &(version, sql) in MIGRATIONS {
        let done = client
            .query_opt(
                "SELECT 1 FROM schema_migration WHERE version = $1",
                &[&version],
            )?
            .is_some();
        if done {
            continue;
        }
        let mut tx = client.transaction()?;
        tx.batch_execute(sql)?;
        tx.execute(
            "INSERT INTO schema_migration (version) VALUES ($1)",
            &[&version],
        )?;
        tx.commit()?;
        applied.push(version);
    }
    Ok(applied)
}

const STAGING: &str = "
CREATE TEMP TABLE stage_filing (
    accession text, form_type text, filer_cik bigint,
    accepted_at text, source_url text, raw_sha256 text, source text
) ON COMMIT DROP;
CREATE TEMP TABLE stage_tx (
    accession text, row_in_filing integer, issuer_cik bigint, owner_cik bigint,
    is_derivative boolean, transaction_date date, code text, shares text,
    price_per_share text, acquired_disposed text, shares_after text,
    direct_indirect text, plan_10b5_1 text
) ON COMMIT DROP;
CREATE TEMP TABLE stage_amend_tx (LIKE stage_tx) ON COMMIT DROP;
CREATE TEMP TABLE stage_owner (
    accession text, position integer, owner_cik bigint
) ON COMMIT DROP;
CREATE TEMP TABLE stage_ownership (
    accession text, issuer_cik bigint, date_of_original_submission date
) ON COMMIT DROP;
CREATE TEMP TABLE stage_owner_role (
    accession text, position integer, owner_cik bigint, owner_name text,
    is_director boolean, is_officer boolean, is_ten_percent_owner boolean,
    is_other boolean, officer_title text
) ON COMMIT DROP;
CREATE TEMP TABLE stage_issuer (
    accession text, issuer_cik bigint, issuer_name text, ticker text
) ON COMMIT DROP;";

const INSERT_FILING: &str = "
INSERT INTO filing (accession, form_type, filer_cik, accepted_at, source_url, raw_sha256, source)
SELECT accession, form_type, filer_cik, accepted_at::timestamptz, source_url, raw_sha256, source
FROM stage_filing
ON CONFLICT (accession) DO NOTHING";

const INSERT_TX: &str = "
INSERT INTO insider_transaction (
    accession, row_in_filing, issuer_cik, owner_cik, is_derivative,
    transaction_date, known_at, code, shares, price_per_share,
    acquired_disposed, shares_after, direct_indirect, plan_10b5_1)
SELECT s.accession, s.row_in_filing, s.issuer_cik, s.owner_cik, s.is_derivative,
       s.transaction_date, f.accepted_at, s.code, s.shares::numeric, s.price_per_share::numeric,
       s.acquired_disposed, s.shares_after::numeric, s.direct_indirect, s.plan_10b5_1::boolean
FROM stage_tx s
JOIN filing f ON f.accession = s.accession
ON CONFLICT (accession, row_in_filing) DO NOTHING";

// The side tables are filled for every staged filing whose stored hash matches
// (so a repeat load also completes filings loaded before migration 0002), and
// never from a file whose content changed.
const INSERT_OWNER: &str = "
INSERT INTO filing_owner (accession, position, owner_cik)
SELECT s.accession, s.position, s.owner_cik
FROM stage_owner s
JOIN stage_filing sf ON sf.accession = s.accession
JOIN filing f ON f.accession = s.accession AND f.raw_sha256 = sf.raw_sha256
ON CONFLICT DO NOTHING";

const INSERT_OWNERSHIP: &str = "
INSERT INTO ownership_filing (accession, issuer_cik, date_of_original_submission)
SELECT s.accession, s.issuer_cik, s.date_of_original_submission
FROM stage_ownership s
JOIN stage_filing sf ON sf.accession = s.accession
JOIN filing f ON f.accession = s.accession AND f.raw_sha256 = sf.raw_sha256
ON CONFLICT DO NOTHING";

const INSERT_OWNER_ROLE: &str = "
INSERT INTO filing_owner_role (accession, position, owner_cik, owner_name, is_director,
    is_officer, is_ten_percent_owner, is_other, officer_title)
SELECT s.accession, s.position, s.owner_cik, s.owner_name, s.is_director,
       s.is_officer, s.is_ten_percent_owner, s.is_other, s.officer_title
FROM stage_owner_role s
JOIN stage_filing sf ON sf.accession = s.accession
JOIN filing f ON f.accession = s.accession AND f.raw_sha256 = sf.raw_sha256
ON CONFLICT DO NOTHING";

const INSERT_ISSUER: &str = "
INSERT INTO ownership_issuer (accession, issuer_cik, issuer_name, ticker)
SELECT s.accession, s.issuer_cik, s.issuer_name, s.ticker
FROM stage_issuer s
JOIN stage_filing sf ON sf.accession = s.accession
JOIN filing f ON f.accession = s.accession AND f.raw_sha256 = sf.raw_sha256
ON CONFLICT DO NOTHING";

const INSERT_AMEND_TX: &str = "
INSERT INTO amendment_transaction (
    accession, row_in_filing, issuer_cik, owner_cik, is_derivative,
    transaction_date, code, shares, price_per_share,
    acquired_disposed, shares_after, direct_indirect, plan_10b5_1)
SELECT s.accession, s.row_in_filing, s.issuer_cik, s.owner_cik, s.is_derivative,
       s.transaction_date, s.code, s.shares::numeric, s.price_per_share::numeric,
       s.acquired_disposed, s.shares_after::numeric, s.direct_indirect, s.plan_10b5_1::boolean
FROM stage_amend_tx s
JOIN stage_filing sf ON sf.accession = s.accession
JOIN filing f ON f.accession = s.accession AND f.raw_sha256 = sf.raw_sha256
ON CONFLICT DO NOTHING";

/// Amendments without a link yet, oldest first, with the number of reports
/// that could be the one amended: a reporting owner in common, same issuer,
/// the base form (4 for a 4/A), accepted on `dateOfOriginalSubmission`
/// (US Eastern date) and not after the amendment. "An owner in common"
/// rather than "the same first owner": the bulk data sets do not keep the
/// XML's owner order (3 of 200 sampled filings, all joint). "Not after"
/// rather than "before": bulk filings of one day share one time (ADR 0008).
const UNLINKED_AMENDMENTS: &str = "
SELECT a.accession, a.accepted_at::text, count(o.accession), min(o.accession)
FROM filing a
JOIN ownership_filing ao ON ao.accession = a.accession
LEFT JOIN amendment_link l ON l.amendment_accession = a.accession
LEFT JOIN (filing o JOIN ownership_filing oo ON oo.accession = o.accession)
       ON oo.issuer_cik = ao.issuer_cik
      AND o.form_type = left(a.form_type, length(a.form_type) - 2)
      AND (o.accepted_at AT TIME ZONE 'America/New_York')::date = ao.date_of_original_submission
      AND o.accepted_at <= a.accepted_at
      AND o.accession <> a.accession
      AND EXISTS (SELECT 1 FROM filing_owner fa
                  JOIN filing_owner fo ON fo.owner_cik = fa.owner_cik
                  WHERE fa.accession = a.accession AND fo.accession = o.accession)
WHERE a.form_type LIKE '%/A' AND l.amendment_accession IS NULL
GROUP BY a.accession, a.accepted_at
ORDER BY a.accepted_at, a.accession";

/// The newest loaded amendment of a report, and whether it was accepted after
/// the amendment being linked (`$2`).
const LATEST_LOADED_VERSION: &str = "
SELECT l.amendment_accession, f.accepted_at > $2::text::timestamptz
FROM amendment_link l
JOIN filing f ON f.accession = l.amendment_accession
WHERE l.original_accession = $1 AND l.outcome = 'loaded'
ORDER BY f.accepted_at DESC, l.amendment_accession DESC
LIMIT 1";

const LEDGER_SHAPES: &str = "
SELECT is_derivative, code, transaction_date::text, shares::text
FROM insider_transaction WHERE accession = $1 ORDER BY row_in_filing";

const AMENDMENT_SHAPES: &str = "
SELECT is_derivative, code, transaction_date::text, shares::text
FROM amendment_transaction WHERE accession = $1 ORDER BY row_in_filing";

/// Moves an amendment's rows into the ledger. Row `i` supersedes row `i` of
/// the version it replaces; rows past its end supersede nothing.
const PROMOTE_AMENDMENT_ROWS: &str = "
INSERT INTO insider_transaction (
    accession, row_in_filing, issuer_cik, owner_cik, is_derivative,
    transaction_date, known_at, code, shares, price_per_share,
    acquired_disposed, shares_after, direct_indirect, plan_10b5_1, supersedes_id)
SELECT a.accession, a.row_in_filing, a.issuer_cik, a.owner_cik, a.is_derivative,
       a.transaction_date, f.accepted_at, a.code, a.shares, a.price_per_share,
       a.acquired_disposed, a.shares_after, a.direct_indirect, a.plan_10b5_1, c.id
FROM amendment_transaction a
JOIN filing f ON f.accession = a.accession
LEFT JOIN insider_transaction c ON c.accession = $2 AND c.row_in_filing = a.row_in_filing
WHERE a.accession = $1
ON CONFLICT (accession, row_in_filing) DO NOTHING";

const INSERT_LINK: &str = "
INSERT INTO amendment_link (amendment_accession, original_accession, replaces_accession, outcome, reason)
VALUES ($1, $2, $3, $4, $5)";

/// Filings whose stored raw hash differs from the one just computed from the
/// same kind of source: the source changed after it was first loaded.
/// Nothing is overwritten. (A filing stored from its XML and offered again
/// from a bulk data set, or the other way round, is not a change: the two
/// fingerprints are of different texts. The first one loaded is kept.)
const CHANGED_SOURCE: &str = "
SELECT s.accession
FROM stage_filing s
JOIN filing f ON f.accession = s.accession
WHERE f.raw_sha256 <> s.raw_sha256 AND f.source = s.source
ORDER BY s.accession";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LoadReport {
    pub filings_new: u64,
    pub filings_existing: u64,
    pub rows_new: u64,
    pub rows_existing: u64,
    /// Transaction rows of the amendments in the batch (kept in
    /// `amendment_transaction`; the ledger gets them only through a link).
    pub amendment_rows: u64,
    /// What linking did after the batch.
    pub links: LinkReport,
    /// Accession numbers already stored whose raw XML now hashes differently.
    pub changed_source: Vec<String>,
}

/// The outcome of one [`link_amendments`] pass.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct LinkReport {
    /// Linked and restating: their rows are now in the ledger.
    pub loaded: u64,
    /// Rows moved into the ledger by those links.
    pub rows_loaded: u64,
    /// Linked, but not a restatement (or out of order): rows held back.
    pub held_back: u64,
    /// More than one report could be the one amended: not linked.
    pub ambiguous: u64,
    /// Amendments in the database still without a candidate report (the
    /// original is not loaded, or the date does not match). Not a sum: the
    /// count after the latest pass.
    pub waiting: u64,
}

impl LinkReport {
    pub fn add(&mut self, other: &LinkReport) {
        self.loaded += other.loaded;
        self.rows_loaded += other.rows_loaded;
        self.held_back += other.held_back;
        self.ambiguous += other.ambiguous;
        self.waiting = other.waiting;
    }
}

impl LoadReport {
    pub fn add(&mut self, other: LoadReport) {
        self.filings_new += other.filings_new;
        self.filings_existing += other.filings_existing;
        self.rows_new += other.rows_new;
        self.rows_existing += other.rows_existing;
        self.amendment_rows += other.amendment_rows;
        self.links.add(&other.links);
        self.changed_source.extend(other.changed_source);
    }
}

/// Loads one batch in a single transaction: either all of it or none. Then
/// links amendments ([`link_amendments`]) in the same transaction.
pub fn load_batch(client: &mut Client, batch: &[Prepared]) -> Result<LoadReport, StoreError> {
    load_batch_with(client, batch, true)
}

/// [`load_batch`], with the amendment linking pass optional. A bulk load of
/// a quarter loads its batches without linking, then calls [`link_pass`]
/// once (ADR 0008): linking after every batch would re-examine every
/// waiting amendment each time.
pub fn load_batch_with(
    client: &mut Client,
    batch: &[Prepared],
    link: bool,
) -> Result<LoadReport, StoreError> {
    let mut tx = client.transaction()?;
    tx.batch_execute(STAGING)?;

    let mut staged_rows = 0u64;
    {
        let mut w = tx.copy_in("COPY stage_filing FROM STDIN")?;
        for p in batch {
            w.write_all(p.filing.copy_line().as_bytes())?;
        }
        w.finish()?;
    }
    {
        let mut w = tx.copy_in("COPY stage_tx FROM STDIN")?;
        for p in batch {
            for t in p.ledger_rows() {
                w.write_all(t.copy_line().as_bytes())?;
                staged_rows += 1;
            }
        }
        w.finish()?;
    }
    let mut amendment_rows = 0u64;
    {
        let mut w = tx.copy_in("COPY stage_amend_tx FROM STDIN")?;
        for p in batch {
            for t in p.amendment_rows() {
                w.write_all(t.copy_line().as_bytes())?;
                amendment_rows += 1;
            }
        }
        w.finish()?;
    }
    {
        let mut w = tx.copy_in("COPY stage_owner FROM STDIN")?;
        for p in batch {
            w.write_all(p.owner_copy_lines().as_bytes())?;
        }
        w.finish()?;
    }
    {
        let mut w = tx.copy_in("COPY stage_ownership FROM STDIN")?;
        for p in batch {
            w.write_all(p.ownership_copy_line().as_bytes())?;
        }
        w.finish()?;
    }
    {
        let mut w = tx.copy_in("COPY stage_owner_role FROM STDIN")?;
        for p in batch {
            w.write_all(p.owner_role_copy_lines().as_bytes())?;
        }
        w.finish()?;
    }
    {
        let mut w = tx.copy_in("COPY stage_issuer FROM STDIN")?;
        for p in batch {
            w.write_all(p.issuer_copy_line().as_bytes())?;
        }
        w.finish()?;
    }

    let filings_new = tx.execute(INSERT_FILING, &[])?;
    let rows_new = tx.execute(INSERT_TX, &[])?;
    tx.execute(INSERT_OWNER, &[])?;
    tx.execute(INSERT_OWNERSHIP, &[])?;
    tx.execute(INSERT_OWNER_ROLE, &[])?;
    tx.execute(INSERT_ISSUER, &[])?;
    tx.execute(INSERT_AMEND_TX, &[])?;
    let links = if link {
        link_amendments(&mut tx)?
    } else {
        LinkReport::default()
    };
    let changed_source = tx
        .query(CHANGED_SOURCE, &[])?
        .iter()
        .map(|r| r.get::<_, String>(0))
        .collect();
    tx.commit()?;

    Ok(LoadReport {
        filings_new,
        filings_existing: (batch.len() as u64).saturating_sub(filings_new),
        rows_new,
        rows_existing: staged_rows.saturating_sub(rows_new),
        amendment_rows,
        links,
        changed_source,
    })
}

/// Open-market buying and selling per day, as the public could know it now:
/// non-derivative rows with code P (acquired) and S (disposed), dated by the
/// US Eastern day their filing became known, from
/// `insider_transaction_as_of(now())` so amended rows count once.
#[derive(Debug, Clone, PartialEq)]
pub struct FlowDay {
    /// `YYYY-MM-DD`.
    pub day: String,
    pub buy_value: f64,
    pub buys: i64,
    pub sell_value: f64,
    pub sells: i64,
}

use oe_core::{MAX_PLAUSIBLE_PRICE, MAX_PLAUSIBLE_ROW_VALUE};

// Two tests: each row's own amounts, then each issuer's total for the day
// (Notis Global, Nov 2016: one filer reported over a thousand purchases of
// $9.86 billion each, every row under the row limit; see
// oe_core::MAX_PLAUSIBLE_ISSUER_DAY).
const FLOW_BY_DAY: &str = "
WITH r AS (
    SELECT (known_at AT TIME ZONE 'America/New_York')::date AS day, issuer_cik, code,
           acquired_disposed, shares * price_per_share AS value,
           coalesce(price_per_share > $1 OR shares * price_per_share > $2, false) AS bad_row
    FROM insider_transaction_as_of(now())
    WHERE NOT is_derivative AND code IN ('P', 'S')
), t AS (
    SELECT day, code, acquired_disposed, value,
           bad_row OR coalesce(sum(value) FILTER (WHERE NOT bad_row)
                               OVER (PARTITION BY issuer_cik, day) > $3, false) AS implausible
    FROM r
)
SELECT day::text,
       coalesce(sum(value) FILTER (WHERE code = 'P' AND acquired_disposed = 'A' AND implausible IS NOT TRUE), 0)::float8,
       count(*) FILTER (WHERE code = 'P' AND acquired_disposed = 'A' AND implausible IS NOT TRUE),
       coalesce(sum(value) FILTER (WHERE code = 'S' AND acquired_disposed = 'D' AND implausible IS NOT TRUE), 0)::float8,
       count(*) FILTER (WHERE code = 'S' AND acquired_disposed = 'D' AND implausible IS NOT TRUE),
       count(*) FILTER (WHERE implausible)
FROM t
GROUP BY 1
ORDER BY 1";

/// The daily flow, and the number of rows left out as implausible
/// ([`MAX_PLAUSIBLE_PRICE`], [`MAX_PLAUSIBLE_ROW_VALUE`]).
pub fn flow_by_day(client: &mut Client) -> Result<(Vec<FlowDay>, i64), StoreError> {
    check_schema(client)?;
    let price = MAX_PLAUSIBLE_PRICE.to_string();
    let value = MAX_PLAUSIBLE_ROW_VALUE.to_string();
    let sql = FLOW_BY_DAY
        .replace("$1", &format!("{price}::numeric"))
        .replace("$2", &format!("{value}::numeric"))
        .replace(
            "$3",
            &format!("{}::numeric", oe_core::MAX_PLAUSIBLE_ISSUER_DAY),
        );
    let mut left_out = 0i64;
    let days = client
        .query(sql.as_str(), &[])?
        .iter()
        .map(|r| {
            left_out += r.get::<_, i64>(5);
            FlowDay {
                day: r.get(0),
                buy_value: r.get(1),
                buys: r.get(2),
                sell_value: r.get(3),
                sells: r.get(4),
            }
        })
        .collect();
    Ok((days, left_out))
}

/// One earlier open-market trade, for the cluster-buy routine-trader filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HistoryTrade {
    pub issuer_cik: u64,
    pub owner_cik: u64,
    /// `YYYY-MM-DD`, the transaction date.
    pub date: String,
    /// `P` or `S`.
    pub code: String,
}

const TRADE_HISTORY: &str = "
SELECT DISTINCT t.issuer_cik, fo.owner_cik, t.transaction_date::text, t.code
FROM insider_transaction_as_of(now()) t
JOIN filing_owner fo ON fo.accession = t.accession
WHERE t.issuer_cik = ANY($1) AND t.transaction_date >= $2::text::date
  AND NOT t.is_derivative AND t.code IN ('P', 'S')";

/// Every open-market buy (P) and sale (S) in the ledger for these issuers
/// since `since` (`YYYY-MM-DD`), once per owner of the filing (a joint
/// filing's co-owners each count as having traded).
pub fn trade_history(
    client: &mut Client,
    issuers: &[u64],
    since: &str,
) -> Result<Vec<HistoryTrade>, StoreError> {
    check_schema(client)?;
    let ids: Vec<i64> = issuers.iter().map(|&c| c as i64).collect();
    Ok(client
        .query(TRADE_HISTORY, &[&ids, &since])?
        .iter()
        .map(|r| HistoryTrade {
            issuer_cik: r.get::<_, i64>(0) as u64,
            owner_cik: r.get::<_, i64>(1) as u64,
            date: r.get(2),
            code: r.get(3),
        })
        .collect())
}

/// One amendment linking pass in its own transaction.
pub fn link_pass(client: &mut Client) -> Result<LinkReport, StoreError> {
    let mut tx = client.transaction()?;
    let report = link_amendments(&mut tx)?;
    tx.commit()?;
    Ok(report)
}

fn shapes(
    tx: &mut postgres::Transaction<'_>,
    sql: &str,
    accession: &str,
) -> Result<Vec<RowShape>, StoreError> {
    Ok(tx
        .query(sql, &[&accession])?
        .iter()
        .map(|r| RowShape {
            is_derivative: r.get(0),
            code: r.get(1),
            date: r.get(2),
            shares: r.get(3),
        })
        .collect())
}

/// Links every amendment that has no link yet, oldest first (ADR 0005):
///
/// * no candidate report: left alone and tried again on the next load;
/// * more than one: recorded as `ambiguous`, rows held back;
/// * exactly one: its current version (the newest loaded amendment of it, or
///   the report itself) is compared with the amendment by
///   [`crate::amend::compare`]. A restatement's rows enter the ledger and
///   supersede the current version's rows; anything else is `held_back`.
///
/// Runs inside the caller's transaction. Safe to repeat.
pub fn link_amendments(tx: &mut postgres::Transaction<'_>) -> Result<LinkReport, StoreError> {
    let mut report = LinkReport::default();
    let pending = tx.query(UNLINKED_AMENDMENTS, &[])?;
    for row in &pending {
        let amendment: String = row.get(0);
        let accepted: String = row.get(1);
        let candidates: i64 = row.get(2);
        let original: Option<String> = row.get(3);
        let original = match (candidates, original) {
            (1, Some(o)) => o,
            (n, Some(_)) if n > 1 => {
                let reason = format!("{n} reports by the same owner and issuer on that date");
                tx.execute(
                    INSERT_LINK,
                    &[
                        &amendment,
                        &None::<String>,
                        &None::<String>,
                        &"ambiguous",
                        &reason,
                    ],
                )?;
                report.ambiguous += 1;
                continue;
            }
            _ => {
                report.waiting += 1;
                continue;
            }
        };

        let latest = tx.query_opt(LATEST_LOADED_VERSION, &[&original, &accepted])?;
        let (replaces, out_of_order) = match latest {
            Some(r) => (r.get::<_, String>(0), r.get::<_, bool>(1)),
            None => (original.clone(), false),
        };
        let verdict = if out_of_order {
            Verdict::HeldBack(format!(
                "a later amendment of the same report ({replaces}) was loaded first"
            ))
        } else {
            let current = shapes(tx, LEDGER_SHAPES, &replaces)?;
            let new = shapes(tx, AMENDMENT_SHAPES, &amendment)?;
            compare(&current, &new)
        };
        match verdict {
            Verdict::Restates { replaced, added } => {
                let moved = tx.execute(PROMOTE_AMENDMENT_ROWS, &[&amendment, &replaces])?;
                let reason = format!("restates {replaced} row(s) of {replaces}, adds {added}");
                tx.execute(
                    INSERT_LINK,
                    &[
                        &amendment,
                        &Some(&original),
                        &Some(&replaces),
                        &"loaded",
                        &reason,
                    ],
                )?;
                report.loaded += 1;
                report.rows_loaded += moved;
            }
            Verdict::HeldBack(reason) => {
                tx.execute(
                    INSERT_LINK,
                    &[
                        &amendment,
                        &Some(&original),
                        &None::<String>,
                        &"held_back",
                        &reason,
                    ],
                )?;
                report.held_back += 1;
            }
        }
    }
    Ok(report)
}

#[derive(Debug, PartialEq, Eq)]
pub struct Status {
    pub migrations: Vec<String>,
    pub filings: i64,
    /// Of those, loaded from the quarterly bulk data sets (ADR 0008).
    pub bulk_filings: i64,
    pub amendment_filings: i64,
    /// Filings with more than one reporting owner.
    pub joint_filings: i64,
    /// Amendments linked and loaded / linked and held back / ambiguous.
    pub amendments_loaded: i64,
    pub amendments_held_back: i64,
    pub amendments_ambiguous: i64,
    /// Amendments with no link yet (their original is not in the database).
    pub amendments_waiting: i64,
    pub transactions: i64,
    /// Rows visible through `insider_transaction_as_of(now())`.
    pub transactions_known_now: i64,
    /// Rows in the other forms' tables (migration 0005).
    pub form144_notices: i64,
    pub schedule13_filings: i64,
    pub contest_filings: i64,
    pub trading_arrangements: i64,
    pub earliest_accepted: Option<String>,
    pub latest_accepted: Option<String>,
}

/// What [`maintain`] found.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MaintainReport {
    /// `4072 MB`, as PostgreSQL writes it.
    pub database_size: String,
}

/// `VACUUM (ANALYZE)` over every table: fresh statistics for the query
/// planner and an up-to-date visibility map (index-only scans). The ledger
/// only grows, so there is little to vacuum, but after a large load, or after
/// Docker stopped the server abruptly (which resets the counters autovacuum
/// goes by), the statistics can be days behind. Safe to run at any time;
/// reads and writes go on while it runs.
pub fn maintain(client: &mut Client) -> Result<MaintainReport, StoreError> {
    check_schema(client)?;
    client.batch_execute("VACUUM (ANALYZE)")?;
    Ok(MaintainReport {
        database_size: client
            .query_one(
                "SELECT pg_size_pretty(pg_database_size(current_database()))",
                &[],
            )?
            .get(0),
    })
}

/// Fails with [`StoreError::NoSchema`] unless every migration's tables exist
/// (the first and the newest table are checked).
pub fn check_schema(client: &mut Client) -> Result<(), StoreError> {
    let has_log: bool = client
        .query_one("SELECT to_regclass('schema_migration') IS NOT NULL", &[])?
        .get(0);
    if !has_log {
        return Err(StoreError::NoSchema);
    }
    let applied: Vec<String> = client
        .query("SELECT version FROM schema_migration", &[])?
        .iter()
        .map(|r| r.get(0))
        .collect();
    if MIGRATIONS
        .iter()
        .all(|(v, _)| applied.iter().any(|a| a == v))
    {
        Ok(())
    } else {
        Err(StoreError::NoSchema)
    }
}

pub fn status(client: &mut Client) -> Result<Status, StoreError> {
    check_schema(client)?;
    let migrations = client
        .query("SELECT version FROM schema_migration ORDER BY version", &[])?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let one = |client: &mut Client, sql: &str| -> Result<i64, StoreError> {
        Ok(client.query_one(sql, &[])?.get(0))
    };
    let filings = one(client, "SELECT count(*) FROM filing")?;
    let bulk_filings = one(client, "SELECT count(*) FROM filing WHERE source = 'bulk'")?;
    let amendment_filings = one(
        client,
        "SELECT count(*) FROM filing WHERE form_type IN ('3/A', '4/A', '5/A')",
    )?;
    let joint_filings = one(
        client,
        // Owners are numbered from 0 with no gaps: a joint filing has a 1.
        "SELECT count(*) FROM filing_owner WHERE position = 1",
    )?;
    let outcome = |client: &mut Client, o: &str| -> Result<i64, StoreError> {
        Ok(client
            .query_one(
                "SELECT count(*) FROM amendment_link WHERE outcome = $1",
                &[&o],
            )?
            .get(0))
    };
    let amendments_loaded = outcome(client, "loaded")?;
    let amendments_held_back = outcome(client, "held_back")?;
    let amendments_ambiguous = outcome(client, "ambiguous")?;
    let amendments_waiting = one(
        client,
        "SELECT count(*) FROM filing f
         WHERE f.form_type IN ('3/A', '4/A', '5/A')
           AND NOT EXISTS (SELECT 1 FROM amendment_link l WHERE l.amendment_accession = f.accession)",
    )?;
    let transactions = one(client, "SELECT count(*) FROM insider_transaction")?;
    let form144_notices = one(client, "SELECT count(*) FROM form144_notice")?;
    let schedule13_filings = one(client, "SELECT count(*) FROM schedule13_filing")?;
    let contest_filings = one(client, "SELECT count(*) FROM contest_filing")?;
    let trading_arrangements = one(client, "SELECT count(*) FROM trading_arrangement")?;
    let transactions_known_now = one(
        client,
        "SELECT count(*) FROM insider_transaction_as_of(now())",
    )?;
    let range = client.query_one(
        "SELECT min(accepted_at)::text, max(accepted_at)::text FROM filing",
        &[],
    )?;
    Ok(Status {
        migrations,
        filings,
        bulk_filings,
        amendment_filings,
        joint_filings,
        amendments_loaded,
        amendments_held_back,
        amendments_ambiguous,
        amendments_waiting,
        transactions,
        transactions_known_now,
        form144_notices,
        schedule13_filings,
        contest_filings,
        trading_arrangements,
        earliest_accepted: range.get(0),
        latest_accepted: range.get(1),
    })
}
