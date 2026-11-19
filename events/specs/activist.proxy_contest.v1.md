# `activist.proxy_contest@1`

**Status:** active · **Code:** `crates/oe-events/src/proxy_contest.rs` · **Reader:** `crates/oe-edgar/src/header.rs` · **Fixtures:** `fixtures/proxy/`

## Question it answers

Is someone other than the company's own board asking shareholders for their votes, that is, is there a proxy contest?

## Background

When a shareholder (an activist fund, a founder, a rival bidder) wants to replace directors or block a deal, they solicit proxies against the board. The SEC has separate form types for contested solicitations:

| Form | Meaning |
| --- | --- |
| `PREC14A`, `DEFC14A` (and `PREC14C`, `DEFC14C`) | Preliminary / definitive proxy statement in a contested solicitation |
| `PREN14A`, `PRRN14A`, `DEFN14A` | Preliminary / revised preliminary / definitive proxy statement filed by a **non-management** party |
| `DFAN14A` | Additional soliciting material filed by a non-management party: letters, presentations, press releases |

These filings are HTML, so v1 does not read the documents. It reads the small SGML header of each filing (`<accession>.hdr.sgml`), which names the **subject company** and every party the filing is **filed by**, with their CIKs.

**The form type does not say who filed.** In the 26 sampled filings, Pacira's `DEFC14A` and lululemon's `PREC14A` were filed by the company, but Fermi's `PREC14A` was filed by a shareholder (Toby Neugebauer) and Two Harbors' `DEFC14A` by a rival bidder (UWM Holdings). The rule therefore compares the filer with the subject company.

## Definition

The rule takes a batch of contest filings and fires **one event per subject company** that has at least one of the forms above in the batch.

For each filing, a filer is a **challenger** when its CIK is not the subject company's CIK. The event lists every challenger seen in the batch.

## Output

| Field | Meaning |
| --- | --- |
| `issuer`, `issuer_name` | The subject company |
| `challengers` | Every non-company filer (CIK and name), in order of first appearance |
| `first_known_on`, `last_known_on` | The earliest and latest US Eastern acceptance date among the batch's filings for this company |
| `challenger_proxy_statement` | A challenger filed a proxy statement (`PREN14A`, `PRRN14A`, `DEFN14A`, or a `PREC14A`/`DEFC14A` under its own CIK): the contest has reached a formal vote |
| `company_contested_proxy` | The company filed a contested proxy statement (`PREC14A`/`DEFC14A`/`PREC14C`/`DEFC14C` under its own CIK) |
| `filings` | Each filing's form, accession, filers and acceptance date |

## Parameters

None.

## Known limitations

- **It is per batch, not per contest.** Run daily, the same contest fires again each day it has filings; telling a new contest from a continuing one needs the filing history in the database. `first_known_on` is the first filing *in the batch*.
- It does not read the documents, so it cannot say what the challenger wants (board seats, a sale, against a merger) or how many seats.
- Exempt solicitations (`PX14A6G`, "vote no" campaigns) and ordinary proxy materials (`DEFA14A`) are not counted.
- A rival bidder fighting a merger is a challenger here, as is an activist fund: both solicit against the board.
- Challengers include individuals (founders, activists). The project reports the names the SEC header gives, in their public role as soliciting shareholders, and never their addresses.

## Evidence

Not yet studied. The Phase 4 study will look at outcomes after contests start, measured from `first_known_on`, once the history lets "start" be known.
