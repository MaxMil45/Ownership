# ADR 0004: A separate crate for the PostgreSQL loader

**Status:** accepted · **Date:** 2026-11-19

## Context

ADR 0001 keeps the core crates on the standard library. The live ledger needs a PostgreSQL driver, which brings dozens of transitive dependencies. Bulk loading also needs `COPY`, a hash of each raw filing, and a schema runner.

## Decision

- All database code lives in a new crate, `oe-store`. It is the only crate that depends on a driver (`postgres`, synchronous, no async runtime, no TLS).
- The row mapping (`oe-store::rows`) and a small SHA-256 (`oe-store::sha256`) are plain Rust and are tested offline. Only `oe-store::db` needs a server.
- `oe-cli` reaches it through a `db` feature (on by default, like `net`). `cargo build --no-default-features` still builds without the driver.
- The schema is `migrations/0001_init.sql`, compiled into the binary and applied by `oe db-migrate`, which records versions in a `schema_migration` table.
- A load is `COPY` into temporary tables, then `INSERT ... ON CONFLICT DO NOTHING` into the ledger, one transaction per batch. Repeating a load changes nothing. A stored filing whose raw hash now differs is reported and never overwritten.
- Tests that need a server are `#[ignore]`d and read `OE_TEST_DATABASE_URL`; each makes a private schema and drops it.
- For development the server is PostgreSQL 16 in Docker (`docker/compose.yaml`): bound to 127.0.0.1, a placeholder password, data in a named volume, and an empty `ownership_test` database created on first start.

## Choices the schema forced

- `filing.filer_cik` is the first reporting owner; the issuer is on each transaction row. A joint filing's co-owners are not stored yet.
- A `4/A` or `5/A` is stored as a `filing` row (the receipt) but its transaction rows are **not loaded**. The ledger links an amendment to the row it replaces (`supersedes_id`), a 4/A does not say which filing it amends, and a guessed link would show both versions in `insider_transaction_as_of`. Revisit when amendments can be linked with evidence.
- `accepted_at` comes from the sidecar. Eastern timestamps (from `oe daily-form4` and `oe follow`) are sent to PostgreSQL as `... America/New_York`, so the server applies daylight saving. A filing with no acceptance time is rejected, not given an invented one.

## Consequences

- No TLS: use a local or trusted-network server. Add TLS when a remote database is needed.
- Row counts and the 4/A rule are visible in `oe db-load --dry-run`, which needs no database.
- Bulk history since 2015 is a download problem, not a loading one (see ROADMAP).
