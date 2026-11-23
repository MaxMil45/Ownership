# `activist.passive_to_active@2`

**Status:** active when the ledger is used (`--db`); `@1` runs otherwise · **Code:** `crates/oe-events/src/activist.rs` (`passive_to_active_v2_event`), ledger query `oe_store::schedule13g_history` · **Replaces:** nothing; `@1` stays as it was

## Question it answers

The same as `@1`: did a holder that had reported its stake as passive (Schedule 13G) just change to a Schedule 13D? `@1` takes the filer's word for it. `@2` also looks in the ledger for the holder's own earlier 13G.

## Why a new version

`@1` fires only when the 13D's `previouslyFiledFlag` box is ticked. With the 13D/G history in the database (ADR 0009) the earlier 13Gs can be checked. Measured on every original 13D in the ledger (2,154, December 2024 to November 2026): 237 tick the box, and **77 more** have an earlier 13G from the same holder about the same company that the filer did not mention (74 matched on the SEC filer number, 3 on the name; 68 of them had stated at least 5% on that 13G). Changing what fires means a new version.

## Definition

An event fires for each **original Schedule 13D** (`SCHEDULE 13D`, not an amendment) when either

1. its cover page sets `previouslyFiledFlag` to true (exactly `@1`), or
2. the ledger holds a Schedule 13G or 13G/A about the **same issuer**, accepted on an **earlier US Eastern date** than the 13D, filed by the **same holder**: the 13G's filer CIK is one of the 13D's filer CIKs or reporting persons' CIKs, or one of its reporting persons has the same name as one of the 13D's after removing everything but letters and digits and ignoring case ("Elliott Investment Management L.P." = "ELLIOTT INVESTMENT MANAGEMENT LP"); **and** the latest such 13G does not state under 5%. A 13G/A under 5% is the usual exit filing: the holder had left, so a later 13D is a new stake, not a passive holder turning active. An unstated percentage does not count as an exit.

Point in time: only 13Gs accepted before the 13D's day count, so the event could have been raised the day the 13D appeared. A 13G from the same day is ignored (the ledger keeps exact times, but downloaded 13Ds carry only their date). A 13D without an acceptance date can fire only on its flag.

No percentage threshold, as in `@1`.

## Output

The fields of `activist.new_13d@1`, plus:

- `basis`: `filer's own previouslyFiledFlag`, `earlier 13G in the ledger (the filer did not flag it)`, or `filer's own previouslyFiledFlag and an earlier 13G in the ledger`;
- `earlier_13g`: the latest such 13G (accession, acceptance date, largest stated percent). Alerts show it and link it as a second receipt.

## Parameters

None. The exit level (5%) is fixed by the 13G rules themselves (`EXITED_BELOW_PERCENT`).

## Known limitations

- The ledger's 13G history starts on 2024-12-23 (the first XML 13Gs; older ones are text documents this project does not read). A holder whose only 13G is older is found by its flag alone. 93 flagged 13Ds show no usable 13G in the ledger: 50 filed before July 2025 (their 13G most likely predates the history), 43 later (an agent filer under another name, a name spelled differently, or an exit).
- A 13G names no reporting-person CIKs, and the ledger stores only its first submitting filer, so a 13G filed by an agent under another name is missed unless a reporting person's name matches.
- Name matching can join two different holders with the same name (for an individual, a namesake). It is the source of 3 of the 77 extra events.
- A switch is not always activism. Holders also move to a 13D when they lose 13G eligibility (crossing 20%, becoming an insider). Sampled extra events include both: funds such as General Atlantic at Alkami Technology, and founders or executives such as Warren B. Kanders at Cadre Holdings. The rule reports the switch; Item 4 says why.
- Only original 13Ds count, as in `@1`.

## How this was checked

- Unit tests (`crates/oe-events/src/activist.rs`): a match by name and by filer CIK, the latest earlier 13G chosen, 13Gs from the same day or later or about another company ignored, the flag kept, amendments and 13Gs never firing.
- Database test (`crates/oe-store/tests/postgres.rs`): `schedule13g_history` returns only 13Gs, for the issuers asked, with holders and Eastern dates.
- Real run, 2026-11-23, `oe activist-events data/sched13 --db` over 63,715 downloaded 13D/Gs and the ledger's 9,888 13Gs for their issuers: 302 events against 237 for `@1`. Every `@1` event is also an `@2` event, 65 are new, and 209 carry an earlier 13G; the gap from that 13G to the 13D ranged from 1 to 598 days, median 86. Before the exit test there were 74 new events: the first alert checked by hand (Pale Fire Capital at Angi Inc.) pointed at a 13G stating 0.9%, which is how the exit case was found; it removed 9.
- The SQL count above (77) is a little higher than the first run (74) because it also counted 13Gs accepted the same day.

## Evidence

Not yet studied, as for `@1`.
