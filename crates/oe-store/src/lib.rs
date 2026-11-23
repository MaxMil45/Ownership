//! Loads parsed ownership filings into the PostgreSQL point-in-time ledger
//! (`migrations/`).
//!
//! * [`rows`]: filing XML + sidecar → table rows. Pure, tested offline.
//! * [`amend`]: may an amendment's rows replace the report it amends? Pure,
//!   tested offline (ADR 0005).
//! * [`sha256`]: fingerprint of each raw filing, standard library only.
//! * [`db`]: connect, migrate, load in batches, link amendments, status.
//!   Needs a PostgreSQL server.
//!
//! This is the only crate in the workspace that depends on a database driver,
//! which keeps the core crates dependency-free (ADR 0001, ADR 0004).

pub mod amend;
pub mod db;
pub mod other;
pub mod rows;
pub mod sha256;

pub use amend::{compare, RowShape, Verdict};
pub use db::{
    check_schema, connect, flow_by_day, link_amendments, link_pass, load_batch, load_batch_with,
    migrate, status, trade_history, FlowDay, HistoryTrade, LinkReport, LoadReport, Status,
    StoreError, MIGRATIONS,
};
pub use other::{
    load_other, prepare_other, schedule13g_history, ContestHeader, OtherDoc, OtherFiling,
    OtherReport, Stored13G,
};
pub use rows::{prepare, prepare_bulk, FilingRow, PrepareError, Prepared, Sidecar, TransactionRow};
pub use sha256::sha256_hex;

/// The database connection type, re-exported so callers need no driver dependency.
pub use postgres::Client;
