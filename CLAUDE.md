# Ownership Engine: instructions for Claude Code

Open-source Rust project. It reads US ownership filings (SEC Form 3/4/5, 144, 13D/G, 13F...), resolves investors, detects meaningful events with versioned rules, and alerts with evidence ("receipts").

## Ground rules
- Work ONLY inside this folder (`C:\Users\mmurp\Ownership`). Do not touch anything outside it.
- Follow `ROADMAP.md` and the ADRs in `docs/adr/`. Phase 1 = insider + activist alerts.
- Core crates are std-only (ADR 0001). Network code lives behind the `net` feature in `oe-edgar` (ureq).
- The ledger is append-only and point-in-time: `valid_on` vs `known_on` (SEC acceptance date). Amendments supersede, never overwrite (ADR 0002).
- Event rules are versioned (`insider.cluster_buy@1`); specs live in `events/specs/`. Change a rule = new version.
- SEC fair access: User-Agent with name+email (`OE_USER_AGENT` env var), at most 8 req/s. Never bypass the rate limiter.
- Never use real third-party price data, CUSIPs lists or donor data in the repo (licensing; see ADR 0003, undecided).
- Keep `cargo fmt`, `cargo clippy -- -D warnings` and `cargo test --workspace` green, plus a build without the `net` feature.

## Layout
crates/oe-core (dates, ids, models), oe-form4 (XML tokenizer + parser), oe-ledger, oe-events, oe-edgar (JSON, submissions API, rate limiter, client), oe-cli.
Fixtures: `fixtures/form4/synthetic/` (invented), `fixtures/form4/real/` (real filings, add with `.meta.txt`).

## State
- Done: workspace scaffold, Form 4 parser, ledger, cluster-buy rule, SEC submissions reader, `oe recent-form4`, `cluster-buys`.
- NOT yet verified: the HTTP client and `recent-form4` have never run against the real SEC (the earlier environment had no network). First task: run them, fix anything that breaks.
- Any "the parser failed" filing becomes a new real fixture plus a test.

## Next steps (in order)
1. DONE (2026-11-19): tests pass, `recent-form4` ran against the real SEC, 339 real filings parsed with no failures.
2. DONE (2026-11-19): 24 real fixtures in `fixtures/form4/real/`, listed in `MANIFEST.tsv` and checked by `crates/oe-form4/tests/real_fixtures.rs`.
3. Follow `filings.files` pagination for older history; check acceptanceDateTime time zone.
4. EDGAR daily index / live feed follower.
5. Bulk backfill into PostgreSQL (`migrations/0001_init.sql`).
6. Form 144 parser + `insider.planned_sale@1`; Item 408 + `insider.plan_terminated@1`.
7. 13D/13G parsers + activist events; proxy-contest events; alerts and watchlists.
8. Gate 1: public v0.1 by end of Feb 2027. Before publishing, replace the placeholder `LICENSE` with the official Apache-2.0 text.

Do not start a new step without the user confirming.
