# ADR 0009: Form 144, Schedule 13D/G, proxy contests and Item 408 in the ledger

**Status:** accepted · **Date:** 2026-11-21

## Context

Until now only Forms 3/4/5 were in PostgreSQL. Form 144 notices, Schedule 13D/13G filings, contested-proxy headers and 10-Q/10-K insider trading arrangements (Item 408) were read from the download folders each time a rule ran. That means no history beyond what is on disk, no queries across companies, and every rule re-reading every file.

## Decision

- Migration `0005_other_forms` adds append-only tables: `form144_notice` and `form144_security`; `schedule13_filing` and `schedule13_person`; `contest_filing` and `contest_filer`; `trading_arrangement`.
- Every such filing also gets its receipt in `filing` (exact acceptance time from the sidecar or the SGML header, fingerprint of the file as saved, source URL), like a Form 4. Known-at is `filing.accepted_at`.
- `filing.filer_cik` is the filer the document names: the Form 144 submitting filer (`headerData/filerInfo`, now read by the parser), the first 13D/G filer, the first challenger of a contest (the company itself when it filed alone), the registrant of a 10-Q/10-K. The issuer stands in only when a filing names no filer.
- Amendments (144/A, 13D/A, 13G/A) are their own filings: each is a complete statement as of its acceptance, so no linking is needed; "as known on" a date means the latest statement accepted by then.
- **Not stored:** street addresses (Form 144 and 13D/G can carry home addresses) and CUSIPs (a licensed identifier; ADR 0003). A database test checks that no table has such a column.
- `oe db-load <folder>` recognises each file by what it is (root element `ownershipDocument` or `edgarSubmission`, `.hdr.sgml`, `.htm`), so `oe db-load data` loads every folder. `oe follow --db` loads every kind after each poll. These forms are hundreds a day, not thousands, so plain inserts in one transaction per batch are enough (no `COPY`).

Checked 2026-11-21: the 745 downloaded files (274 Form 144s, 382 13D/Gs, 70 contest headers, 19 10-Qs/10-Ks with 48 arrangements) loaded with none skipped; a repeat load wrote nothing.

## How far back each form can go

Sampled on real days, 2026-11-21:

| Form | Structured data from | Evidence |
| --- | --- | --- |
| Form 144 | early 2023 (electronic filing required from April 2023) | 0 in the index on 2022-06-15, 25 on 2023-03-15, 120 to 180 a day from 2023-05 |
| Schedule 13D/13G | 2024-12-23 | 0 on 2024-12-20, 62 on 2024-12-23; before that they are `SC 13D`/`SC 13G` text documents (83 on 2024-11-20), which this parser does not read |
| Item 408 tags in 10-Q/10-K | late 2023, more complete each season | the tagged page in 7 of 15 reports on 2023-11-08, 10 of 15 on 2024-05-08, 13 of 15 on 2024-11-06 |
| Contest headers | any year (SGML headers exist throughout EDGAR) | |

## Consequences

- Rules still read the folders; moving them onto these tables is the next step, and makes history usable (for example, looking up earlier 13Gs for `activist.passive_to_active`).
- Older 13D/Gs would need a reader for the `SC 13D`/`SC 13G` text format.

## History download, 2026-11-21 to 2026-11-23

Run one command at a time (each `oe` process has its own rate limiter), with the daily index for every weekday:

| Step | Range | Time | Result |
| --- | --- | --- | --- |
| `daily-proxy --all` | 2015-01-02 to 2026-11-20 | 3 h 40 min | 10,108 contest headers |
| `daily-form144 --all --include-amendments` | 2023-01-03 to 2026-11-20 | about 7 h (25 to 35 s a day) | 130,904 notices |
| `daily-13dg --all --include-amendments` | 2024-12-23 to 2026-11-20 | about 2 h 30 min | 63,714 filings |
| `daily-periodic --all` | 2023-10-02 to 2026-11-20 | 9 h 26 min | 75,516 reports checked (about 135 a minute); 1,498 with a termination saved; 1,530 reports and 4,409 arrangements stored |
| retries of failed days | | about 1 h | all recovered except 3 contest headers, 6 Form 144s and about 43 13D/Gs that the SEC answers 404 at the path the index gives |
| `db-load data` | 226,541 files | 21 min | 205,511 new filings, 508,306 content rows; a repeat writes nothing |

Learned on the way:
- The SEC sometimes answers 503 for a daily index. A range used to stop at the first such error; the index download now waits 5 s, 30 s and 2 min before giving up, and a range reports a failed day and goes on, listing the failed days at the end.
- The 10-Q/10-K check made 6 reports at a time under the shared limiter (it was one at a time, which would have taken about two days).
- Downloaded files take about 1 GB for these forms (Form 144 2.6 KB and 13D/G 5.6 KB per file); the saved 10-Q/10-K reports with a termination are the largest part (about 2 MB each).
