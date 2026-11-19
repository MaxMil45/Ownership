# Field Guide: Form 4

Form 4 reports a change in an insider's holdings of their company's securities. It is the fastest ownership signal the SEC publishes, and the most misread.

## Who files and when

- **Who:** directors, officers (as defined in Rule 16a-1(f)) and owners of more than 10% of a class of registered equity: "Section 16 insiders".
- **When:** within **2 business days** of the transaction.
- **Related forms:** Form 3 is the initial holdings statement when someone becomes an insider. Form 5 is an annual catch-up for exempt or deferred transactions, due within 45 days of fiscal year end. An amendment carries the suffix `/A` (for example `4/A`).

## Where to get it

Every filing has an XML file (the `ownershipDocument`) in its EDGAR folder. Always parse the XML, never the HTML rendering. The XML does **not** contain the SEC acceptance time; get that from the EDGAR index or the submissions API and store it as `known_on`.

## Structure

```text
ownershipDocument
├── documentType            4, 4/A, 3, 5 ...
├── periodOfReport          date of the earliest transaction reported
├── aff10b5One              10b5-1 checkbox (filings since 2023)
├── issuer                  issuerCik, issuerName, issuerTradingSymbol
├── reportingOwner (1..n)   rptOwnerCik, rptOwnerName, relationship flags, officerTitle
├── nonDerivativeTable      common stock: transactions and holdings
├── derivativeTable         options, RSUs, warrants: transactions and holdings
└── footnotes               free text referenced by footnoteId
```

Each value sits inside a `<value>` element and may carry `<footnoteId id="F1"/>` siblings. A value can be missing entirely, with only a footnote explaining it.

## Transaction codes that matter most

| Code | Meaning | Signal value |
| --- | --- | --- |
| P | Open-market or private purchase | **High**: the insider spent their own money |
| S | Open-market or private sale | Low on its own: diversification, tax, plans |
| A | Grant or award from the company | None: compensation |
| M | Exercise or conversion of an exempt derivative | Low: usually paired with S or F |
| F | Shares withheld to pay exercise price or tax | None: mechanical |
| G | Gift | Low; occasionally interesting around news |
| X | Exercise of an in- or at-the-money derivative | Low |
| C | Conversion of a derivative security | Low |
| D | Disposition back to the issuer | Context-dependent |
| J | Other (explained in a footnote) | Read the footnote |

## Edge cases the parser must survive

- **Booleans** appear as `1`/`0` or `true`/`false`.
- **Numbers** may include thousands separators.
- **Weighted-average prices** with a footnote giving the price range.
- **Prices given only in a footnote**, with no `<value>`: treat as unknown, never zero.
- **Holdings rows** (`nonDerivativeHolding`) are positions, not transactions.
- **Indirect ownership** (`I`) through trusts, LLCs, family members: see `natureOfOwnership`.
- **Joint filings:** several reporting owners on one form, often a fund and its manager. Do not count them as separate insiders.
- **Option exercise then sale on the same day:** an M row plus an S row. Neither is a buy.
- **Amendments** restate earlier filings. Append them; never overwrite the original.
- **No ticker** (`NONE` or blank) for some issuers.

## Common misreadings

1. Treating every sale as bearish. Most sales are scheduled or for tax.
2. Counting option exercises as purchases.
3. Using the transaction date instead of the filing date when testing a strategy. The market learns on the filing date.
4. Adding up joint filers as separate people.

## Sources to check against

- SEC Form 4 General Instructions (transaction codes are in item 8).
- EDGAR Ownership XML Technical Specification.
- Cohen, Malloy and Pomorski (2012), "Decoding Inside Information", *Journal of Finance*: the routine/opportunistic distinction.
