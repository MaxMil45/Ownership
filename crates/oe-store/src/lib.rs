//! Loads parsed ownership filings into the PostgreSQL point-in-time ledger
//! (`migrations/`).
//!
//! * [`rows`]: filing XML + sidecar → table rows. Pure, tested offline.
//! * [`amend`]: may an amendment's rows replace the report it amends? Pure,
//!   tested offline (ADR 0005).
//! * `sha256_hex` (from `oe-core`): fingerprint of each raw filing.
//! * [`db`]: connect, migrate, load in batches, link amendments, status.
//!   Needs a PostgreSQL server.
//!
//! This is the only crate in the workspace that depends on a database driver,
//! which keeps the core crates dependency-free (ADR 0001, ADR 0004).

pub mod amend;
pub mod db;
pub mod other;
pub mod rows;

pub use amend::{compare, RowShape, Verdict};
pub use db::{
    check_schema, connect, flow_by_day, link_amendments, link_pass, load_batch, load_batch_with,
    migrate, status, trade_history, FlowDay, HistoryTrade, LinkReport, LoadReport, Status,
    StoreError, MIGRATIONS,
};
pub use oe_core::sha256_hex;
pub use other::{
    adoption_history, contest_history, form144_history, load_other, prepare_other,
    schedule13g_history, ContestHeader, OtherDoc, OtherFiling, OtherReport, Stored13G,
    StoredAdoption, StoredContest, StoredNotice,
};
pub use rows::{prepare, prepare_bulk, FilingRow, PrepareError, Prepared, Sidecar, TransactionRow};

/// The database connection type, re-exported so callers need no driver dependency.
pub use postgres::Client;
