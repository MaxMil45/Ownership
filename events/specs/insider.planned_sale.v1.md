# `insider.planned_sale@1`

**Status:** active · **Code:** `crates/oe-events/src/planned_sale.rs` · **Parser:** `crates/oe-form144` · **Fixtures:** `fixtures/form144/real/`

## Question it answers

Did a company insider file notice that they are about to sell a large block of the company's stock?

## Background

Under SEC Rule 144, an officer, director or large holder who plans to sell restricted or control shares files **Form 144** when they place the sell order, before the sale happens. It is a forward-looking filing: it says what is about to be sold and roughly when. (Form 4 reports the trade afterwards.) Since April 2023 Form 144 is filed as XML; older filings are on paper or HTML and are out of scope.

## Definition

An event fires for each **Form 144 notice** (submission type `144`) when both hold:

| Condition | Form 144 field | Why |
| --- | --- | --- |
| The seller is an insider | at least one `relationshipsToIssuer/relationshipToIssuer` contains, ignoring case, `officer`, `director`, `president`, `chairman`, `10%`, or the whole word `ceo`, `cfo` or `coo`, and does not start with `former` | The project covers insiders in their public roles. The field is free text (real values include "Chief Financial Officer", "President & CEO", "Director/Non-Executive Director"), so a word test is used. "Affiliate", "Shareholder", "See Remarks", "May be deemed affiliate" and "Former ..." do not qualify. |
| The sale is large | sum of `securitiesInformation/aggregateMarketValue` ≥ **$1,000,000** | Most notices are small; the threshold keeps the rest out. It is a starting point, not an evidence-based value. |

Amendments (`144/A`) never fire an event in v1: an amendment corrects an earlier notice and may not say which one, so counting both would double-count.

## Output

| Field | Meaning |
| --- | --- |
| `issuer`, `issuer_name` | The company whose stock is to be sold |
| `seller`, `relationships` | Name given on the form and every relationship ticked |
| `units`, `aggregate_market_value` | Sum over the notice's securities blocks (where present) |
| `percent_of_outstanding` | `units` ÷ `noOfUnitsOutstanding`, as a percentage, when both are present and the class is a single one; otherwise empty |
| `approx_sale_date` | `approxSaleDate` of the first securities block: the planned date |
| `under_plan` | `true` if the notice lists at least one `planAdoptionDate`, i.e. the sale is under a Rule 10b5-1 plan. **Flagged, not filtered**: a plan sale says little about the seller's current view, but consumers decide. |
| `plan_adopted_on` | The earliest plan adoption date, when the notice gives one |
| `sold_past_3_months` | Units the same notice lists as already sold in the past three months |
| `known_on` | US Eastern date the SEC accepted the notice: when the public could first know |

## Parameters

| Parameter | v1 default | CLI flag |
| --- | --- | --- |
| `min_value` | 1,000,000 (US dollars) | `--min-value` |

Changing a default, or the insider test, is a new version (`@2`), not an edit to this one.

## Known limitations

- **The insider test misses people.** In a sample of 274 real notices (four days from 2023 to 2026), 82 were for at least $1,000,000, and 23 of those (28%) were excluded by the insider test: "Shareholder" (6), "See Remarks" (5), "Affiliate" (5), and one each of "Self", "May be deemed affiliate", "Member of Section 13D group" and four "Former ..." values. Some are probably insiders who described themselves loosely; counting them would admit unknown holders. Revisit in @2 with evidence.
- **No ticker.** Form 144 has no trading symbol; events carry the issuer's CIK and name.
- The value is the filer's own `aggregateMarketValue` (a price the filer chose, as of the notice), not an independent price.
- `percent_of_outstanding` uses the first block's `noOfUnitsOutstanding`. A notice covering several share classes would be misleading, so it is left empty when blocks name different classes.
- A seller may file several notices for one planned selling programme; each fires separately.
- The notice says a sale is planned, not that it happened. Matching to the later Form 4 is a separate question (not in v1).
- Filings before the XML era are not covered.

## Evidence

Not yet studied. The Phase 4 study will report frequency, outcomes against size- and sector-matched benchmarks, confidence intervals and sample sizes, measured from `known_on`, and will test the `$1,000,000` threshold and the plan/no-plan split.
