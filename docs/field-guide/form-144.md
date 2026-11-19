# Field Guide: Form 144

Form 144 is the notice an insider files **before** selling restricted or control shares under SEC Rule 144. Form 4 reports a trade after it happens; Form 144 says one is about to. It is filed when the sell order is placed with the broker.

Everything below was checked against 274 real filings (four days: 2023-06-14, 2024-03-12, 2025-08-13, 2026-11-18), all of which the parser reads.

## Who files and when

- **Who:** an officer, director or large holder ("affiliate") selling restricted or control securities, and holders of restricted stock more generally. A notice is required when the sale is over 5,000 shares or $50,000 in a three-month period.
- **When:** at the time the sell order is placed. `approxSaleDate` is the same day the SEC accepted the notice in 262 of 274 filings, and earlier than the acceptance day in the other 8 (a notice can be filed after the sale date it names), so treat it as a planned date, not a guarantee.
- **Since April 2023** Form 144 is filed as XML. Earlier filings are paper or HTML and are not covered here.
- **Volume:** about 150 distinct filings on a busy day (154 on 2026-11-18), against about 760 Form 4s.

## Where to get it

The complete submission text file (`.../data/<CIK>/<accession>.txt`) holds the XML in its first `<XML>` block (`primary_doc.xml`). `oe daily-form144 <date>` downloads a day's worth; it finds them through the daily index (see `edgar-apis.md`). The submission header gives the acceptance time. Like Form 4, the XML itself has no acceptance time.

## Structure

```text
edgarSubmission
├── headerData/submissionType      144 or 144/A
└── formData
    ├── issuerInfo                 issuerCik, issuerName, nameOfPersonForWhoseAccountTheSecuritiesAreToBeSold,
    │                              relationshipsToIssuer/relationshipToIssuer (1..n)
    ├── securitiesInformation (1..n)  securitiesClassTitle, noOfUnitsSold, aggregateMarketValue,
    │                              noOfUnitsOutstanding, approxSaleDate, securitiesExchangeName,
    │                              brokerOrMarketmakerDetails/name
    ├── securitiesToBeSold (1..n)  how the shares were acquired: acquiredDate, natureOfAcquisitionTransaction,
    │                              nameOfPersonfromWhomAcquired, isGiftTransaction, amountOfSecuritiesAcquired
    ├── nothingToReportFlagOnSecuritiesSoldInPast3Months   Y / N
    ├── securitiesSoldInPast3Months (0..n)  sellerDetails/name, saleDate, amountOfSecuritiesSold, grossProceeds
    ├── remarks                    optional free text
    └── noticeSignature            noticeDate, planAdoptionDates/planAdoptionDate (0..n)
```

There is **no ticker symbol**. Dates are `MM/DD/YYYY`. A notice can list many `securitiesToBeSold` blocks (24 in one real filing) and more than one `securitiesInformation` block (two in 2 of 274).

## Traps found in real filings

- **Namespaces differ by filing software.** Most files use a default namespace with `ns2:` on address fields. Others prefix every element (`own:`, `com:`), put an XML comment after the root, or prefix the root itself (`ns2:`). Match on local names; `oe-form4`'s tokenizer already strips prefixes.
- **`relationshipToIssuer` is free text.** Seen: `Officer`, `Director`, `10% Stockholder`, but also `Chief Financial Officer`, `President &amp; CEO`, `Director/Non-Executive Director`, `Affiliate`, `Shareholder`, `See Remarks`, `May be deemed affiliate`, `Member of Section 13D group`, `Self`, `Former CEO`, `Former Officer`. Never match it against a short fixed list. Of 82 notices for $1,000,000 or more, 23 had a relationship the `insider.planned_sale@1` word test does not count.
- **Plan adoption dates** (`planAdoptionDate`) are how a notice says the sale is under a Rule 10b5-1 plan. 109 of the 274 notices (40%) listed one.
- **Numbers sometimes have thousands separators** and values can be missing, so a notice can lack a total.
- **A seller's home address can be in the file.** The `sellerDetails/address` block in the past-sales section is usually the issuer's address, but 45 of 274 files had a different street there, and at least one was a residence. The fixtures in `fixtures/form144/real/` contain only filings whose seller address matches the issuer's or is "c/o". Check before adding more (project rule: no private individuals).
- The value is the filer's own `aggregateMarketValue`, not an independent price.

## What this project does with it

`oe-form144` parses it, and `insider.planned_sale@1` (`events/specs/insider.planned_sale.v1.md`) turns large notices by insiders into events. `oe planned-sales data/form144` lists them.
