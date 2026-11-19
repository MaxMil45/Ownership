//! The PostgreSQL side: apply the schema, load prepared filings, report status.
//!
//! Loading uses `COPY` into temporary staging tables, then one `INSERT ...
//! SELECT ... ON CONFLICT DO NOTHING` per table, so a load is fast, atomic per
//! batch, and safe to repeat: filings and rows already present are left
//! exactly as they are (the ledger is append-only, ADR 0002).
//!
//! Connections use no TLS, so point `DATABASE_URL` at a local or trusted-network
//! server only.

use crate::rows::Prepared;
use postgres::{Client, NoTls};
use std::fmt;
use std::io::Write;

/// Schema migrations in order. The SQL is compiled in, so the binary does not
/// depend on where it is run from.
pub const MIGRATIONS: &[(&str, &str)] = &[(
    "0001_init",
    include_str!("../../../migrations/0001_init.sql"),
)];

#[derive(Debug)]
pub enum StoreError {
    Db(postgres::Error),
    Io(std::io::Error),
    /// The schema has not been applied (run the migrations first).
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
            StoreError::NoSchema => {
                f.write_str("the ledger tables do not exist yet; run `oe db-migrate` first")
            }
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
    accepted_at text, source_url text, raw_sha256 text
) ON COMMIT DROP;
CREATE TEMP TABLE stage_tx (
    accession text, row_in_filing integer, issuer_cik bigint, owner_cik bigint,
    is_derivative boolean, transaction_date date, code text, shares text,
    price_per_share text, acquired_disposed text, shares_after text,
    direct_indirect text, plan_10b5_1 text
) ON COMMIT DROP;";

const INSERT_FILING: &str = "
INSERT INTO filing (accession, form_type, filer_cik, accepted_at, source_url, raw_sha256)
SELECT accession, form_type, filer_cik, accepted_at::timestamptz, source_url, raw_sha256
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

/// Filings whose stored raw hash differs from the one just computed: the
/// source changed after it was first loaded. Nothing is overwritten.
const CHANGED_SOURCE: &str = "
SELECT s.accession
FROM stage_filing s
JOIN filing f ON f.accession = s.accession
WHERE f.raw_sha256 <> s.raw_sha256
ORDER BY s.accession";

#[derive(Debug, Default, PartialEq, Eq)]
pub struct LoadReport {
    pub filings_new: u64,
    pub filings_existing: u64,
    pub rows_new: u64,
    pub rows_existing: u64,
    /// Amendment rows deliberately not loaded (see `rows` module docs).
    pub held_back_amendment_rows: u64,
    /// Accession numbers already stored whose raw XML now hashes differently.
    pub changed_source: Vec<String>,
}

impl LoadReport {
    pub fn add(&mut self, other: LoadReport) {
        self.filings_new += other.filings_new;
        self.filings_existing += other.filings_existing;
        self.rows_new += other.rows_new;
        self.rows_existing += other.rows_existing;
        self.held_back_amendment_rows += other.held_back_amendment_rows;
        self.changed_source.extend(other.changed_source);
    }
}

/// Loads one batch in a single transaction: either all of it or none.
pub fn load_batch(client: &mut Client, batch: &[Prepared]) -> Result<LoadReport, StoreError> {
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
            for t in &p.transactions {
                w.write_all(t.copy_line().as_bytes())?;
                staged_rows += 1;
            }
        }
        w.finish()?;
    }

    let filings_new = tx.execute(INSERT_FILING, &[])?;
    let rows_new = tx.execute(INSERT_TX, &[])?;
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
        held_back_amendment_rows: batch
            .iter()
            .map(|p| p.held_back_amendment_rows as u64)
            .sum(),
        changed_source,
    })
}

#[derive(Debug, PartialEq, Eq)]
pub struct Status {
    pub migrations: Vec<String>,
    pub filings: i64,
    pub amendment_filings: i64,
    pub transactions: i64,
    /// Rows visible through `insider_transaction_as_of(now())`.
    pub transactions_known_now: i64,
    pub earliest_accepted: Option<String>,
    pub latest_accepted: Option<String>,
}

pub fn status(client: &mut Client) -> Result<Status, StoreError> {
    let has_schema: bool = client
        .query_one("SELECT to_regclass('filing') IS NOT NULL", &[])?
        .get(0);
    if !has_schema {
        return Err(StoreError::NoSchema);
    }
    let migrations = client
        .query("SELECT version FROM schema_migration ORDER BY version", &[])?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let one = |client: &mut Client, sql: &str| -> Result<i64, StoreError> {
        Ok(client.query_one(sql, &[])?.get(0))
    };
    let filings = one(client, "SELECT count(*) FROM filing")?;
    let amendment_filings = one(
        client,
        "SELECT count(*) FROM filing WHERE form_type LIKE '%/A'",
    )?;
    let transactions = one(client, "SELECT count(*) FROM insider_transaction")?;
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
        amendment_filings,
        transactions,
        transactions_known_now,
        earliest_accepted: range.get(0),
        latest_accepted: range.get(1),
    })
}
