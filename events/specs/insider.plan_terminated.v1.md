# `insider.plan_terminated@1`

**Status:** active · **Code:** `crates/oe-events/src/plan_terminated.rs` · **Extractor:** `crates/oe-item408` · **Fixtures:** `fixtures/item408/real/`

## Question it answers

Did a director or officer end a pre-arranged (Rule 10b5-1) trading plan, according to the company's quarterly or annual report?

## Background

Item 408(a) of Regulation S-K requires a company to disclose, in each 10-Q and 10-K, any Rule 10b5-1 trading arrangement that one of its directors or officers **adopted or terminated** during the quarter, with the person's name and title and the plan's terms. Companies tag this in Inline XBRL using the `ecd` (Executive Compensation Disclosure) taxonomy, so it can be read without parsing prose.

The disclosure arrives with the report, weeks after the plan changed. The event is therefore not a real-time signal; its `known_on` is the report's acceptance date.

## Definition

An event fires for each **trading arrangement** in a 10-Q or 10-K for which the filing tags `ecd:Rule10b51ArrTrmntdFlag` as **true** (`format` `ixt:fixed-true`, or `ixt:booleantrue`, which real reports have so far only used for company-level flags) in a context that names a plan (`ecd:TradingArrAxis`), a person (`ecd:IndividualAxis`), or both.

How companies tag differs. Some name each plan (Palantir: one member per plan, so a person who ended one plan and adopted another gives two arrangements). Most name only the person; then all of that person's plan facts in the filing form **one** arrangement, and a person who ended two plans in a quarter gives one event. The report is not otherwise split.

Two kinds of tagged flag do **not** fire an event:

- a flag with `format="ixt:fixed-false"`: the company's statement that nothing was terminated (its text reads "terminated", "expire" or similar even though the fact is false);
- a flag whose context names neither a plan nor a person: a company-level statement, not a plan.

`ecd:NonRule10b51ArrTrmntdFlag` (arrangements that are not Rule 10b5-1 plans) is not covered in v1.

## Output

| Field | Meaning |
| --- | --- |
| `issuer`, `issuer_name` | The company (`dei:EntityCentralIndexKey`, `dei:EntityRegistrantName`) |
| `person`, `title` | `ecd:TrdArrIndName` and `ecd:TrdArrIndTitle`. The title is often tagged on the person's other arrangement; the event takes it from any arrangement of the same individual in the filing. |
| `termination_date` | `ecd:TrdArrTerminationDate` if the filing tags one **and it is not after the end of the period**; otherwise empty. Never guessed from prose. |
| `rejected_termination_date` | A tagged date that was after the period end and so cannot be the termination (see Known limitations). Shown, not hidden. |
| `period_end` | End of the reporting period, which bounds the date |
| `replaced_in_filing` | `true` if the same person adopted a plan in the same filing (`ecd:Rule10b51ArrAdoptedFlag` true): the plan was replaced, not just dropped |
| `terms_text` | The tagged text block (`ecd:MtrlTermsOfTrdArrTextBlock`), as written |
| `form`, `accession`, `known_on` | The report and the US Eastern date the SEC accepted it |

## Parameters

None in v1.

## Known limitations

- Only reports that use the `ecd` tags are covered. Tagging was phased in, so older 10-Qs and 10-Ks, and some filers, give nothing.
- **Tagged termination dates are not all true.** On 2026-11-05 all 25 events had a `TrdArrTerminationDate` tag, but 3 of them (12%: Cryoport, Verastem, Amplitude) were after the end of the quarter. Cryoport's visible text says the plan was terminated on 11 August 2026 and `would have terminated` on 12 March 2027, and the tag holds the March 2027 date. Such dates are rejected; the real date is then only in `terms_text`.
- On 2026-11-05, 647 reports (10-Q and 10-K) were filed. 18 of them (2.8%) gave events: 25 events, for 23 people at 18 companies. 11 of the 25 had a new plan adopted in the same report, and 5 were by people whose title starts "Former" (a plan often ends when someone leaves). 24 of the 647 had no insider-trading page at all.
- A company that discloses in prose without tags is invisible.
- A report is read as the company tagged it. Palantir's filing, for example, tags a plan that was terminated on 14 August 2026 with an expiration date of 31 March 2026; the event shows what was tagged and does not repair it.
- Companies often name the person only as "Mr. Smith" in one arrangement; the full name comes from the arrangement that has it.
- Disclosure comes at the next report: weeks to months after the plan ended.
- It does not say when the plan was adopted. `insider.plan_terminated@2` (with `--db`) adds the adoption and the plan's length, and flags plans ended within the 90-day cooling-off.

## How this was checked

- `oe daily-periodic 2026-11-05 --all --audit 30` looked at all 647 reports. The cheap page check kept 19 reports and skipped 603; the full extractor, run on 30 of the skipped ones, found no termination in any. One of the 19 (Twin Vee) was a false positive of the page check: it tags a company-level "terminated" flag `booleantrue` while its text says nothing was terminated.
- For all 19 kept reports the extractor's count of terminated arrangements (25) equals a separate count from the raw XML.
- 10 real reports are in `fixtures/item408/real/` as excerpts; the extractor reads each excerpt exactly as it reads the full report, and agrees with a manifest computed from the originals.
- Not checked: other days, 10-Ks (only one fixture), reports before the ecd tags existed, and the share of reports that disclose in prose without tags.

## Evidence

Not yet studied. The Phase 4 study will test whether terminations precede changes in insider trading or returns, measured from `known_on`, with sample sizes.
