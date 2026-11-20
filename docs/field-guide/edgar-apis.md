# Field Guide: EDGAR's free APIs

The SEC publishes JSON APIs at `data.sec.gov`. They need no account and no API key, only a `User-Agent` header with your name and email, and fewer than 10 requests per second. Third-party services (sec-api.io and others) resell this same data; this project goes to the source.

## The ones this project uses

| API | URL | Used for |
| --- | --- | --- |
| Ticker list | `https://www.sec.gov/files/company_tickers.json` | Ticker → CIK lookup |
| Submissions | `https://data.sec.gov/submissions/CIK##########.json` | A filer's filing history: form, accession, dates, primary document, former names |
| Company facts | `https://data.sec.gov/api/xbrl/companyfacts/CIK##########.json` | Shares outstanding (for % ownership); later Item 408 |
| Frames | `https://data.sec.gov/api/xbrl/frames/{taxonomy}/{tag}/{unit}/{period}.json` | One value per company for a period |
| Bulk submissions | `https://www.sec.gov/Archives/edgar/daily-index/bulkdata/submissions.zip` | Backfill of every filer, rebuilt nightly around 3 a.m. ET |

`##########` is the CIK padded to 10 digits with leading zeros.

## Submissions API: what matters

- `filings.recent` is a table stored column by column: `accessionNumber[i]`, `form[i]`, `filingDate[i]`, `acceptanceDateTime[i]`, `primaryDocument[i]` all describe filing `i`. Newest first.
- It holds at least one year or 1,000 filings, whichever is more (JPMorgan's held 26,419 in November 2026). Older filings are in extra pages named in `filings.files`, each with `filingFrom` and `filingTo` dates. A page is `https://data.sec.gov/submissions/<name>` and holds the same columns as `filings.recent`, but at the top level of the JSON, newest first. `oe-edgar` reads them with `parse_history_page`, and `oe recent-form4` follows them (newest page first) when the recent list has fewer Form 4s than `--limit`.
- `acceptanceDateTime` is when the SEC accepted the filing, **in UTC** (see "The time zone trap" below). This project stores its US Eastern date as `known_on`, the earliest the public could know.
- `formerNames` gives free alias history for the investor registry.
- Form 4s about a company appear in the company's own history and in each insider's history.

## Finding filings across all companies

Three more sources, all measured on real data (November 2026):

| Source | URL | What it is |
| --- | --- | --- |
| Daily form index | `https://www.sec.gov/Archives/edgar/daily-index/YYYY/QTRn/form.YYYYMMDD.idx` | Every filing of one past day, fixed-width text, about 830 KB. 2026-11-18 held 4,269 rows and 762 distinct Form 4 / 4/A filings. A weekend or holiday gives 404. Not available for the current day. |
| Latest filings feed | `https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=4&owner=only&count=100&output=atom` | Atom feed of the newest 100 entries (about 50 filings). Use `owner=only`: with `owner=include` the `type=4` filter is ignored and most entries are prospectuses. It timed out once in testing, so a follower must treat a failed poll as normal. Declared encoding is ISO-8859-1. **Other forms** (checked 2026-11-20): use `owner=include`; `type` is a **prefix** match (`type=144` gives 144 and 144/A, `type=SCHEDULE%2013` gives 13D, 13D/A, 13G and 13G/A, `type=DEF` every DEF form); the pre-2025 name `SC 13D` gives nothing. Roles in the titles are `Reporting`/`Subject` (144) and `Filed by`/`Subject` (13D/G, proxy). 100 entries reached back about a day for Form 144 and two days for 13D/G. An entry's `<updated>` equals the SEC acceptance time. |
| Complete submission | `https://www.sec.gov/Archives/edgar/data/<any filer CIK>/<accession>.txt` | SGML header plus the ownership XML, one request. About 20 KB for a Form 4. |

Things to know:

- **A Form 4 is listed once per filer** (the issuer and each reporting owner), all with the same accession number. Count accession numbers, not rows. Any of the CIKs works in the `.txt` path.
- The index columns are: form type (may contain spaces such as `SC 13D`), company name, CIK, date filed (`YYYYMMDD`), file path. **The width of the form-type column changes over time and the header does not say:** 12 characters in the files checked from 2015-03-12 to 2024-03-12, 17 in 2025-08-13 and 2026-11-18. A parser that assumes one width misreads the other (it read 2023 Form 144 rows as form `144         Zoom`, found no filings, and reported 1 to 3 unreadable rows). `oe-edgar` finds the company column from the data: the position where almost every row starts a word. Checked on eight dates from 2015 to 2025: no unreadable rows, and Form 4 filing counts of about half the row count, as expected.
- **The submission header's `<ACCEPTANCE-DATETIME>` is US Eastern**, unlike the submissions API's UTC. Header `20261005184245` is API `2026-10-05T22:42:45Z`. Checked to the second on filings either side of the November 2025 clock change (EDT and EST). Its date part is `known_on` directly.
- The feed's `<updated>` carries an explicit offset (`-05:00`).
- Some Form 4s have no transaction rows, only holdings (for example directors reporting RSU holdings). That is not a parse failure.

`oe daily-*` and `oe follow` use these (`follow` polls Form 4, Form 144, `SCHEDULE 13` and the six contest-form prefixes); see `crates/oe-edgar/src/{daily_index,feed,submission}.rs`.

## Who filed about whom: the SGML header's parties

The `.hdr.sgml` header names the parties: `<SUBJECT-COMPANY>` (the company a filing is about, for proxy materials, 13D/G and tender offers), one `<FILED-BY>` block per filer, and `<FILER>` for a company's own reports. Each block has `<CONFORMED-NAME>` and `<CIK>`. `FilingHeader` exposes them as `subject`, `filed_by` and `filers`. See `proxy-contests.md`.

## Schedule 13D / 13G in the daily index

The form types are `SCHEDULE 13D`, `SCHEDULE 13D/A`, `SCHEDULE 13G` and `SCHEDULE 13G/A` (not `SC 13D`). They need the 17-character form column (`SCHEDULE 13D/A` is 14). Rows are per filer, so one filing can appear several times. See `schedule-13.md`.

## Periodic reports (10-Q, 10-K): what is different

- **Filing date is a day late after 5:30 pm Eastern** (the cutoff for Form 4 is 10 pm). Palantir's 10-Q: accepted 2026-11-02 18:00:39 Eastern, `FILING-DATE` 2026-11-03. The `.hdr.sgml` file (`.../<CIK>/<accession without dashes>/<accession>.hdr.sgml`, under 1 KB, same Eastern time as the full submission header) gives the acceptance time. The path without the accession folder gives 403.
- `.../<accession without dashes>/index.json` lists the files of a filing; its `type` field is an icon name, not a form type, and `size` is empty for some.
- `FilingSummary.xml` in the same folder (30 to 60 KB) names the main document (`<File doctype=...>`) and lists the rendered report pages `R1.htm`, `R2.htm`... with their `ShortName`; the `Insider Trading Arrangements` page is the small one that shows Item 408.
- The XBRL frames API (`/api/xbrl/frames/ecd/<tag>/<unit>/<period>.json`) does **not** work for discovery here: `ecd` facts carry dimensions, and the frame for `TrdArrSecuritiesAggAvailAmt` returned only 19 companies.
- See `item-408.md`.

## The trap: `primaryDocument` for Form 4s

For ownership forms, `primaryDocument` usually points at the **rendered** version, for example `xslF345X05/wk-form4_1234.xml`. Remove the `xsl…/` folder to get the raw XML. The raw URL is:

```text
https://www.sec.gov/Archives/edgar/data/{CIK}/{accession without dashes}/{primaryDocument without the xsl folder}
```

`FilingRef::raw_document_url` in `oe-edgar` does this.

## The time zone trap: `acceptanceDateTime` is UTC

The trailing `Z` is honest. Measured on 325 real Form 4s (November 2026): most were accepted between 20:00 and 23:59 UTC, which is the afternoon in New York, and 58 of them (18%) were accepted after midnight UTC. For all of those, EDGAR's own `filingDate` is the **previous** day, because filing dates and deadlines are in US Eastern time.

So the first ten characters of `acceptanceDateTime` are a day late for anything accepted after about 8 pm in New York (7 pm in winter). Example: accepted `2026-01-01T02:09:16Z` is 9:09 pm on 31 December in New York, `filingDate` 2025-12-31. That is why `known_on` is the Eastern date, computed by `oe_core::us_eastern_date` (daylight saving rules in code, no time-zone database). Checked against the Windows time-zone database and `filingDate` for 135 JPMorgan filings: no differences.

The sidecar keeps the raw UTC value as `accepted=` and `oe` re-derives `known_on` from it when it reads a sidecar, so a sidecar written before this fix is corrected on read.

## To check

- Filings accepted after 10 pm Eastern get the next business day as `filingDate`, but the public could see them at acceptance. `known_on` follows acceptance, not `filingDate`. No such case turned up in the data yet.
- Whether history pages for the busiest filers hold the same columns as the first page (only page 001 was read, for JPMorgan and Tesla).
