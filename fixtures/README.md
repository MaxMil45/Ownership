# Fixtures

Fixtures are filings the test suite parses. They are the project's memory of every edge case it has met.

## Layout

| Folder | Contents |
| --- | --- |
| `form4/synthetic/` | Hand-built Form 4s for a made-up issuer ("Synthetic Widgets Inc", CIK 9999901). **Not real filings.** Each file's header comment says which edge case it covers. |
| `form4/real/` | Real Forms 3, 4, 4/A, 5 and 5/A downloaded from EDGAR, each with a `.meta.txt` sidecar. `MANIFEST.tsv` lists every file with its document type, owner count, transaction row counts, codes and what it exercises. |
| `edgar/` | Small **synthetic** samples shaped like the SEC submissions API (recent list and an older history page), ticker list, daily form index (`form_index_synthetic.idx`), latest-filings Atom feed (`latest_feed_synthetic.atom`) and a complete submission text file (`submission_synthetic.txt`), for testing the parsers without the network. |

## Synthetic set: what each file tests

| File | Edge case |
| --- | --- |
| `01_director_a_buy.xml` | Plain open-market purchase; shares written with a thousands separator |
| `02_ceo_buy_with_footnotes.xml` | Footnote IDs inside values, CDATA footnote, `&amp;` in a name, `true`/`false` booleans, indirect ownership, a holding row that is not a transaction |
| `03_cfo_buy.xml` | Third distinct insider inside the 10-day window |
| `04_planned_sale_and_option_exercise.xml` | 10b5-1 checkbox set, option exercise (code M), sale (code S), derivative table |
| `05_late_buy_outside_window.xml` | Amendment (`4/A`), buy outside the window, price given only as a footnote |

Together, files 01–03 must produce exactly one `insider.cluster_buy@1` event.

## Real set

24 filings from 15 companies, all public SEC records, chosen on 2026-11-19 from 325 Form 4s downloaded from 22 companies with `oe recent-form4`. The Forms 3, 5 and 5/A and two of the 4/As were downloaded with `oe fetch` from the submissions API listing, because `recent-form4` only takes Form 4 and 4/A. The files are named `<ticker>_<accession>.xml`. What each one exercises is in `fixtures/form4/real/MANIFEST.tsv`; in short, the set covers codes P, S, A, D, F, G, M, C and J, joint filings (two owners), a Form 4 with no transaction rows, derivative tables, 10b5-1 box ticked, not ticked and absent, a 22-row filing, and Forms 3, 5, 4/A and 5/A.

In each sidecar, `accepted` is the SEC's UTC timestamp and `known_on` is its US Eastern date (the test checks this).

The manifest counts come from counting tags in the raw XML, not from the parser. `crates/oe-form4/tests/real_fixtures.rs` parses every file and fails if the parser disagrees, if a fixture is missing from the manifest, or if a sidecar has no `known_on`. All 24 parse today with no parser failures.

Not yet covered: a filing that makes the parser fail (none of the 325 downloaded did), a Form 4 with more than two owners (the only joint filings found had two), and a late filing whose transaction date is well before `known_on` (not searched for yet).

Owner addresses in these filings are company or business addresses. Check that before adding more (see the "no private individuals" rule in `CONTRIBUTING.md`).

## Adding real fixtures

1. Set your SEC contact details (PowerShell): `$env:OE_USER_AGENT = "Your Name your.email@example.com"`
2. Download a company's recent Form 4s: `cargo run -p oe-cli -- recent-form4 <TICKER> --limit 20 --include-amendments`
3. Look through `data/form4/<CIK>/` and copy the interesting ones (both the `.xml` and its `.meta.txt`) into `fixtures/form4/real/`, named `<ticker>_<accession>.xml`.
4. Add a row to `fixtures/form4/real/MANIFEST.tsv` (tab-separated; count the `<nonDerivativeTransaction>` and `<derivativeTransaction>` tags yourself) saying what the filing exercises, then run `cargo test -p oe-form4 --test real_fixtures`.

If the download prints "the parser failed", that filing is the most valuable fixture you can add.

Aim for variety: joint filers, Form 5s, late filers, gifts (G), tax withholding (F), large option exercises, and amendments.
