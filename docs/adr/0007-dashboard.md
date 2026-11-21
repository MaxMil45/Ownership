# ADR 0007: A static, self-contained HTML dashboard

**Status:** accepted · **Date:** 2026-11-20

## Context

The command-line output is plain text: hard to see what is happening across hundreds of filings, or to research one company. A visual view is needed without adding a web server, a JavaScript build, or dependencies to the Rust workspace (ADR 0001).

## Decision

- `oe dashboard` writes **one HTML file** (default `data/report/dashboard.html`). The data is embedded as JSON in a `<script type="application/json">` element; the charts are drawn by about 400 lines of plain JavaScript and SVG in the same file. No libraries, no network requests, nothing sent anywhere; it opens from disk in any browser.
- The page shows the same events as `oe alerts` (one per alert key, every rule, every downloaded folder) plus every open-market buy (code P) and sale (code S) from the Form 4 folder. Amendment rows are left out so a trade is not counted twice.
- Filters (time range, event types, search) sit in one row and apply to every number, chart and list. Clicking a company in "Companies with the most events" searches for it.
- Charts follow one palette (six fixed rule colors, checked with a color-vision validator in light and dark mode), thin marks, a legend for every multi-series chart, a hover and keyboard tooltip, and a table view under each chart.
- Data from filings is inserted with `textContent` only; links are made only for `https://www.sec.gov/` receipt URLs; `</` in the data is escaped so it cannot end the script element.
- `Alert` gained two numeric fields for this, `amount_usd` and `percent`, which the webhook JSON now also carries (added at the end; nothing renamed).

- **Update 2026-11-21:** `oe dashboard --db` adds a chart of open-market buying and selling over the whole PostgreSQL ledger (2,963 days since 2015 after the history load, ADR 0008), summed per day by the database so the page stays small (466 KB). It follows the time range, not the search box. Rows with implausible amounts are left out of all dollar totals, on this chart and in the downloaded-files chart, with the count shown (`oe_core::plausible_amounts`, ADR 0008). The same option gives the cluster-buy routine filter the ledger's history.

## Consequences

- To refresh, run the command again (for example after `oe follow` or a `daily-*` download). The page does not update itself.
- The file grows with the data: about 315 KB for 143 events and 500 trades. With the history since 2015 loaded it will need aggregation (or the database) instead of embedding every row.
- Trade dollar values are shares × price as filed; filings without a price are left out of the buy and sale totals.
