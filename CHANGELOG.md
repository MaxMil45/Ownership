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

### Ledger
- Point-in-time PostgreSQL ledger: every fact has the date it was true and the time the public could know it; nothing is updated or deleted (ADR 0002, 0004).
- Amendments are linked to the report they amend by evidence, and only restatements replace rows (ADR 0005, revised in ADR 0008). Joint filings keep every owner.
- History since 2015 from the quarterly data sets: 2.4 million filings in 31 minutes, checked against the filings' own XML (ADR 0008).

### Alerts and dashboard
- `oe alerts` and `oe follow --alert`: each event once per destination, with links to its SEC filings, to Discord or any JSON webhook, filtered by a watchlist (ADR 0006). Measured: alerts about 1 to 1.5 minutes after SEC acceptance.
- `oe dashboard`: one offline HTML page with charts, filters, search and every event (ADR 0007); `--db` adds insider buying and selling over the whole ledger.

### Known limits
- No email delivery yet; not yet tried with a real Discord channel.
- Alert delay measured on a few filings only, not over a full trading day.
- 10-Q/10-K plan terminations come from the next day's index, not live.
- Bulk-data numbers have two decimals and no time of day; dollar totals leave out implausible amounts as filed (ADR 0008).
- The licence for published data is not decided yet (ADR 0003); this release publishes code only.
