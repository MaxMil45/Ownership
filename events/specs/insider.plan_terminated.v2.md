# `insider.plan_terminated@2`

**Status:** active when the ledger is used (`--db`); `@1` runs otherwise · **Code:** `crates/oe-events/src/plan_terminated.rs` (`detect_plan_terminations_with_ledger`), ledger query `oe_store::adoption_history` · **Replaces:** nothing; `@1` stays as it was

## Question it answers

The same as `@1`, did a director or officer end a Rule 10b5-1 trading plan, plus: **when was the plan adopted, and how long did it last?** In particular, did it end before it could ever be used?

## Background

A director's or officer's Rule 10b5-1 plan has a cooling-off period: no trade under it until at least 90 days after adoption (Rule 10b5-1(c)(1)(ii)(B)(1); up to 120 days, depending on when the next report is filed). A plan ended within those 90 days was never used to sell. Such quick terminations are what the SEC tightened the rule against in 2023.

## Why a new version

`@1` reports the termination only. The ledger (ADR 0009) holds the earlier reports' adoptions, and many reports tag the adoption date of the plan they say was ended. Adding the plan's life changes what the event says, so this is a new version.

## Definition

The events of `@1`, each with a **plan life** when the adoption is known:

1. **The adoption date tagged on the same arrangement** in this report (`ecd:TrdArrAdoptionDate`), unless:
   - the arrangement is person-level (no plan member) and also flagged as adopted: the company tagged only the person, so the old and the new plan's facts are merged and the date may be the new plan's (501 of the ledger's 710 terminated arrangements with a tagged adoption date are like this);
   - the report's termination date was rejected (after the period): then its date tags are shifted. Cryoport's 10-Q of November 2026 tags the termination day, 11 August 2026, as the adoption date and the plan's scheduled end, March 2027, as the termination date; its text says the plan was adopted on 11 May 2026.
2. Otherwise, **the latest adoption by the same person at the same company** that an earlier report in the ledger disclosed with an adoption date (accepted on an earlier US Eastern date; the person's name compared on its letters only, ignoring case).

The plan life gives:

| Field | Meaning |
| --- | --- |
| `adopted_on` | The adoption date |
| `adoption_report` | Empty when tagged in this report; else the earlier report's accession (linked in the alert as a second receipt) |
| `days` | Adoption to the termination date, or to the period end when no termination date is tagged (`days_at_most`: an upper bound). Empty when the adoption is after the end, a tagging error |
| `within_cooling_off` | `days` is at most 90 (`COOLING_OFF_DAYS`) |

The alert headline ends "(ended before it could be used)" when `within_cooling_off`.

## Parameters

None. 90 days is the rule's shortest cooling-off period.

## Known limitations

- **Most adoptions were not in the ledger.** Until 2026-11-23 the 10-Q/10-K download kept only reports whose insider-trading page showed a termination, so an adoption was stored only when its report also had a termination; of the 1,886 terminations, 338 found their adoption in an earlier report. The download now keeps adoption reports too (ADR 0009; on 2026-11-05, 104 reports with adoptions only against 18 with terminations); the history before that needs `daily-periodic --recheck-none`.
- A person with several plans: the latest earlier adoption may be another plan than the one ended, which would make the plan look younger than it was.
- `days` to the period end is only an upper bound; 19 of the 102 within the cooling-off rest on it.
- Officers can have up to 120 days of cooling-off, and directors 90; the rule uses 90 for everyone, so "within the cooling-off" is the certain part only.
- The tags are read as filed, apart from the two guards above.

## How this was checked

- Unit tests (`crates/oe-events/src/plan_terminated.rs`): a tagged adoption gives an exact length or an upper bound; an adoption after the end gives none; the latest earlier adoption by the same person in the ledger, skipping undated ones, other people and the same day; a person-level replaced plan's date is not used, a plan-level one adopted and ended in one quarter is; Cryoport's shifted dates are not used.
- Alert test (`crates/oe-alerts/src/convert.rs`): the headline tag, the plan-life line and the earlier report as a second receipt.
- Database test: `adoption_history` returns every adopted arrangement with a person.
- Real run, 2026-11-23, `oe plan-terminations data/periodic --db` over the 1,530 saved reports: 1,886 events, as in `@1`. 522 have a known adoption: 184 tagged in the same report, 338 from an earlier report. 443 have an exact length, median 213 days. 102 ended within 90 days of adoption, 45 of them without a new plan in the same report. Examples: Figure Technology Solutions (adopted 11 June 2026, ended 26 August, 76 days), Monolithic Power Systems (77 days), Live Oak Bancshares (3 days). Two tagged adoptions were after the termination.

## Evidence

Not yet studied. The Phase 4 study can compare quick terminations with plans that ran their course, measured from `known_on`.
