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

- `filing.filer_cik` is the first reporting owner; the issuer is on each transaction row. A joint filing's co-owners were not stored at first; since ADR 0005 they are in `filing_owner`.
- A `4/A` or `5/A` is stored as a `filing` row (the receipt) but its transaction rows are **not loaded**. The ledger links an amendment to the row it replaces (`supersedes_id`), a 4/A does not say which filing it amends, and a guessed link would show both versions in `insider_transaction_as_of`. Revisit when amendments can be linked with evidence. **Superseded by ADR 0005** (2026-11-20): amendments are now linked by evidence, and restatements are loaded.
- `accepted_at` comes from the sidecar. Eastern timestamps (from `oe daily-form4` and `oe follow`) are sent to PostgreSQL as `... America/New_York`, so the server applies daylight saving. A filing with no acceptance time is rejected, not given an invented one.

## Consequences

- No TLS: use a local or trusted-network server. Add TLS when a remote database is needed.
- Row counts and the 4/A rule are visible in `oe db-load --dry-run`, which needs no database.
- Bulk history since 2015 is a download problem, not a loading one (see ROADMAP).

## Tuning (2026-11-25)

Measured with `docker/bench.sql` on the full ledger (4 GB, 2.6 million filings). Most tables showed no autovacuum or analyze since Docker Desktop had last stopped the server abruptly: an unclean stop resets the counters autovacuum goes by. Changes:

- Server settings in `docker/compose.yaml` for a 16 GB laptop with an SSD: `shared_buffers` 1 GB (was 128 MB), `effective_cache_size` 4 GB, `work_mem` 64 MB, `maintenance_work_mem` 512 MB, `random_page_cost` 1.1, `effective_io_concurrency` 200, four parallel workers per query, `max_wal_size` 4 GB, `jit` off; `shm_size` 1 GB for parallel queries and `stop_grace_period` 60 s for a clean shutdown.
- `oe db-maintain` (`VACUUM (ANALYZE)`), run by `scripts/update.ps1` after each load.
- Migration `0009_lookup_indexes`: a partial index on `schedule13_person (name) WHERE cik IS NULL` (holder lookup 41 to 0.4 ms) and trigram indexes (`pg_trgm`, installed in `public`) on the event feed's headline and company name (word search 128 to 1.4 ms).
- `status` counts joint filings as owners at position 1 instead of grouping every filing (4.1 s to 0.06 s).

Left as they are: the event build's two whole-ledger scans (about 1.2 and 2.8 s; run only by `events-build`) and the daily flow (1.3 s; cached ten minutes and worked out when `oe serve` starts).
