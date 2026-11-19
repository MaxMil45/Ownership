# Fixtures

Fixtures are filings the test suite parses. They are the project's memory of every edge case it has met.

## Layout

| Folder | Contents |
| --- | --- |
| `form4/synthetic/` | Hand-built Form 4s for a made-up issuer ("Synthetic Widgets Inc", CIK 9999901). **Not real filings.** Each file's header comment says which edge case it covers. |
| `form4/real/` | Real Form 4s downloaded from EDGAR (add these next, see below). |

## Synthetic set: what each file tests

| File | Edge case |
| --- | --- |
| `01_director_a_buy.xml` | Plain open-market purchase; shares written with a thousands separator |
| `02_ceo_buy_with_footnotes.xml` | Footnote IDs inside values, CDATA footnote, `&amp;` in a name, `true`/`false` booleans, indirect ownership, a holding row that is not a transaction |
| `03_cfo_buy.xml` | Third distinct insider inside the 10-day window |
| `04_planned_sale_and_option_exercise.xml` | 10b5-1 checkbox set, option exercise (code M), sale (code S), derivative table |
| `05_late_buy_outside_window.xml` | Amendment (`4/A`), buy outside the window, price given only as a footnote |

Together, files 01–03 must produce exactly one `insider.cluster_buy@1` event.

## Adding real fixtures

1. Set your SEC contact details (PowerShell): `$env:OE_USER_AGENT = "Your Name your.email@example.com"`
2. Find a Form 4 on EDGAR and copy the URL of its `.xml` file (not the HTML rendering).
3. Download it: `cargo run -p oe-cli -- fetch <url> fixtures/form4/real/<accession>.xml`
4. Add a test, or a row to this table, saying what the filing exercises.

Aim for variety: joint filers, Form 5s, late filers, gifts (G), tax withholding (F), large option exercises, and amendments.
