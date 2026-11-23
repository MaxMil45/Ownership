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
  - [x] Download the history: from the SEC's quarterly Insider Transactions Data Sets instead of 2.2 million single downloads (`oe-bulk`, `oe bulk-insider`, migration `0003_bulk`, ADR 0008). Checked on 200 random 2026 Q3 filings against their XML: 197 identical, 3 with joint owners in another order; numbers to two decimals; filing date = Eastern acceptance date in all. Loaded 2015 Q1 to 2026 Q3 into the dev database in 31 minutes: 2.4 million filings, 4.94 million transaction rows.
  - [x] Keep it current: the gap after the last quarterly data set filled from the daily index (`daily-form4 --to`, 6 parallel downloads within the SEC limit), and `oe follow --db` loads new Forms 3/4/5 as they arrive. The database now runs from 2015-01-02 to 2026-11-20.
  - [x] Use the history: `oe dashboard --db` (insider buying and selling over the whole ledger) and `oe alerts --db` / `oe follow --alert --db` (the cluster-buy routine-trader filter sees earlier years).
  - [x] Link amendments (3/A, 4/A, 5/A) to the report they amend and load the rows of restatements; store co-owners of joint filings (migration `0002_amendments`, ADR 0005; checked on 36 real amendments: 30 loaded, 4 held back as supplements, 2 with no matching report; 11 of 11 database tests pass). Supplements (amendments that report only the omitted rows) stay out of the ledger.
- [x] Form 144 parser and `insider.planned_sale@1` (`oe-form144`, `oe daily-form144`, `oe planned-sales`; run on 274 real filings; the insider test misses 28% of large notices, see the spec; not in the database yet)
- [x] Item 408 extraction from 10-Q/10-K Inline XBRL and `insider.plan_terminated@1` (`oe-item408`, `oe daily-periodic`, `oe plan-terminations`; one full day crawled: 647 reports, 25 events; 10 real fixtures; only one day and one 10-K checked; not in the database yet)
- [x] Schedule 13D/13G parser; `activist.new_13d@1`, `activist.passive_to_active@1` (`oe-sched13`, `oe daily-13dg`, `oe activist-events`; 382 real filings parse; 31 new_13d and 3 passive_to_active events in the sample; 14 real fixtures; the passive-to-active rule trusts the filer's own flag and does not look up earlier 13Gs; not in the database yet)
- [x] Proxy contest filings; `activist.proxy_contest@1` (`oe daily-proxy`, `oe proxy-contests`; reads the SGML header, not the documents; 70 real filings, 26 contests; one event per company per batch, so it repeats across days until the history is in the database)
- [x] Form 144, Schedule 13D/G, contested-proxy and Item 408 filings in the ledger (migration `0005_other_forms`, ADR 0009); `db-load` and `follow --db` take every kind. History downloaded and loaded (2026-11-23): 130,904 Form 144s since 2023, 63,714 13D/Gs since 2024-12-23, 10,108 contest filings since 2015, 4,409 trading arrangements from 75,516 10-Qs/10-Ks checked since 2023-10; about 20 hours of downloading at 8 requests a second.
- [x] `activist.passive_to_active@2`: also an earlier 13G from the same holder in the ledger (`--db`); 302 events instead of 237 over the 13D/G history, every `@1` event kept
- [ ] The other rules read these tables too (proxy contests once per contest rather than per batch, Form 144 history)
- [ ] Alerts: email, webhook, Discord; watchlists by ticker and insider
  - [x] `oe alerts` and `oe follow --alert` (crate `oe-alerts`, ADR 0006): every rule's events become alerts with receipts, filtered by watchlist (ticker, issuer, insider, name, rule) and age, delivered once per destination to Discord and JSON webhooks, logged in `data/alerts/sent.tsv`. Checked on real data: 51 alerts delivered to a local stand-in for Discord and a webhook, a second run sent none; one live cluster buy alerted from the feed (alert pass 44 ms). Not yet tried against a real Discord channel.
  - [x] Feed delay measured once (2026-11-20, 10:21–10:28 am Eastern, 4 filings): 34 to 66 s from acceptance, with 30 s polling.
  - [x] Live feeds for Form 144, 13D/G and proxy filings: `oe follow` polls all four kinds by default (`--forms` to pick). Run live for 9 minutes: 50 Form 4s, 50 Form 144s, 26 13D/Gs and 6 proxy headers saved, 31 alerts; a new Form 4 and a new Form 144 arrived 86 s and 62 s after acceptance. 10-Q/10-K plan terminations still come from the next-day index.
  - [ ] Email
- [x] Dashboard (`oe dashboard`, ADR 0007): one offline HTML page from the downloaded folders, with headline numbers, events over time, insider buying vs selling, top companies, largest planned sales and trades, and a searchable event list with SEC receipt links. Checked in a browser on 143 real events and 500 trades.
- [x] Clean-up (2026-11-19): shared helpers for flags, numbers, dates, entities, tags, sidecars and CLI arguments (one copy each); `insider.cluster_buy@1` routine filter indexed (96,000 trades: 8.98 s to 17 ms); outputs on all real data unchanged
- [ ] **Gate 1:** public v0.1 release; alerts within 5 minutes of filing; fixture suite green
  - [x] Release prepared (2026-11-21): official licence, metadata, CI covering every check, changelog, `docs/release-checklist.md`
  - [ ] A full trading day of alert delays; CI green; tag and publish

## Phase 2: investor registry (Mar – May 2027)
## Phase 3: institutions (Jun – Aug 2027)
## Phase 4: evidence studies (Jul – Sep 2027)
## Phase 5: add-on modules and v1.0 launch (Sep – Nov 2027)

Details for later phases are in the master plan; they are copied here as each phase starts.
