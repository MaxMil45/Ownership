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
3. DONE (2026-11-19): history pages followed (`parse_history_page`, `recent-form4` reads them newest first when the recent list is short); `acceptanceDateTime` is UTC, `known_on` is now the US Eastern date (`oe_core::us_eastern_date`). 18% of filings had a `known_on` one day late before this.
4. DONE (2026-11-19): `oe daily-form4 <date>` (daily index) and `oe follow` (latest-filings Atom feed, `owner=only`); both fetch each filing's complete submission `.txt` (XML + exact acceptance time in US Eastern). Files only, no database. Not measured: how long after acceptance the feed shows a filing.
5. PARTLY DONE (2026-11-19): loader done and verified (crate `oe-store`, `oe db-migrate|db-load|db-status`, ADR 0004). Database: PostgreSQL 16 in Docker, `docker-compose -f docker/compose.yaml up -d` (use `docker-compose` with a hyphen on this machine; the compose file is in `docker/` because the root `.env` is PowerShell syntax and breaks compose). URLs: `postgres://oe:oe_dev_only@127.0.0.1:55432/ownership` and `.../ownership_test`. The local PostgreSQL 16 install is incomplete (no initdb/pg_ctl); not used. DB tests: `cargo test -p oe-store --test postgres -- --ignored --test-threads=1` (6 pass). Still open: the since-2015 download (~75 h estimated), amendment linking, joint-filing co-owners.
6. PARTLY DONE (2026-11-19): Form 144 parser (crate `oe-form144`, 274 real filings parse) + `insider.planned_sale@1` (spec `events/specs/insider.planned_sale.v1.md`), `oe daily-form144|parse-form144|planned-sales`, 17 real fixtures in `fixtures/form144/real/` (only filings with no home address; some real Form 144s carry one). Found and fixed: the daily-index form column is 12 wide before ~2025, 17 after (detected from data). NOT done: Item 408 + `insider.plan_terminated@1` (needs Inline XBRL; separate large task); Form 144 not loaded into PostgreSQL; the insider test excludes 23 of 82 large notices.
6b. DONE (2026-11-19): Item 408 reader (crate `oe-item408`) + `insider.plan_terminated@1` (spec `events/specs/insider.plan_terminated.v1.md`); `oe daily-periodic|parse-408|plan-terminations`; 10 real excerpt fixtures in `fixtures/item408/real/` (manifest from the full originals). Crawled all 647 reports of 2026-11-05: 25 events, 3 tagged termination dates after the period were rejected, audit of 30 skipped reports found no miss. Learned: periodic-report filing date is a day late after 5:30 pm Eastern (use `.hdr.sgml`); most companies tag only the person; `fixed-false` flags and company-level `booleantrue` are not terminations. Not done: other days, non-Rule-10b5-1 arrangements, database storage.
7. PARTLY DONE (2026-11-19): Schedule 13D/13G parser (crate `oe-sched13`, 382 real filings from 5 days parse) + `activist.new_13d@1` and `activist.passive_to_active@1` (specs in `events/specs/`), `oe daily-13dg|parse-13|activist-events`, 14 real fixtures in `fixtures/sched13/real/` (entities only: 204 of 382 filings name an individual and may carry a home address). Found by real data: 13D and 13G use different element names; 36 filings prefix every element; one filing writes a single `issuerCUSIP` tag; 13D/As may omit Item 4. Proxy contests DONE (2026-11-19): `activist.proxy_contest@1` from the SGML `.hdr.sgml` (subject company vs FILED-BY; the form type does not say who filed), `oe daily-proxy|proxy-contests`, 12 real header fixtures in `fixtures/proxy/real/` (entity challengers only). Not done: alerts and watchlists, looking up earlier 13Gs, database storage of 144/13D/G/proxy/408.
   Clean-up DONE (2026-11-19): shared helpers live in `oe-core/src/text.rs` (flags, decimals, entities, tags, sidecar key=value), `oe-edgar/src/markup.rs` and `sgml.rs`, `oe-cli/src/args.rs`; reuse them instead of writing new copies. How to run: `docs/how-to-run.md` and `scripts/run-day.ps1`.
8. Gate 1: public v0.1 by end of Feb 2027. Before publishing, replace the placeholder `LICENSE` with the official Apache-2.0 text.

Do not start a new step without the user confirming.
