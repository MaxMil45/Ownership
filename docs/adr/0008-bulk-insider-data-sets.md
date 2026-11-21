# ADR 0008: History from the SEC's quarterly Insider Transactions Data Sets

**Status:** accepted · **Date:** 2026-11-21

## Context

The ledger needs Forms 3/4/5 since 2015. Downloading each filing (about 2.2 million) at 8 requests per second was estimated at 75 hours. The SEC publishes the same filings as **Insider Transactions Data Sets**: one zip per quarter since 2006, tab-separated tables (SUBMISSION, REPORTINGOWNER, NONDERIV_TRANS, DERIV_TRANS, holdings, footnotes, signatures), "as filed", including amendments. 2026 Q3 is an 8.7 MB zip with 38,152 filings.

Checked on 2026-11-21 against the filings' own XML (200 random filings of 2026 Q3, read with this project's parser):

- 197 of 200 documents identical field by field once numbers are rounded to two decimals; the 3 others list the same owners of a joint filing in a different order.
- The filing date equals the US Eastern date of the SEC acceptance in all 199 comparable filings.
- Numbers are stored as `NUMBER(16,2)`: 43 of 199 filings had a price or share count with more decimals in the XML ($168.5672 becomes $168.57).
- One filing (LSI Industries, 0000892251-26-000141) has no `<transactionCoding>` at all. Our parser rejected it; it now accepts it with an empty code and it is a real fixture.
- The files contain owners' street addresses. They are never read.
- Some text fields are quoted CSV-style (`""A""`); they are unquoted.

## Decision

- A new crate, `oe-bulk`, reads a quarter's zip into the same `OwnershipDocument` the XML parser returns, so the ledger, rules and dashboard need nothing new. It has its own small ZIP reader and uses `miniz_oxide` (inflate) and `crc32fast` (CRC-32), which were already compiled into the project through `ureq`; no new crate enters the build.
- `oe bulk-insider` lists the quarters on the SEC's page (the folder changed in 2026, so the URL is read, not built), downloads each missing zip once into `data/bulk/` through the rate-limited client, and loads it.
- Migration `0004_link_index` (2026-11-21) replaces 0003's index on `(form_type, accepted_at)`, which could not serve linking's US Eastern date expression, with one on `(form_type, (accepted_at AT TIME ZONE 'America/New_York')::date)`: one lookup went from 353 ms to 0.23 ms on 2.4 million filings.
- Migration `0003_bulk` adds `filing.source` (`xml` or `bulk`). A bulk filing's `accepted_at` is **23:59:59 US Eastern on its filing date**: never earlier than the real acceptance, so "as known on" queries never see it too early (at most a day late). Its `source_url` is the zip URL plus `#accession`; its `raw_sha256` fingerprints the loaded fields.
- When the same accession arrives from both sources, the first one loaded is kept and the other is not reported as a changed source.
- A quarter loads in batches of 5,000 filings without linking, then one amendment linking pass. Linking now matches a candidate report by **any reporting owner in common** (bulk owner order is not the XML's), and accepts a report accepted **at the same time or earlier** (bulk filings of one day share one time).
- The restatement rule of ADR 0005 was widened on the bulk evidence: a row counts as the same row corrected when it keeps its table and **two of** code, date and share count. On 2026 Q3 the old rule held back 32 amendments that say in their remarks that they correct a transaction code (A to P, D to S); 8 were read and all said so. Amendments loaded on that quarter went from 217 to 255, held back from 151 to 113.

## Consequences

- The history load takes minutes, not days. Measured 2026-11-21, 2015 Q1 to 2026 Q3 into the dev database: 47 zips (8.7 to 15 MB each), 31 minutes in all, 21 to 60 seconds per quarter (slower as the tables grow). Result: 2,396,842 bulk filings, 4.94 million transaction rows, 66,525 joint filings; 50,175 amendments, of which 30,252 linked and loaded, 14,503 linked but held back, 1,020 ambiguous and 4,400 still waiting (mostly an original before 2015, or a stated date that matches no report).
- The data sets hold filing errors as filed: prices that are really totals ($525,600,000 a share), round placeholders (10,000,000 shares at $1,000), and whole filers that report billions for penny stocks. Dollar totals (the dashboard's history chart) leave out rows over $2 million a share or $2 billion a row, and company-days over $10 billion (`oe_core::MAX_PLAUSIBLE_*`): 1,082 open-market rows of 2015-2026, counted on the page. The rows stay in the ledger. A few real block trades over $2 billion are lost from the totals; trades reported once by each of several related funds are still counted once per filing.
- Bulk rows are rounded to two decimals and carry day-level time. Studies that need the exact time of day must use XML filings (the feed and daily downloads keep doing that for new filings).
- The latest quarter appears some weeks after it ends; the daily index and the live feed cover the gap.
- Amendments whose original is before the loaded range stay "waiting".
