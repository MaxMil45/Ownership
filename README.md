# Ownership Engine

An open-source engine that reads US ownership filings, works out who each investor really is, and sends evidence-backed alerts when something meaningful changes. It tells you what changed, how fresh the data is, and what history honestly says about it, with the filings behind every claim.

> **Status: Phase 1, week 1.** The Form 4 parser, the point-in-time ledger model and the first event rule (`insider.cluster_buy@1`) work and are tested against synthetic and real fixtures. Live EDGAR download (per company, per day, and a live follower) works; storing into the database and alerts come next. See [ROADMAP.md](ROADMAP.md).

## Why it exists

Raw SEC data is free, and parsers already exist ([edgartools](https://github.com/dgunning/edgartools)). What nobody open-sources is the *interpretation*: which filer names are really the same investor, which holders are passive index funds, which ownership events matter, and the evidence for that. Commercial tools sell this as closed scores. This project publishes it openly, with sources and methodology you can check.

## What works today

| Piece | Crate | State |
| --- | --- | --- |
| Dates, CIKs, accession numbers, filing models | `oe-core` | Done, tested |
| Form 3/4/5 XML parser (no dependencies) | `oe-form4` | Done, tested on synthetic fixtures |
| Point-in-time ledger (in memory) + PostgreSQL schema | `oe-ledger`, `migrations/` | Model done and tested |
| PostgreSQL loader (`db-migrate`, `db-load`, `db-status`) | `oe-store` | Done; tested against PostgreSQL 16 in Docker; 332 real filings loaded |
| History since 2015 from the SEC's quarterly bulk data sets (`bulk-insider`) | `oe-bulk`, `oe-store` | Done; checked against 200 filings' XML; loaded 2015 Q1 to 2026 Q3 (ADR 0008) |
| `insider.cluster_buy@1` rule | `oe-events` | Done, tested |
| Rate-limited EDGAR client (SEC fair-access rules) | `oe-edgar` | Done; run against the real SEC |
| SEC submissions API (with older history pages) and ticker list (dependency-free JSON reader) | `oe-edgar` | Done, tested on synthetic JSON and run live |
| Daily form index, latest-filings feed, complete submission files | `oe-edgar` | Done, tested on synthetic samples and run live |
| Form 144 (planned insider sale) parser | `oe-form144` | Done; reads 274 real filings; 17 real fixtures |
| `insider.planned_sale@1` rule | `oe-events` | Done, tested on real filings (spec in `events/specs/`) |
| Item 408 reader (insider trading plans in 10-Q/10-K Inline XBRL) | `oe-item408` | Done; run on 647 real reports; 10 real fixtures |
| `insider.plan_terminated@1` rule | `oe-events` | Done, tested on real reports (spec in `events/specs/`) |
| Schedule 13D/13G parser (large holders) | `oe-sched13` | Done; reads 382 real filings; 14 real fixtures |
| `activist.new_13d@1` and `activist.passive_to_active@1` rules | `oe-events` | Done, tested on real filings (specs in `events/specs/`) |
| `activist.proxy_contest@1` (contested proxy filings, from their SGML headers) | `oe-edgar`, `oe-events` | Done; 70 real filings from 21 days, 26 contests; 12 real fixtures |
| Alerts with receipts: watchlists, delivered once to Discord and JSON webhooks | `oe-alerts` | Done; tested over real HTTP to a local stand-in; not yet tried on a real Discord channel; no email yet (ADR 0006) |
| Dashboard: one offline HTML page with charts, filters and a searchable event list | `oe-cli` (`oe dashboard`) | Done; checked in a browser on the real downloads (ADR 0007) |
| `oe` command-line tool | `oe-cli` | `alerts`, `dashboard`, `parse-form4`, `cluster-buys`, `recent-form4`, `daily-form4`, `follow`, `daily-form144`, `parse-form144`, `planned-sales`, `daily-periodic`, `parse-408`, `plan-terminations`, `daily-13dg`, `parse-13`, `activist-events`, `daily-proxy`, `proxy-contests`, `db-*`, `fetch` |

## How to run it

See **[docs/how-to-run.md](docs/how-to-run.md)** for the full guide. The short version, once Rust is installed and `.env` holds your SEC contact line:

```powershell
.\scripts\run-day.ps1 -Date 2026-11-18
```

This downloads one day from every source and prints every event found so far.

## Quick start (Windows)

1. **Install Rust.** Download and run `rustup-init.exe` from [rustup.rs](https://rustup.rs). If it asks for the Visual Studio C++ build tools, install "Desktop development with C++" from the Visual Studio Build Tools installer, then rerun rustup.
2. **Open PowerShell in this folder** and run the tests:
   ```powershell
   cargo test --workspace
   ```
3. **Try the CLI on the synthetic fixtures:**
   ```powershell
   cargo run -p oe-cli -- cluster-buys fixtures/form4/synthetic
   cargo run -p oe-cli -- parse-form4 fixtures/form4/synthetic/02_ceo_buy_with_footnotes.xml
   ```
4. **Download real Form 4s.** The SEC requires your name and email in every request; no account or API key is needed:
   ```powershell
   $env:OE_USER_AGENT = "Your Name your.email@example.com"
   cargo run -p oe-cli -- recent-form4 AAPL --limit 10
   cargo run -p oe-cli -- cluster-buys data/form4/0000320193
   ```
   `recent-form4` accepts a ticker or a CIK, finds the company's latest Form 4s through the SEC submissions API, downloads the raw XML into `data/form4/<CIK>/` and saves each filing's SEC acceptance date in a `.meta.txt` file beside it. `cluster-buys` then reports real "knowable on" dates. The `data/` folder is not committed; copy interesting filings into `fixtures/form4/real/` as test cases.
5. **Look across all companies.** Download Form 4s from every company for one day, or follow new ones as they are filed:
   ```powershell
   cargo run -p oe-cli -- daily-form4 2026-11-18 --limit 25
   cargo run -p oe-cli -- follow
   ```
   `daily-form4` reads the EDGAR daily index (a day has about 800 Form 4s; `--all` takes them all). `follow` polls the latest-filings feeds for Form 4, Form 144, Schedule 13D/13G and contested proxies every 30 seconds until you press Ctrl-C (`--polls 2` stops after two polls; `--forms 4` for Form 4 only). Both save into `data/<kind>/<issuer CIK>/` (`form4`, `form144`, `sched13`, `proxy`) and skip filings already saved.
6. **Planned sales (Form 144).** Insiders file Form 144 before selling restricted or control shares, so it comes before the Form 4:
   ```powershell
   cargo run -p oe-cli -- daily-form144 2026-11-18 --all
   cargo run -p oe-cli -- planned-sales data/form144
   ```n   `planned-sales` lists the `insider.planned_sale@1` events: notices by officers, directors and 10% holders worth at least $1,000,000 (change it with `--min-value`), with whether the sale is under a 10b5-1 plan. `db-load` is for Form 4 folders (`data/form4`) only; Form 144 is not in the database yet.
7. **Insider plan terminations (Item 408).** Companies say in each 10-Q and 10-K which directors and officers adopted or ended a Rule 10b5-1 trading plan:
   ```powershell
   cargo run -p oe-cli -- daily-periodic 2026-11-05 --all   # about 650 reports; takes minutes
   cargo run -p oe-cli -- plan-terminations data/periodic
   ```n   `daily-periodic` reads a small page per report and downloads the 2 MB report only when it shows a termination. `--audit 30` also downloads 30 reports it skipped to measure its error rate. `parse-408 <report.htm>` shows what one report says.
8. **Large holders (Schedule 13D/13G).** Someone crossing 5% of a company files a 13D (may want influence) or a 13G (passive):
   ```powershell
   cargo run -p oe-cli -- daily-13dg 2026-11-18 --all --include-amendments
   cargo run -p oe-cli -- activist-events data/sched13
   ```n   `activist-events` lists new 13Ds (`activist.new_13d@1`, at least 5%, change with `--min-percent`) and 13Ds that follow an earlier 13G (`activist.passive_to_active@1`). On deadline days there are over a thousand 13Gs; use `--limit`.
9. **Proxy contests.** `daily-proxy <date> --all` downloads the 1 KB headers of the day's contested-solicitation filings; `proxy-contests data/proxy` groups them into contests (who is challenging which company).

On macOS or Linux, install Rust with `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` and use `export OE_USER_AGENT="..."` instead.

## Loading into PostgreSQL

The ledger tables are in `migrations/0001_init.sql`. The easiest server is the Docker one in `docker/compose.yaml` (PostgreSQL 16, reachable from this machine only, data kept in a Docker volume). With Docker Desktop running:

```powershell
docker-compose -f docker/compose.yaml up -d
$env:DATABASE_URL = "postgres://oe:oe_dev_only@127.0.0.1:55432/ownership"
cargo run -p oe-cli -- db-migrate
cargo run -p oe-cli -- db-load data/form4 --dry-run   # counts everything, no database needed
cargo run -p oe-cli -- db-load data/form4
cargo run -p oe-cli -- db-status
```

`db-load` is safe to repeat. Amendments (3/A, 4/A, 5/A) are linked to the report they amend by owner, issuer, form and `dateOfOriginalSubmission`; only amendments that restate the report replace its rows, the rest are kept aside with a reason. Every owner of a joint filing is stored. See [ADR 0004](docs/adr/0004-postgres-store-crate.md) and [ADR 0005](docs/adr/0005-amendment-linking.md). To run the database tests (they make and drop a private schema in the database you name; the Docker setup creates an empty `ownership_test` for this):

```powershell
$env:OE_TEST_DATABASE_URL = "postgres://oe:oe_dev_only@127.0.0.1:55432/ownership_test"
cargo test -p oe-store --test postgres -- --ignored --test-threads=1
```

Any PostgreSQL 15+ server works instead of Docker: point `DATABASE_URL` at an empty database. The connection uses no TLS, so use a local or trusted-network server. `docker-compose -f docker/compose.yaml down` stops the container and keeps the data; adding `-v` deletes it.

Use `docker-compose` (with a hyphen) if `docker compose` says "unknown command". The compose file lives in `docker/` so that compose does not try to read the `.env` in the repository root.

## Repository layout

```text
crates/
  oe-core/      shared types: Date, Cik, Accession, OwnershipDocument
  oe-form4/     Form 3/4/5 parser and a small XML tokenizer
  oe-form144/   Form 144 (notice of proposed sale) parser
  oe-item408/   Item 408 reader for 10-Q/10-K Inline XBRL (insider trading plans)
  oe-sched13/   Schedule 13D/13G (large holder) parser
  oe-ledger/    append-only, point-in-time fact store
  oe-events/    versioned event rules (insider.cluster_buy@1)
  oe-edgar/     polite EDGAR HTTP client with rate limiting
  oe-store/     PostgreSQL loader (the only crate with a database driver)
  oe-bulk/      the SEC's quarterly Insider Transactions Data Sets (history since 2006)
  oe-alerts/    alerts: receipts, watchlists, sent log, Discord and webhook delivery
  oe-cli/       the `oe` binary
events/specs/   written spec for every event rule version
fixtures/       filings the tests parse (synthetic now, real next)
migrations/     PostgreSQL schema for the live ledger
docker/         local PostgreSQL 16 for development (compose.yaml)
docs/           Field Guide to SEC filings, design decisions, methodology
```

## Principles

1. **Receipts on every claim.** Every alert links to the filings behind it.
2. **Point-in-time honesty.** Every fact has the date it was true and the date the public could know it. Amendments never rewrite history.
3. **Versioned rules.** Changing what an event means creates a new version (`@2`); old results stay reproducible.
4. **Public roles only.** The project covers insiders, large holders, fund managers and officials in their public or market roles, never private individuals.
5. **Polite to the SEC.** A declared User-Agent and under ten requests per second, enforced in code.

## Licence

Code: Apache-2.0 (see [LICENSE](LICENSE)). The licence for published data (CC-BY or ODbL) will be decided before the first data release; see [docs/adr/0003-data-licence.md](docs/adr/0003-data-licence.md).

This project is not affiliated with the SEC. Nothing it produces is investment advice.
