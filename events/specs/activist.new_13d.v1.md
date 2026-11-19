# `activist.new_13d@1`

**Status:** active · **Code:** `crates/oe-events/src/activist.rs` · **Parser:** `crates/oe-sched13` · **Fixtures:** `fixtures/sched13/`

## Question it answers

Did someone just report owning more than 5% of a company's stock on the form meant for holders who may want to influence it?

## Background

A holder who crosses 5% of a class of a public company's stock must file a Schedule 13D within five business days (since 2024; it was ten calendar days) **unless** they qualify for the short, passive form, Schedule 13G (institutions, and passive investors under 20%). A 13D has a free-text Item 4, "Purpose of Transaction", where the filer says what they intend: investment only, talks with management, a board seat, a sale of the company.

A new 13D is the first public sign of a large holder who is not simply passive. It is not always an "activist" in the campaigning sense: the 33 original 13Ds in the sample below include a controlling holder of a company that just did an acquisition (86%), an individual with 5.1%, and several funds.

Since December 2024 these filings are XML; older ones are out of scope.

## Definition

An event fires for each **original Schedule 13D** (`submissionType` exactly `SCHEDULE 13D`, not `SCHEDULE 13D/A`) where the largest percentage of the class any one reporting person states is **at least 5%**.

Joint filers (a fund and its manager) usually state the same shares, so the percentages are not added up: the largest one is used.

Amendments (`/A`) never fire an event. Nor does a 13D that states no percentage, or one that states less than 5%: two of the 33 sampled originals report 0.51% and 0% (a holder that sold out).

## Output

| Field | Meaning |
| --- | --- |
| `issuer`, `issuer_name`, `class_title` | The company and the share class |
| `holders` | Names of every reporting person |
| `largest_percent`, `largest_shares` | The largest stated percentage and share count among the reporting persons |
| `is_group` | At least one reporting person ticks "member of a group" (`memberOfGroup` = `a`) |
| `event_date` | `dateOfEvent`: the date that triggered the filing |
| `previously_filed_13g` | The filer says the holding was earlier reported on a Schedule 13G (see `activist.passive_to_active@1`) |
| `purpose` | Item 4 (`transactionPurpose`), as written. **Not classified**: reading it is left to the user. |
| `accession`, `known_on` | The filing and the US Eastern date the SEC accepted it |

## Parameters

| Parameter | v1 default | CLI flag |
| --- | --- | --- |
| `min_percent` | 5.0 | `--min-percent` |

Changing a default is a new version (`@2`).

## Known limitations

- It does not tell activists from other 13D filers. Of 33 original 13Ds in the sample, some are controlling holders or insiders. The Item 4 text is included so a person can tell.
- A filing must be an XML 13D (December 2024 onwards).
- The percentage is the filer's own, against the share count they chose (often the issuer's last 10-Q or 8-K).
- A holder files late sometimes; `event_date` and `known_on` show by how much.
- Filings by individuals are public disclosures of a large holding in a company; the project reports names as filed and never adds personal details.

## How this was checked

- 382 real filings from five days in 2026 (11-17, 11-18, 08-14, 05-14, 02-13) were parsed with no failure. 33 were original 13Ds: 31 gave an event, 2 did not (0.51% and 0%). A separate count from the raw XML (form type, largest stated percentage) gave the same 31.
- 14 of those filings, all by entities, are in `fixtures/sched13/real/` with a manifest worked out from the raw XML; the tests compare the code to it.
- Not checked: other periods, how often an original 13D comes weeks after the event date, and whether a 13D's reporting persons are really independent of each other.

## Evidence

Not yet studied. The Phase 4 study will test returns after new 13Ds, split by size, group, and what Item 4 says, measured from `known_on`.
