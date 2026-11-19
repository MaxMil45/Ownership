# Field Guide: Item 408 (insider trading arrangements in 10-Q and 10-K)

Item 408(a) of Regulation S-K makes a company disclose, in every 10-Q and 10-K, any Rule 10b5-1 trading plan that one of its directors or officers **adopted or terminated** during the quarter, with the person's name and title and the plan's material terms. Rule 10b5-1 plans let insiders sell on a pre-set schedule; ending one is a signal worth knowing about.

The disclosure arrives with the quarterly report, weeks after the change. It is not a real-time signal.

## Where the data is

Companies tag the disclosure in the report's Inline XBRL with the `ecd` taxonomy (Executive Compensation Disclosure), so no prose parsing is needed. The report is XHTML: facts are `<ix:nonNumeric>` / `<ix:nonFraction>` elements, and the context each fact refers to (`<xbrli:context>`, in the hidden `ix:header` block) says which person and which plan it is about.

| `ecd:` concept | What it holds |
| --- | --- |
| `Rule10b51ArrAdoptedFlag`, `Rule10b51ArrTrmntdFlag` | A Rule 10b5-1 plan was adopted / terminated. |
| `NonRule10b51ArrAdoptedFlag`, `NonRule10b51ArrTrmntdFlag` | The same for arrangements that are not Rule 10b5-1 plans. |
| `TrdArrIndName`, `TrdArrIndTitle` | The person and their title. |
| `TrdArrAdoptionDate`, `TrdArrTerminationDate`, `TrdArrExpirationDate` | Dates, written as the company chose (`August 6, 2026`, `8/11/2026`). |
| `TrdArrSecuritiesAggAvailAmt`, `TrdArrDuration` | Maximum shares and length in days. |
| `MtrlTermsOfTrdArrTextBlock` | The text of the disclosure (sometimes only "On August 6, 2026"). |

## Traps

- **A flag is only true if its `format` says so.** Companies with nothing to report still tag the flag, with `format="ixt:fixed-false"`, and the visible text still says "terminated". Of the first 8 reports checked, 5 had such a false "terminated" flag and only 1 had real terminations.
- **Most companies tag only the person, not each plan.** The context names a person (`ecd:IndividualAxis`), sometimes also a plan (`ecd:TradingArrAxis`), sometimes neither (a company-level statement). Coinbase, Robinhood, Carvana and Snap use the person alone; Palantir names every plan. Grouping only by plan finds nothing for the first group.
- **The name and title are often on only one of a person's plans.** Palantir tags "Shyam Sankar" on the terminated plan and the title only on his new one; the new plan says "Mr. Sankar".
- **Dates come in several styles** and can contain non-breaking spaces (`October&#160;22, 2027`).
- **Tags are as filed, and sometimes wrong.** Palantir tags a plan terminated on 14 August 2026 with an expiration date of 31 March 2026. Cryoport tags its plan's scheduled end (12 March 2027) as the termination date in a report for the quarter ended 30 September 2026; the real date is only in the text. 3 of 25 termination dates on 2026-11-05 were after the quarter end.
- **A true flag can be company-wide.** Twin Vee tags `Rule10b51ArrTrmntdFlag` as `ixt:booleantrue` with no person or plan, while its text says no director or officer terminated anything. Formats seen for these flags: `fixed-true`, `fixed-false` and `booleantrue`. `fixed-false` also appears on a person's context (7 times in 19 reports): that person did not terminate.
- **One person can end more than one plan.** ARS Pharmaceuticals' former CEO and former CMO each ended two plans (`PlanOneMember`, `PlanTwoMember`): four people, six terminations.

## Reading it cheaply

A 10-Q or 10-K is 1.5 to 3 MB. SEC's renderer also writes a small page for each report in the filing; the one called "Insider Trading Arrangements" (10 to 60 KB) shows these facts as rows such as `Rule 10b5-1 Arrangement Terminated | true`. `FilingSummary.xml` (30 to 60 KB) lists the pages and names the main document. `oe daily-periodic` reads the page first and downloads the full report only when it shows a termination.

## How common it is

On 2026-11-05, 647 10-Qs and 10-Ks were filed. 24 (3.7%) had no insider-trading page. 18 (2.8%) reported at least one terminated plan, 25 terminations in all. The report page check (read the 10 to 60 KB page, download the full report only when it shows a termination) skipped 603 reports; a sample of 30 of those had no termination in the full extractor.

## Acceptance time: EDGAR's filing date can be a day late

For periodic reports, filing date means something different from acceptance date: a report accepted after 5:30 pm Eastern is dated the next business day. Real example: Palantir's 10-Q was accepted at 18:00:39 Eastern on 2 November 2026 and has `FILING-DATE` 3 November; ACADIA and Amplitude were accepted after 18:00 on 4 November and carry 5 November. The small `.hdr.sgml` file next to the filing (`.../<accession without dashes>/<accession>.hdr.sgml`, under 1 KB) gives the exact acceptance time in US Eastern, and `known_on` is its date.

## What this project does with it

`oe-item408` reads the tags; `insider.plan_terminated@1` (`events/specs/insider.plan_terminated.v1.md`) turns terminated plans into events. `oe parse-408 <file>` shows what one report says, `oe daily-periodic <date>` collects reports, and `oe plan-terminations <folder>` lists the events.
