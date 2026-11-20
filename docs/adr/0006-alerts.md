# ADR 0006: Alerts from files, delivered once, to webhooks and Discord

**Status:** accepted · **Date:** 2026-11-20

## Context

Phase 1 promises alerts with receipts, and Gate 1 asks for them within 5 minutes of filing. The event rules already exist (`oe-events`), each run over a folder of downloaded filings. Nothing turned an event into a message, remembered what was sent, or delivered it.

Measured on 2026-11-20 between 10:21 and 10:28 am Eastern (`oe follow`, polling every 30 s): the four Form 4s that appeared during the run showed up in the EDGAR latest-filings feed 34 to 66 seconds after SEC acceptance, polling wait included, and the feed's time stamp equals the acceptance time. A small sample, at one time of day.

## Decision

- A new crate, `oe-alerts`. Its core is standard library only (ADR 0001): `convert` (one `Alert` per event, with a headline, a few facts and **receipts**, the SEC folder of every filing the event rests on), `watchlist`, `sent`, `select` and `render`. HTTP delivery (`send`) is behind its `net` feature and uses `ureq`, like `oe-edgar`.
- Every alert has a **key**: the facts that make it the same event. Cluster buy: rule, issuer, first day of the window (the `event` table's unique key). 13D, 13G switch and Form 144: rule and accession. Plan termination: rule, accession, person. Proxy contest: rule, issuer and the sorted challenger CIKs, so a contest is announced once and again only when a new challenger appears.
- **Delivered once per destination.** `data/alerts/sent.tsv` gets one appended line per delivery (time, destination, key). A failed delivery is not recorded and is retried on the next run. `--dry-run` records nothing.
- **Fresh only.** By default an alert is delivered only if its event became knowable in the last 3 days (`--max-age`), and undated events are skipped (`--include-undated`), so a first run over months of downloads does not flood anyone.
- **Destinations from the environment**, because a webhook URL is a secret: `OE_DISCORD_WEBHOOK_URL` and `OE_ALERT_WEBHOOK_URL` (a JSON POST; the field names are a public interface: add fields, never rename them). HTTPS only, except plain HTTP to this machine. URLs are never printed, not even in error messages. Discord messages have mentions switched off, are cut to 2,000 characters, and are paced 0.5 s apart; on HTTP 429 the sender waits as asked (at most 10 s) and tries once more. With no destination set, new alerts are printed.
- **Watchlists** are text files: `ticker`, `issuer` (CIK), `insider` (CIK), `name` (part of a person's or holder's name) and `rule` lines. No watchlist means every alert.
- **Two ways to run.** `oe alerts` reads the download folders (every rule). `oe follow --alert` runs it over the folders that got new filings after every poll, which is the fast path.

## Consequences

- Form 4 cluster buys can alert about a minute after acceptance: feed delay as measured above, plus the download (one request per filing) and an alert pass (44 ms over 50 filings). Not yet measured over a full day.
- ~~Form 144, 13D/G, proxy and 10-Q/10-K events come from the daily index, so their alerts are a day late.~~ **Update 2026-11-20:** `oe follow` now polls live feeds for Form 144 (`type=144`), Schedule 13D/13G (`type=SCHEDULE 13`) and the contest forms (`PREC14`, `DEFC14`, `PREN14A`, `PRRN14A`, `DEFN14A`, `DFAN14A`), nine feed requests per poll. Measured in a 9-minute run (15:40 to 15:50 UTC): a Form 4 arrived 86 s and a Form 144 62 s after acceptance, with polls about 40 s apart (30 s pause plus the poll itself). Only 10-Q/10-K plan terminations still wait for the daily index.
- A cluster buy is announced once, when it first qualifies; insiders who join the window later do not trigger a second alert.
- No email yet: sending mail needs an SMTP client with TLS, which is a larger dependency decision. A JSON webhook can feed an email service in the meantime.
- Each run rereads the folders. Fine at today's size (about 1,100 files, under a second); the database will replace this once history is loaded.
