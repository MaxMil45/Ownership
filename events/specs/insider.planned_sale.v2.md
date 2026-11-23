# `insider.planned_sale@2`

**Status:** active when the ledger is used (`--db`); `@1` runs otherwise · **Code:** `crates/oe-events/src/planned_sale.rs` (`detect_planned_sales_with_ledger`), ledger query `oe_store::form144_history` · **Replaces:** nothing; `@1` stays as it was

## Question it answers

The same as `@1`, did an insider file notice of a large planned sale, plus: **is this seller's selling new or routine?** A founder who files every month under a trading plan and a director filing for the first time in years are different news.

## Why a new version

With the Form 144 history in the database (ADR 0009) a seller's earlier notices can be counted. Measured on the ledger: of the 29,568 notices of at least $1,000,000 accepted since April 2024 (the first month with a full year of history before it), only 26% were the seller's first notice for that company in the year before, and 13% came from sellers with notices in 6 or more of those 12 months. Most large planned sales are repeat sales.

The history also exposed amounts no company could have: 34 notices claim over $2 billion, and some imply a company worth thousands of trillions of dollars. `@1` takes every amount as filed. Both changes alter what an event says or whether it fires, so this is a new version.

## Definition

An event fires for each notice that `@1` fires for (an original Form 144 from an insider, at least `min_value`), **unless its amount cannot be believed as filed**: a securities block whose value divided by its units is over $2,000,000 a unit (`oe_core::MAX_PLAUSIBLE_PRICE`, the same limit as for Form 4 totals), or over $10,000 a unit (`SUSPECT_PRICE`) **and** times its units outstanding over $10 trillion (`MAX_PLAUSIBLE_COMPANY_VALUE`).

Why both conditions: on the 34 notices over $2 billion, every real one (Jeffrey P. Bezos selling Amazon, Deutsche Telekom selling T-Mobile, Walton trusts selling Walmart) implies a company worth at most $2.9 trillion at a price under $500, and every error has a price of at least $122,000 or a company value of at least $15 trillion. A huge company value at an ordinary price means the outstanding count is the typo, not the sale. NVIDIA director Tench Coxe's real $142.8 million notice of June 2025 states 244 billion shares outstanding for 24.4 billion, so the company value alone is not enough to reject it.

Each event carries the seller's **history**:

- `notices_past_year`: the seller's original notices (not 144/A) for the same company accepted on an earlier US Eastern date within the 365 days before;
- `months_past_year`: the distinct calendar months among them;
- `previous_notice`: the latest of them (accession, date), linked in the alert as a second receipt;
- `routine`: notices in at least **6** of those months (`ROUTINE_MIN_MONTHS`).

A seller is the company plus the seller's name with only its letters and digits, ignoring case ("Jeffrey P. Bezos" = "JEFFREY P BEZOS"). Measured: within one company, every such name maps to a single filer CIK in the ledger. The batch's own earlier notices count too. A notice without an acceptance date gets no history.

Routine sellers are **flagged, not filtered**, like plan sales in `@1`: the alert headline ends in "(routine seller)" or "(first in a year)", and the consumer decides.

## Output

The fields of `@1`, plus `history` as above.

## Parameters

| Parameter | Default |
| --- | --- |
| `min_value` | 1,000,000 (US dollars), `--min-value` |
| Look-back | 365 days (`HISTORY_DAYS`) |
| Routine | 6 months with notices (`ROUTINE_MIN_MONTHS`) |

## Known limitations

- The ledger's Form 144 history starts in 2023 (XML filing began then), so notices before April 2024 have less than a year of history and look like firsts more often.
- Name matching misses a seller who changes how the name is written ("Bezos Jeffrey P" and "Jeffrey P. Bezos" are different keys) and joins two namesakes at one company.
- A rejected amount is not corrected. The notice is left out, even if the real amount would have qualified. In the ledger: 12 notices from 2023 to 2026, all with prices of $122,000 to $48.6 million a share at companies trading in the tens or hundreds of dollars (Gartner twice, Eagle Materials three times, Insight Enterprises twice, CACI, Meritage Homes, Synchrony, Tenable, CSW Industrials).
- An error at an ordinary price still passes: an NVIDIA notice of May 2023 stated $3,790 a share ($379 million), ten times the real price, and was refiled correctly the same day; both fire.
- Everything in `@1`'s limitations still applies (the insider test, no ticker, the filer's own price).

## How this was checked

- Unit tests (`crates/oe-events/src/planned_sale.rs`): the count and months in the year before, the 365-day edge, the same day not counting, other sellers ignored, names matched without punctuation; six months is routine and five is not; the batch counted once, amendments never; the Tenable, Gartner, Amazon, Berkshire and NVIDIA shapes judged as above.
- Alert test (`crates/oe-alerts/src/convert.rs`): "(first in a year)" and "(routine seller)" headlines, the history line and the previous notice as a second receipt.
- Database test: `form144_history` returns only originals, from the date asked.
- Real run, 2026-11-23, `oe planned-sales data/form144 --db` over all 130,904 downloaded notices and the ledger's 129,408 originals: 34,284 events against 34,296 for `@1` (the 12 rejected amounts). From April 2024: 25,800 events, 6,557 the seller's first in a year (25.4%), 3,619 routine (14.0%). 84% of the routine sales were under a Rule 10b5-1 plan, against 50% of all events. The largest routine ones in October 2026 were George Kurtz (CrowdStrike), Martine Rothblatt (United Therapeutics) and Frank Slootman (Snowflake), each filing monthly under a plan; the largest first-in-a-year sale without a plan was Nikesh Arora's $201 million at Palo Alto Networks.

## Evidence

Not yet studied. The Phase 4 study can now compare first-in-a-year and routine sales, measured from `known_on`.
