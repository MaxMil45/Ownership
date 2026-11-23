# `activist.proxy_contest@2`

**Status:** active when the ledger is used (`--db`); `@1` runs otherwise · **Code:** `crates/oe-events/src/proxy_contest.rs` (`detect_proxy_contests_with_ledger`), ledger query `oe_store::contest_history` · **Replaces:** nothing; `@1` stays as it was

## Question it answers

The same as `@1`: is someone other than the board soliciting shareholders' votes at a company? `@2` answers it once per **contest**, not once per batch of filings, and says whether the contest is new.

## Why a new version

`@1` groups a batch's filings by company. Run daily, that has three faults, all measured by replaying the ledger's contest filings since 2015 (10,108 filings, 1,135 contests) one day at a time with `@1`'s alert key (company plus the day's set of challengers):

- **Repeats:** 357 contests alerted more than once (up to 4 times), because a day with only the company's filings, or a new co-filer, is a new key. 1,398 alerts for 1,135 contests.
- **Missed contests:** 131 contests never alerted, because the same challenger had fought the same company before and that key was already in the sent log.
- **No start:** `first_known_on` was the first filing *in the batch*, so a contest's real start was unknown.

Changing what an event is means a new version.

## Definition

A **contest** is a company's run of contest filings (the forms of `@1`: `PREC14A`, `PREC14C`, `DEFC14A`, `DEFC14C`, `PREN14A`, `PRRN14A`, `DEFN14A`, `DFAN14A`), in the ledger and the batch together, in which no two consecutive filings are more than **90 days** apart. A filing more than 90 days after the company's previous contest filing starts a new contest.

Why 90 days: of the 9,207 gaps between consecutive contest filings at the same company (2015 to 2026), 92% are 30 days or less (1,678 the same day, 5,250 within a week, 1,705 within a month), 265 are 31 to 60 days, and only 62 are 61 to 90 days, after which they spread out (60 at 91 to 180, 98 at 181 to 365, 89 over a year, mostly the next year's annual meeting).

An event fires for each contest with at least one filing in the batch. Point in time: only ledger filings accepted no later than the batch's last day for that company are used, so replaying an old folder never sees what came after it.

## Output

The fields of `@1`, with these meanings:

| Field | Meaning in `@2` |
| --- | --- |
| `challengers`, `challenger_proxy_statement`, `company_contested_proxy` | Over the **whole contest** so far, ledger and batch |
| `first_known_on` and `contest_start` | The contest's first filing |
| `last_known_on` | Its latest filing |
| `earlier_filings` | Its filings already in the ledger before this batch (0: the contest starts here) |
| `filings` | The batch's own filings (the alert's receipts) |

Alert key: rule, company and contest start. So a contest alerts once, whoever joins it later, and a new contest at the same company alerts again.

## Parameters

None. The 90 days is `NEW_CONTEST_AFTER_DAYS`.

## Known limitations

- The ledger's contest filings start in 2015 (the history download). A contest in early 2015 that began in 2014 looks new.
- Two fights at one company less than 90 days apart (for example, a settled contest followed by a different activist) are one contest. The challengers list shows both.
- A gap over 90 days inside one long fight (a contest that pauses and resumes) gives two contests; 2015 to 2026 has 60 gaps of 91 to 180 days.
- Everything `@1` does not do still applies: no reading of the documents, exempt solicitations not counted.

## How this was checked

- Unit tests (`crates/oe-events/src/proxy_contest.rs`): a continuing contest keeps its start and its whole cast; 91 days starts a new contest and 90 does not; a folder spanning two seasons gives two events; ledger filings after the batch, at other companies, of other forms or repeating the batch's own are ignored.
- Alert test (`crates/oe-alerts/src/convert.rs`): a new co-filer joining keeps the same key, where `@1` made a new one; a contest a season later gets a new key.
- Database test: `contest_history` returns every filer in order, as loaded.
- Real runs, 2026-11-23: `oe proxy-contests data/proxy --db` over all 10,108 headers gives 1,135 contests since 2015 (the same as the SQL replay), where `@1` gives 895 events because it merges each company's contests across years. `oe alerts --proxy <one day> --db` for 2026-04-10 (4 filings) gave 3 alerts, each placed in its running contest: WEX and Impactive Capital since 2026-02-09 (4 earlier filings), Ingles Markets and Summer Road since 2026-03-12 (6), lululemon and Dennis J. Wilson since 2025-12-29 (21).

## Evidence

Not yet studied. With contest starts known, the Phase 4 study can measure outcomes from `contest_start`.
