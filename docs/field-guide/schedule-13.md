# Field Guide: Schedules 13D and 13G

A holder who crosses 5% of a class of a public company's stock must say so. There are two forms:

- **Schedule 13D** for holders who may want to influence the company. It includes a free-text "Purpose of Transaction" (Item 4). Due within five business days of crossing 5% (it was ten calendar days until 2024).
- **Schedule 13G**, the short form, for institutions (Rule 13d-1(b)), passive investors under 20% (Rule 13d-1(c)) and exempt investors (Rule 13d-1(d)). It is filed after the quarter, and amended when the holding changes.

A holder who starts passive and later wants influence must **switch** from 13G to 13D. A 13D that follows a 13G ticks a box on its cover page saying so.

Everything here was checked on 382 real filings from five days in 2026 (11-17, 11-18, 08-14, 05-14 and 02-13): 33 original 13Ds, 112 13D/As, 195 13Gs and 42 13G/As. All parse.

## Where to get it

Since December 2024 both forms are XML. In EDGAR's daily index the form types are `SCHEDULE 13D`, `SCHEDULE 13D/A`, `SCHEDULE 13G` and `SCHEDULE 13G/A` (the old names were `SC 13D` and `SC 13G`). The XML is the first `<XML>` block of the complete submission text file, as for Form 144. `oe daily-13dg <date>` downloads a day's worth.

How many there are: a quiet day (2026-11-18) had 4 13Ds, 21 13D/As, 10 13Gs and 2 13G/As. 13G deadline days are huge: 2026-08-14 had 1,127 13G/As and 454 13Gs, 2026-02-13 had 641 and 214. 13Ds stay at about 3 to 10 a day, with 20 to 25 amendments.

## Structure

The two forms use **different element names** for the same facts.

| Fact | 13D | 13G |
| --- | --- | --- |
| Issuer CIK | `issuerInfo/issuerCIK` | `issuerInfo/issuerCik` |
| CUSIP | `issuerCusips/issuerCusipNumber` (or one `issuerCUSIP`) | `issuerCusips/issuerCusipNumber` |
| Event date | `dateOfEvent` | `eventDateRequiresFilingThisStatement` |
| A reporting person | `reportingPersons/reportingPersonInfo` | `coverPageHeaderReportingPersonDetails` (repeated) |
| Person's CIK | `reportingPersonCIK` | not given |
| Shares and percent | `aggregateAmountOwned`, `percentOfClass` | `reportingPersonBeneficiallyOwnedAggregateNumberOfShares`, `classPercent` |
| Voting and dispositive power | direct children | under `reportingPersonBeneficiallyOwnedNumberOfShares` |
| Purpose | `items1To7/item4/transactionPurpose` | none; Item 10 is a certification of passivity |
| Which rule | none | `designateRulePursuantThisScheduleFiled` (`Rule 13d-1(b)`, `(c)`, `(d)`) |
| Came from a 13G | `previouslyFiledFlag` | none |
| Amendment | `amendmentNo`, `previousAccessionNumber` | the same |

Dates are `MM/DD/YYYY`, numbers have two decimals (`195357000.00`), and `typeOfReportingPerson` uses the cover-page codes (`IN` individual, `CO` corporation, `IA` investment adviser, `HC` parent holding company, `PN` partnership, `OO` other).

## Traps found in real filings

- **Joint filers state the same shares.** A fund and its manager each report the full position, so adding persons double counts. Use the largest. Of 33 original 13Ds, 23 had more than one reporting person (up to 15).
- **Namespace prefixes vary.** 36 of 382 filings put a prefix on every element (for example `sch:edgarSubmission`). Match on local names. Some write extra `xmlns` attributes on individual elements, and one filing's XML contains a comment inside `filerCredentials`.
- **The CUSIP is not always where you expect.** Sharps Technology's 13D has a single `<issuerCUSIP>` instead of the `issuerCusips` list; a parser that only reads the list finds no CUSIP.
- **Amendments carry less.** A 13D/A may omit Item 4 (Autoliv's did). `previousAccessionNumber`, which names the filing the amendment amends, is on 52 of 112 13D/As and 39 of 42 13G/As, so it cannot be relied on.
- **A 13D is not always an activist.** The 33 originals include a 93% holder, an 86% holder after an acquisition, and two filings that report **under** 5%: 0.51% (Apollo's) and 0% (an individual who disposed of a material number of securities, filed as an original 13D). Item 4 says what the filer intends.
- **A 13G can report 0%.** Two sampled 13G/As report 0%: the holder dropped below 5%. 5 sampled original 13Gs also state under 5%.
- **`previouslyFiledFlag` means "this holding was earlier reported on a 13G".** Checked on the three originals that tick it: Hilton Grand Vacations ("previously reported on Schedule 13G … filing this Schedule 13D to report that they have initiated communications"), Perfect Moment and MDxHealth (both "pursuant to Rule 13d-1(e)"). 6 of 112 13D/As tick it as well.
- **Individuals file too.** 204 of 382 filings have an individual (`IN`) among the reporting persons. The filing can contain their address, sometimes a residence. The fixtures in `fixtures/sched13/real/` contain only filings by entities, and none of them names an individual among its reporting persons.

## What this project does with it

`oe-sched13` parses both forms. `activist.new_13d@1` and `activist.passive_to_active@1` (`events/specs/`) turn original 13Ds into events. `oe activist-events data/sched13` lists them.
