//! Loads parsed ownership filings into the PostgreSQL point-in-time ledger
//! (`migrations/0001_init.sql`).
//!
//! * [`rows`]: filing XML + sidecar → table rows. Pure, tested offline.
//! * [`sha256`]: fingerprint of each raw filing, standard library only.
//! * [`db`]: connect, migrate, load in batches, status. Needs a PostgreSQL server.
//!
//! This is the only crate in the workspace that depends on a database driver,
//! which keeps the core crates dependency-free (ADR 0001, ADR 0004).

pub mod db;
pub mod rows;
pub mod sha256;

pub use db::{connect, load_batch, migrate, status, LoadReport, Status, StoreError, MIGRATIONS};
pub use rows::{prepare, FilingRow, PrepareError, Prepared, Sidecar, TransactionRow};
pub use sha256::sha256_hex;

/// The database connection type, re-exported so callers need no driver dependency.
pub use postgres::Client;
