# `insider.cluster_buy@1`

**Status:** active · **Code:** `crates/oe-events/src/cluster_buy.rs` · **Fixtures:** `fixtures/form4/synthetic/01–05`

## Question it answers

Did several insiders spend their own money buying the company's stock on the open market within a short period?

## Definition

An event fires for an issuer when, within a window of **10 calendar days** measured from the first qualifying buy, **at least 3 distinct insiders** make qualifying buys.

A transaction **qualifies** when all of these hold:

| Condition | Form 4 field | Why |
| --- | --- | --- |
| Non-derivative security | `nonDerivativeTable` | Option exercises are compensation, not conviction |
| Transaction code `P` | `transactionCoding/transactionCode` | Only open-market or private purchases |
| Not marked as disposed | `transactionAcquiredDisposedCode` ≠ `D` | Guards against coding errors |
| 10b5-1 box not ticked | `aff10b5One` ≠ true | Pre-scheduled trades say little about current views |
| Insider is not routine | history | See below |

**Routine insider:** traded the issuer (code P or S) in the same calendar month in each of the previous 3 years. Routine trades are mostly scheduled and carry little information (after Cohen, Malloy and Pomorski, 2012).

**Joint filings** count once, attributed to the first reporting owner; the other owners are kept as `co_owners`.

## Output

| Field | Meaning |
| --- | --- |
| `window_start`, `window_end` | Dates of the first and last qualifying buy |
| `insiders` | Distinct insiders, in order of first buy |
| `total_value` | Sum of shares × price where both are known |
| `trades_without_value` | Buys with no usable price (blank or footnote only) |
| `known_on` | The date the event became detectable: the SEC acceptance date of the third distinct insider's filing. Unknown if any acceptance date is unknown. |

## Parameters

| Parameter | v1 default | CLI flag |
| --- | --- | --- |
| `min_insiders` | 3 | `--min` |
| `window_days` | 10 | `--window` |
| `exclude_10b5_1` | true | none |
| `routine_lookback_years` | 3 | `--keep-routine` sets 0 |

Changing a default is a new version (`@2`), not an edit to this one.

## Known limitations

- The 10b5-1 checkbox only exists on filings from April 2023 onwards; older filings are treated as "not ticked".
- The routine filter needs at least 3 years of history for the issuer; with less, nobody is treated as routine.
- Windows are greedy: once an event is emitted, the next window starts after its last buy.
- No minimum value yet; a cluster of tiny buys still fires. Candidate for `@2` after the evidence study.

## Evidence

Not yet studied. The Phase 4 study will report frequency, outcomes against size- and sector-matched benchmarks, confidence intervals, and sample sizes, measured from `known_on`.
