# ADR 0002: An append-only ledger with two dates per fact

**Status:** accepted · **Date:** 2026-11-19

## Context

Ownership data is revised: amendments, late filings, confidential 13F positions revealed months later. If a later version overwrites an earlier one, any backtest silently uses information that was not public at the time (look-ahead bias), and the evidence studies become fiction.

## Decision

- Every fact stores `valid_on` (when it was true) and `known_on` (when the SEC accepted the filing that disclosed it).
- Facts are never updated or deleted. An amendment is a new fact with `supersedes` pointing at the one it corrects.
- All analysis asks "as known on date X". The in-memory `oe-ledger` defines the rules and tests; `migrations/0001_init.sql` enforces the same rules in PostgreSQL with triggers and an `insider_transaction_as_of` function.
- Events carry their own `known_on`: the moment they became detectable.

## Consequences

- Storage grows with every amendment. That is acceptable; ownership data is small.
- Queries are slightly more complex. The `as_of` functions hide that.
- Evidence studies can be rerun for any past date and give the same answer.
