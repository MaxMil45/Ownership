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
- It holds at least one year or 1,000 filings, whichever is more. Older filings are in extra files named in `filings.files`.
- `acceptanceDateTime` is when the SEC accepted the filing. This project stores its date as `known_on`, the earliest the public could know.
- `formerNames` gives free alias history for the investor registry.
- Form 4s about a company appear in the company's own history and in each insider's history.

## The trap: `primaryDocument` for Form 4s

For ownership forms, `primaryDocument` usually points at the **rendered** version, for example `xslF345X05/wk-form4_1234.xml`. Remove the `xsl…/` folder to get the raw XML. The raw URL is:

```text
https://www.sec.gov/Archives/edgar/data/{CIK}/{accession without dashes}/{primaryDocument without the xsl folder}
```

`FilingRef::raw_document_url` in `oe-edgar` does this.

## To check

- The time zone of `acceptanceDateTime`: it is written with a trailing `Z`; confirm against EDGAR's filing index pages before relying on times rather than dates.
