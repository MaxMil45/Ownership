# `activist.passive_to_active@1`

**Status:** active · **Code:** `crates/oe-events/src/activist.rs` · **Parser:** `crates/oe-sched13` · **Fixtures:** `fixtures/sched13/`

## Question it answers

Did a holder that had reported its stake as passive (Schedule 13G) just change to the form for holders who may want to influence the company (Schedule 13D)?

## Background

Institutions and passive investors file the short Schedule 13G. If their intent changes (they start talking to management, seek a board seat, or lose their eligibility, for example by crossing 20%), they must switch to a Schedule 13D, and the 13D has a box for exactly this: the filer says they **previously reported this holding on a Schedule 13G** (Rule 13d-1(e), (f) or (g)). A switch from passive to active is a stronger signal than a first-time 13D, because the holder already owned the stake quietly.

Verified on the three real filings that tick the box: Hilton Grand Vacations ("previously reported on Schedule 13G … filing this Schedule 13D to report that they have initiated communications"), Perfect Moment and MDxHealth (both "pursuant to Rule 13d-1(e)").

## Definition

An event fires for each **original Schedule 13D** (`SCHEDULE 13D`, not an amendment) whose cover page sets `previouslyFiledFlag` to true.

No percentage threshold: the switch is the signal. (`activist.new_13d@1` fires on the same filing if it states at least 5%.)

## Output

The same fields as `activist.new_13d@1`, plus `basis`, which in v1 is always `filer's own previouslyFiledFlag`.

## Parameters

None.

## Known limitations

- It relies on the filer's own flag. A holder who switched but left the box unticked is missed, and the box is also ticked by filers who previously reported a 13G for the acquisition but not necessarily as a "passive" holder in the everyday sense. The project does not yet check it against the earlier 13G filings; that needs the 13G history in the database.
- Only original 13Ds are counted: 6 of 112 sampled 13D/A filings also tick the box, but an amendment continues an earlier 13D.
- XML filings only (December 2024 onwards).

## How this was checked

- Of the 33 original 13Ds in 382 real filings (five days in 2026), 3 tick the box, matching a separate count from the raw XML. The flag is never ticked on a filing that does not say, in its own text, that it follows a 13G (checked on those 3).
- The earlier 13Gs were not looked up, so whether every flagged holder really had a filed 13G for the same issuer is unverified.
- 1 flagged 13D, Perfect Moment's, is in `fixtures/sched13/real/`; the other two name individuals among their reporting persons and are not.

## Evidence

Not yet studied. The Phase 4 study will compare returns after a switch with returns after other new 13Ds, measured from `known_on`.
