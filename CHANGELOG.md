# Changelog

All notable changes. Dates are when the change was made; versions follow [Semantic Versioning](https://semver.org/) (before 1.0, minor versions may break things).

## 0.1.0 (unreleased)

The first public version: insider and activist events from SEC filings, each with its receipts.

### Reading filings
- Forms 3/4/5 XML parser with no dependencies, checked on 25 real fixtures (joint filers, amendments, Forms 3 and 5, a filing with no transaction code).
- Form 144, Schedule 13D/13G, Item 408 (10-Q/10-K Inline XBRL) readers, and contested-proxy SGML headers, each with real fixtures.
- EDGAR client with the SEC's fair-access rules built in: a named User-Agent and at most 8 requests per second.
- Downloads: one company's history (`recent-form4`), a whole day (`daily-*`), the live feeds for Form 4, Form 144, 13D/G and contest filings (`follow`), and the SEC's quarterly Insider Transactions Data Sets since 2006 (`bulk-insider`).

### Events (versioned rules, specs in `events/specs/`)
- `insider.cluster_buy@1`, `insider.planned_sale@1`, `insider.plan_terminated@1`, `activist.new_13d@1`, `activist.passive_to_active@1`, `activist.proxy_contest@1`.
- `activist.passive_to_active@2` (with `--db`): also fires when the ledger holds the holder's earlier 13G for the company, even if the filer left the box unticked; the alert links that 13G as a second receipt.
- `activist.proxy_contest@2` (with `--db`): one alert per contest instead of per batch, saying when the contest started and whether it is new.
- `insider.planned_sale@2` (with `--db`): says whether a planned sale is the seller's first Form 144 for the company in a year or part of a routine, links the previous notice, and leaves out notices whose amounts are impossible as filed.
- `insider.plan_terminated@2` (with `--db`): says when the ended plan was adopted and how long it lasted, and flags plans ended within the 90-day cooling-off, before any sale under them was possible.

### Ledger
- Point-in-time PostgreSQL ledger: every fact has the date it was true and the time the public could know it; nothing is updated or deleted (ADR 0002, 0004).
- Amendments are linked to the report they amend by evidence, and only restatements replace rows (ADR 0005, revised in ADR 0008). Joint filings keep every owner.
- Form 144, Schedule 13D/G, contested-proxy and 10-Q/10-K insider-plan filings are stored too, without street addresses or CUSIPs (ADR 0009), with their history: Form 144 since 2023, 13D/G since December 2024, contests since 2015, insider plans since late 2023.
- Day ranges (`--to`) retry the SEC's temporary errors and carry on past a failed day, listing it at the end.
- `daily-periodic` also keeps 10-Q/10-Ks that disclose only plan adoptions, and saves an excerpt (a few KB) instead of the 2 MB report, with the full report's fingerprint in the ledger; `oe periodic-excerpt` shrinks older folders; `--recheck-none` rechecks reports checked before adoptions were kept. The GitHub workflow `recheck-periodic.yml` runs that recheck on GitHub's machines; `scripts/import-periodic-recheck.ps1` loads its result.
- History since 2015 from the quarterly data sets: 2.4 million filings in 31 minutes, checked against the filings' own XML (ADR 0008).

### Alerts and dashboard
- `oe alerts` and `oe follow --alert`: each event once per destination, with links to its SEC filings, to Discord or any JSON webhook, filtered by a watchlist (ADR 0006). Measured: alerts about 1 to 1.5 minutes after SEC acceptance.
- `oe dashboard`: one offline HTML page with charts, filters, search and every event (ADR 0007); `--db` adds insider buying and selling over the whole ledger.
- `oe alert-delays` reports how long after SEC acceptance filings were saved and alerts delivered (every delivery is logged in `data/alerts/delivered.tsv`); `scripts/measure-trading-day.ps1` measures a whole trading day. `oe follow --alert` reads only the last 14 days of filings in each pass, and `--until` stops it at a time.

### Public officials (first step, ADR 0010)
- Crate `oe-officials`: reads the House Clerk's yearly disclosure index. `oe house-ptr` downloads House members' periodic transaction reports (PDFs, not yet read), two requests a second, only after the user acknowledges the legal limits on their use (`OE_OFFICIALS_ACK=1`). The GitHub workflow `officials-house.yml` does the same and encrypts its result; `scripts/import-officials-house.ps1` opens it.
- Members of Congress (names, terms, committees, from the public-domain congress-legislators data set) and every House report in the ledger (migration `0006_officials`), each report linked to its member or held back with the reason: `oe officials-load`, `oe official <name>`.

### Name lookup (ADR 0010)
- Forms 3/4/5 now keep each reporting owner's name, director/officer/10%-owner flags and officer title, and the issuer's name and ticker as filed (migration `0007_people`); `scripts/backfill-people.ps1` fills them for the history already loaded.
- `oe people-index` builds a name index from the ledger (insiders, 13D/G holders, contested-proxy filers, members of Congress); `oe lookup <name>` lists the candidates, never merged, each with roles, latest open-market trades, holdings, contest filings or House reports and their links.
- `oe serve`: the ledger as a JSON API on 127.0.0.1 only (`/v1/health`, `/v1/lookup`, `/v1/status`, `/v1/flow`), with a token, Host and Origin checks, and no logging of looked-up names; no new dependency.
- One command: `scripts/start.ps1` starts the database, builds `oe` and the web app when needed, and opens the app signed in. `oe serve` hosts the built app (`--app`, `--open`), signing the browser in with a per-run login link and an `HttpOnly`, `SameSite=Strict` cookie; pages get a strict Content-Security-Policy.
- Web app: a page per company (insiders, roles, trades, buying and selling, events; `GET /v1/company`, `/v1/companies`), names and companies linked to each other, a 1-year / 5-year / all range on the charts, and connection status in the header.
- Database: tuned server settings, `oe db-maintain` (run by `update.ps1`), migration `0009_lookup_indexes`, a faster joint-filing count (`docker/bench.sql` and ADR 0004 have the numbers).
- Security: the web app's test runner updated to Vitest 4.1.12, fixing three advisories in Vitest 3 and tinypool (one moderate, two critical; development only). CI now audits the Rust crates and the npm packages.
- `web/`: a local web app (TypeScript, React, Vite) over `oe serve`: overview charts, events with filters and receipts, name lookup. CI builds and tests it.
- `scripts/update.ps1`: one command that catches up from the last finished day to yesterday (downloads, loads only new files with `db-load --changed-within <days>`, rebuilds events and the name index); remembers its place in `data/update/last-day.txt`.
- `oe events-build` finds cluster buys over the whole ledger since 2015 (13,395 events, 126 since October), not only the Form 4 files on disk.
- `oe events-build` stores every rule's events in the database (migration `0008_event_feed`, rebuilt whole each time); `oe serve` lists them at `/v1/events` with date, rule, ticker, company, amount and text filters, and per-day counts at `/v1/events/counts`.

### Known limits
- No email delivery yet; not yet tried with a real Discord channel.
- Alert delay measured on a few filings only, not over a full trading day.
- 10-Q/10-K plan terminations come from the next day's index, not live.
- Bulk-data numbers have two decimals and no time of day; dollar totals leave out implausible amounts as filed (ADR 0008).
- The licence for published data is not decided yet (ADR 0003); this release publishes code only.
