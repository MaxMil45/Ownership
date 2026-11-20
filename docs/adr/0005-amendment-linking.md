# ADR 0005: Link Form 3/4/5 amendments by evidence, load only restatements

**Status:** accepted · **Date:** 2026-11-20

## Context

ADR 0004 stored a `4/A` or `5/A` as a receipt and held back its transaction rows, because the XML names no accession number for the filing it amends and a guessed link would show two versions in `insider_transaction_as_of`.

Two facts, measured on 36 real amendments filed 2026-11-16 to 2026-11-19 (3/A, 4/A, 5/A):

- Every amendment carries `dateOfOriginalSubmission`. Matching it against reports by the **same first reporting owner, same issuer, base form (4 for a 4/A), accepted that US Eastern day and before the amendment** gave exactly one report for 34 of 36. The other two have no match: one names a Sunday (2026-11-15), one has no same-day report for that issuer.
- Amendments are of two kinds. Most **restate** the report with a correction (12 from one filer agent moved every transaction date by a week; others change a share count or a holding; one adds an omitted row at the end). Some only **supplement** it: they report the omitted or corrected rows alone (an 8-row 4/A of a 13-row report; a 4/A with only the RSUs "not reported on the original"; a 4/A whose single row is a different purchase from the original's). Treating a supplement as a replacement would hide real trades.

## Decision

- Migration `0002_amendments.sql` adds four append-only tables: `filing_owner` (every owner of a joint filing, in document order), `ownership_filing` (issuer and `dateOfOriginalSubmission`), `amendment_transaction` (an amendment's rows as filed, outside the ledger) and `amendment_link` (the outcome and its reason).
- After each load batch, `link_amendments` takes every amendment without a link, oldest first:
  - **no candidate**: nothing is written; it is tried again on the next load (the original may arrive later).
  - **more than one candidate**: `ambiguous`, rows held back. More filings cannot make this unambiguous.
  - **one candidate**: compare with its current version (the newest loaded amendment of it, otherwise the report itself). It is a **restatement** when it has at least as many rows and each existing row is matched, in the same position, by a row with the same table (derivative or not) and code, and the same date or the same share count. Then its rows enter `insider_transaction`, row *i* superseding row *i*; extra rows supersede nothing. Otherwise `held_back`, with the reason.
  - An amendment accepted **before** an already loaded amendment of the same report is held back, so a report never has two current versions.
- The rule lives in `oe-store::amend::compare`, a pure function tested with the real cases above.

## Consequences

- On the 36-amendment sample: 30 loaded (43 rows), 4 held back, 2 waiting, 0 ambiguous. The four held back are exactly the supplements and the changed trade.
- Supplements stay out of the ledger, so their genuinely new rows (an omitted grant, for example) are missing from `insider_transaction_as_of`. They are kept in `amendment_transaction` with a reason in `amendment_link`, so a later, better rule can use them. Missing data is preferred to double counting.
- A filing accepted after 10 pm Eastern gets the next business day as its filing date, which would not match; such an amendment waits. Not seen in the sample.
- Filings loaded before migration 0002 get their owners and issuer details, and their amendments get linked, when their folder is loaded again (`db-load` is safe to repeat).
- `insider_transaction.owner_cik` still holds the first owner only; queries that need every owner of a joint filing join `filing_owner`.
