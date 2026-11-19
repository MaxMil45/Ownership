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
- [ ] Follow `filings.files` for history older than the recent list
- [ ] EDGAR daily index and live feed follower across all companies
- [ ] Bulk backfill of Forms 3/4/5 since 2015 into PostgreSQL
- [ ] Form 144 parser and `insider.planned_sale@1`
- [ ] Item 408 extraction from 10-Q/10-K Inline XBRL and `insider.plan_terminated@1`
- [ ] Schedule 13D/13G parser; `activist.new_13d@1`, `activist.passive_to_active@1`
- [ ] Proxy contest filings; `activist.proxy_contest@1`
- [ ] Alerts: email, webhook, Discord; watchlists by ticker and insider
- [ ] **Gate 1:** public v0.1 release; alerts within 5 minutes of filing; fixture suite green

## Phase 2: investor registry (Mar – May 2027)
## Phase 3: institutions (Jun – Aug 2027)
## Phase 4: evidence studies (Jul – Sep 2027)
## Phase 5: add-on modules and v1.0 launch (Sep – Nov 2027)

Details for later phases are in the master plan; they are copied here as each phase starts.
