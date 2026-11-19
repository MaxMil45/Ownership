# Roadmap

The full plan lives in the "Ownership Event Intelligence Engine — Master Plan" doc. This file tracks what is done in the repository.

## Phase 1: insider and activist alerts (Dec 2026 – Feb 2027)

- [x] Cargo workspace, core types, CI
- [x] Form 3/4/5 parser with synthetic edge-case fixtures
- [x] Point-in-time ledger model + PostgreSQL schema draft
- [x] `insider.cluster_buy@1` rule with spec and tests
- [x] Rate-limited EDGAR client with User-Agent checks
- [x] SEC submissions API client; `recent-form4` downloads a company's latest Form 4s with acceptance dates as `known_on`
- [x] First 20 real Form 4 fixtures (24 filings: joint filers, Forms 3 and 5, gifts, tax withholding, amendments; manifest-checked test)
- [x] Follow `filings.files` for history older than the recent list; `known_on` is now the US Eastern date (the SEC timestamp is UTC)
- [x] EDGAR daily index and live feed follower across all companies (`oe daily-form4`, `oe follow`; saves files only, no database yet; alert latency not measured)
- [ ] Bulk backfill of Forms 3/4/5 since 2015 into PostgreSQL
  - [x] Loader: `oe db-migrate`, `oe db-load`, `oe db-status` (crate `oe-store`); `--dry-run` checked on 325 real filings
  - [x] Run the loader and the database tests against a real PostgreSQL server (PostgreSQL 16 in Docker, `docker/compose.yaml`; 6 of 6 tests pass; 332 real filings and 871 rows loaded and cross-checked with `psql`)
  - [ ] Download the history: about 2.2 million filings since 2015 is roughly 75 hours at 8 requests per second through `daily-form4` (an estimate, not measured). The SEC's bulk "insider transactions" data sets may be faster; not evaluated yet.
  - [ ] Link amendments (4/A, 5/A) to the rows they replace, then load their rows; store co-owners of joint filings
- [x] Form 144 parser and `insider.planned_sale@1` (`oe-form144`, `oe daily-form144`, `oe planned-sales`; run on 274 real filings; the insider test misses 28% of large notices, see the spec; not in the database yet)
- [x] Item 408 extraction from 10-Q/10-K Inline XBRL and `insider.plan_terminated@1` (`oe-item408`, `oe daily-periodic`, `oe plan-terminations`; one full day crawled: 647 reports, 25 events; 10 real fixtures; only one day and one 10-K checked; not in the database yet)
- [x] Schedule 13D/13G parser; `activist.new_13d@1`, `activist.passive_to_active@1` (`oe-sched13`, `oe daily-13dg`, `oe activist-events`; 382 real filings parse; 31 new_13d and 3 passive_to_active events in the sample; 14 real fixtures; the passive-to-active rule trusts the filer's own flag and does not look up earlier 13Gs; not in the database yet)
- [x] Proxy contest filings; `activist.proxy_contest@1` (`oe daily-proxy`, `oe proxy-contests`; reads the SGML header, not the documents; 70 real filings, 26 contests; one event per company per batch, so it repeats across days until the history is in the database)
- [ ] Alerts: email, webhook, Discord; watchlists by ticker and insider
- [x] Clean-up (2026-11-19): shared helpers for flags, numbers, dates, entities, tags, sidecars and CLI arguments (one copy each); `insider.cluster_buy@1` routine filter indexed (96,000 trades: 8.98 s to 17 ms); outputs on all real data unchanged
- [ ] **Gate 1:** public v0.1 release; alerts within 5 minutes of filing; fixture suite green

## Phase 2: investor registry (Mar – May 2027)
## Phase 3: institutions (Jun – Aug 2027)
## Phase 4: evidence studies (Jul – Sep 2027)
## Phase 5: add-on modules and v1.0 launch (Sep – Nov 2027)

Details for later phases are in the master plan; they are copied here as each phase starts.
