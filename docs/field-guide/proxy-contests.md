# Field Guide: proxy contests

A proxy contest is a fight for shareholders' votes: someone other than the board (an activist fund, a founder, a rival bidder, even a union) asks shareholders to vote their way, usually to elect their own directors or to block or force a deal.

Everything here was checked on 70 contested-solicitation filings from 21 days in 2026 (most in the March to May proxy season), which make up 26 contests.

## The forms

| Form | Who usually files it | What it is |
| --- | --- | --- |
| `PREC14A` / `DEFC14A` | The company, or a challenger | Preliminary / definitive proxy statement in a **contested** solicitation |
| `PREN14A` / `PRRN14A` / `DEFN14A` | A challenger ("non-management") | Preliminary / revised preliminary / definitive proxy statement |
| `DFAN14A` | A challenger | Additional soliciting material: letters to shareholders, presentations, press releases. By far the most common (51 of the 70). |
| `PREC14C` / `DEFC14C` | | The information-statement versions (no proxies solicited) |
| `PX14A6G` | A shareholder | Exempt solicitation ("vote no" campaigns): **not** a contest here |
| `DEFA14A` | The company | Ordinary additional proxy material: **not** a contest by itself |

The documents are HTML letters and long proxy statements with no structured data. What is structured is the filing's **SGML header**: `.../<CIK>/<accession without dashes>/<accession>.hdr.sgml`, under 1 KB.

## What the header says

```text
<SUBJECT-COMPANY>  ... <CONFORMED-NAME>WEX Inc.            <CIK>0001309108 ...
<FILED-BY>         ... <CONFORMED-NAME>Impactive Capital LP <CIK>0001786767 ...
```

The subject company is the company the contest is about. Each `FILED-BY` block is a party that filed. A filing can have several `FILED-BY` blocks (a fund and its general partner).

## Traps found in real filings

- **The form type does not say who filed.** `PREC14A` and `DEFC14A` are filed by the company in some contests (Pacira, lululemon, Sturm Ruger) and by the **challenger** in others: Fermi's `PREC14A` by a shareholder (Toby Neugebauer), Genco's by Diana Shipping, Two Harbors' `DEFC14A` by a rival bidder (UWM Holdings). Compare `FILED-BY` with `SUBJECT-COMPANY`.
- **A company's own filing names itself in `FILED-BY`.** Pacira's `DEFC14A` lists Pacira as subject and as filer.
- **There can be more than one challenger.** Two Harbors had two rival bidders (CrossCountry and UWM) soliciting against the board.
- **Challengers are varied:** activist funds (Trian at Janus Henderson, Impactive at WEX, Blackwells, Land & Buildings), a hostile bidder (Paramount Skydance at Warner Bros. Discovery), a founder (lululemon), individuals (Bradley Radoff at Seer), a union (Communications Workers of America at Nexstar).
- **Contests stretch over weeks**, so a day's filings are a slice: Fermi had seven contest filings between 5 and 26 May 2026.
- Headers include business addresses of the filers, and some challengers are individuals. The fixtures in `fixtures/proxy/real/` use only contests whose challengers are companies or funds.

## Volume

One to eight contest filings a day in the 2026 proxy season (March to May), and none at all on 2026-08-14. Of the 70 sampled, 51 were `DFAN14A`, 9 `PREC14A`, 7 `DEFC14A`, 2 `PRRN14A` and 1 `PREN14A`.

## What this project does with it

`oe daily-proxy <date>` downloads the headers of a day's contest filings; `activist.proxy_contest@1` (`events/specs/activist.proxy_contest.v1.md`) groups them by subject company; `oe proxy-contests data/proxy` lists the contests.
