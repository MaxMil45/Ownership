# How to run the Ownership Engine

Everything below is PowerShell on Windows, run from the repository folder (`C:\Users\mmurp\Ownership`).

## 1. One-time setup

1. **Rust**: install from [rustup.rs](https://rustup.rs) (it asks for the Visual Studio C++ build tools if they are missing).
2. **Your SEC contact**: the SEC requires a name and email on every request (no account or key). Put one line in `.env` (git ignores it):

   ```powershell
   $env:OE_USER_AGENT = "Your Name your.email@example.com"
   ```

   `oe` reads the environment variable, not the file. Load it into the current terminal with:

   ```powershell
   Get-Content .env -Raw | Invoke-Expression
   ```

3. **Build** (release is faster for big folders):

   ```powershell
   cargo build --release -p oe-cli
   ```

   The program is then `.\target\release\oe.exe`. `.\target\release\oe.exe help` lists every command. You can also use `cargo run -p oe-cli -- <command>` instead.

4. **Optional, PostgreSQL** (only for the database commands): start Docker Desktop, then

   ```powershell
   docker-compose -f docker/compose.yaml up -d
   $env:DATABASE_URL = "postgres://oe:oe_dev_only@127.0.0.1:55432/ownership"
   ```

## 2. The quick way: one day, every source

```powershell
.\scripts\run-day.ps1 -Date 2026-11-18
```

It downloads that day's Form 4s, Form 144s, Schedule 13D/Gs, contested proxy headers and 10-Q/10-K Item 408 pages (the first 25 per form; add `-All` for everything, which can take many minutes on busy days), then prints every event found in everything downloaded so far. It loads `.env` itself if `OE_USER_AGENT` is not set.

## 3. Command by command

Set a short name for the program first: `$oe = ".\target\release\oe.exe"`.

| What | Download | Find events |
| --- | --- | --- |
| **Insider trades** (Form 4): several insiders buying in a short window | `& $oe daily-form4 2026-11-18 --all` (all companies, one day) or `& $oe recent-form4 AAPL --limit 50` (one company) | `& $oe cluster-buys data/form4/0000320193` (one company's folder) |
| **Planned insider sales** (Form 144) | `& $oe daily-form144 2026-11-18 --all` | `& $oe planned-sales data/form144` |
| **Large holders** (Schedule 13D/13G): new 13Ds, and holders switching from passive to active | `& $oe daily-13dg 2026-11-18 --all --include-amendments` | `& $oe activist-events data/sched13` |
| **Proxy contests** (PREC14A, DEFC14A, PREN14A, DFAN14A ...) | `& $oe daily-proxy 2026-05-18 --all` | `& $oe proxy-contests data/proxy` |
| **Insider plan terminations** (10-Q/10-K Item 408) | `& $oe daily-periodic 2026-11-05 --all` (several minutes on a reporting-deadline day) | `& $oe plan-terminations data/periodic` |
| **Live** Form 4 feed | `& $oe follow` (polls every 30 s until Ctrl-C; `--polls 2` to stop after two) | then `cluster-buys` as above |

Looking at a single file: `parse-form4`, `parse-form144`, `parse-13`, `parse-408` (each takes a file path).

Downloads go to `data/` (not committed), one folder per company CIK, each filing with a `.meta.txt` note of when the SEC accepted it. Commands never download the same filing twice. Weekends and holidays have no index ("no daily index for ...").

Useful options: `--limit <n>` / `--all`, `--out <folder>`, `--include-amendments`; for rules, `--min-value` (Form 144, dollars), `--min-percent` (13D), `--window` and `--min` (cluster buys).

## 4. The database

```powershell
& $oe db-migrate                      # create or update the tables (run after every upgrade)
& $oe db-load data/form4 --dry-run    # check what would load, no database needed
& $oe db-load data/form4              # load Form 4 filings (safe to repeat)
& $oe db-status                       # counts
```

Only Form 3/4/5 filings go into the database so far. After each batch, amendments are linked to the report they amend (ADR 0005); `db-status` shows how many were loaded, held back (not a full restatement), ambiguous, or are still waiting because the report they amend is not loaded yet. A waiting amendment is tried again on every load.

After upgrading to migration `0002_amendments`, load your folders once more (for example `& $oe db-load data/form4`): filings loaded earlier only then get their co-owners and amendment details. Nothing already stored changes. `docker-compose -f docker/compose.yaml down` stops it and keeps the data.

## 5. Checks

```powershell
cargo fmt --all
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

Database tests (need the Docker database running):

```powershell
$env:OE_TEST_DATABASE_URL = "postgres://oe:oe_dev_only@127.0.0.1:55432/ownership_test"
cargo test -p oe-store --test postgres -- --ignored --test-threads=1
```

Speed check of the cluster-buy rule on 1.44 million synthetic trades (about a third of a second):

```powershell
$env:OE_BENCH_ISSUERS = "3000"; cargo test --release -p oe-events --test bench_cluster_buy -- --ignored --nocapture
```

## 6. Reading the output

Every event line starts with `known_on`: the US Eastern date the SEC accepted the filing, the earliest the public could have known. The rules and their limits are written up in `events/specs/`; what each form contains and its traps are in `docs/field-guide/`. Nothing here is investment advice.
