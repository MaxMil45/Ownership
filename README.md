# Ownership Engine

An open-source engine that reads US ownership filings, works out who each investor really is, and sends evidence-backed alerts when something meaningful changes. It tells you what changed, how fresh the data is, and what history honestly says about it, with the filings behind every claim.

> **Status: Phase 1, week 1.** The Form 4 parser, the point-in-time ledger model and the first event rule (`insider.cluster_buy@1`) work and are tested against synthetic fixtures. Live EDGAR ingestion and alerts come next. See [ROADMAP.md](ROADMAP.md).

## Why it exists

Raw SEC data is free, and parsers already exist ([edgartools](https://github.com/dgunning/edgartools)). What nobody open-sources is the *interpretation*: which filer names are really the same investor, which holders are passive index funds, which ownership events matter, and the evidence for that. Commercial tools sell this as closed scores. This project publishes it openly, with sources and methodology you can check.

## What works today

| Piece | Crate | State |
| --- | --- | --- |
| Dates, CIKs, accession numbers, filing models | `oe-core` | Done, tested |
| Form 3/4/5 XML parser (no dependencies) | `oe-form4` | Done, tested on synthetic fixtures |
| Point-in-time ledger (in memory) + PostgreSQL schema | `oe-ledger`, `migrations/` | Model done; database wiring next |
| `insider.cluster_buy@1` rule | `oe-events` | Done, tested |
| Rate-limited EDGAR client (SEC fair-access rules) | `oe-edgar` | Written; first real use on your machine |
| `oe` command-line tool | `oe-cli` | `parse-form4`, `cluster-buys`, `fetch` |

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
4. **Download a real filing.** The SEC requires your name and email in every request:
   ```powershell
   $env:OE_USER_AGENT = "Your Name your.email@example.com"
   cargo run -p oe-cli -- fetch <url-of-a-form-4-xml-file> fixtures/form4/real/example.xml
   cargo run -p oe-cli -- parse-form4 fixtures/form4/real/example.xml
   ```

On macOS or Linux, install Rust with `curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh` and use `export OE_USER_AGENT="..."` instead.

## Repository layout

```text
crates/
  oe-core/      shared types: Date, Cik, Accession, OwnershipDocument
  oe-form4/     Form 3/4/5 parser and a small XML tokenizer
  oe-ledger/    append-only, point-in-time fact store
  oe-events/    versioned event rules (insider.cluster_buy@1)
  oe-edgar/     polite EDGAR HTTP client with rate limiting
  oe-cli/       the `oe` binary
events/specs/   written spec for every event rule version
fixtures/       filings the tests parse (synthetic now, real next)
migrations/     PostgreSQL schema for the live ledger
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
