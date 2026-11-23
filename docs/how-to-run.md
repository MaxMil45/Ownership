# How to run the Ownership Engine

Everything below is PowerShell on Windows, run from the repository folder (the folder that holds `Cargo.toml`).

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
| **Live** feeds (Form 4, Form 144, 13D/G, proxy contests) | `& $oe follow` (polls every 30 s until Ctrl-C; `--polls 2` to stop after two; `--forms 4,144` to pick feeds) | the commands above, or `& $oe follow --alert` to alert as filings arrive |
| **Everything** | any of the above | `& $oe alerts` (every rule, every folder; see section 3b) |

Looking at a single file: `parse-form4`, `parse-form144`, `parse-13`, `parse-408` (each takes a file path).

Downloads go to `data/` (not committed), one folder per company CIK, each filing with a `.meta.txt` note of when the SEC accepted it. Commands never download the same filing twice. Weekends and holidays have no index ("no daily index for ...").

Useful options: `--limit <n>` / `--all`, `--to <date>` (every weekday of a range; holidays are skipped), `--out <folder>`, `--include-amendments`; for rules, `--min-value` (Form 144, dollars), `--min-percent` (13D), `--window` and `--min` (cluster buys).

## 3b. Alerts

`& $oe alerts` runs every rule over the `data/` folders and shows each **new** event once, with links to the SEC filings it rests on. Run it again and it shows only what is new since (it keeps a log in `data/alerts/sent.tsv`). Only events from the last 3 days count (`--max-age 7` for a week); `--dry-run` shows what would happen and records nothing.

To get alerts in Discord, create a webhook in your Discord channel (Channel settings, Integrations, Webhooks) and put its URL in `.env`, next to your SEC contact. Treat the URL like a password:

```powershell
$env:OE_DISCORD_WEBHOOK_URL = "https://discord.com/api/webhooks/..."
```

`OE_ALERT_WEBHOOK_URL` does the same for any service that accepts JSON (Zapier, Make, n8n, your own server); each alert arrives as one JSON object. With a destination set, alerts go there instead of the screen.

To get only the companies and people you care about, make a text file, for example `watchlist.txt`:

```text
ticker AAPL
issuer 320193        # a company by CIK
insider 1494730      # a person or fund by CIK
name Elliott         # any holder or insider whose name contains this
rule activist.new_13d@1
```

and add `--watchlist watchlist.txt`. For alerts within about a minute or two of filing, leave this running. It follows Form 4s, Form 144s, 13D/13Gs and contested proxies; 10-Q/10-K plan terminations still come only from `daily-periodic` the next day:

```powershell
& $oe follow --alert --watchlist watchlist.txt
```

## 3c. The dashboard (charts)

```powershell
& $oe dashboard
```

writes `data/report/dashboard.html`. Open it in your browser (double-click it in Explorer, or `Start-Process data\report\dashboard.html`). It shows, for the last 7, 30 or 90 days or everything downloaded:

- headline numbers: events, insider buying and selling, planned sales, new 13Ds, proxy contests;
- events over time, coloured by type;
- insider buying above the line and selling below it, in dollars;
- the companies with the most events (click one to search for it) and the largest planned sales;
- the largest insider trades, and every event with links to its SEC filings.

The search box matches companies, tickers and people. Run the command again after downloading more to refresh the page. It works offline and sends nothing anywhere.

## 4. The database

```powershell
& $oe db-migrate                      # create or update the tables (run after every upgrade)
& $oe db-load data/form4 --dry-run    # check what would load, no database needed
& $oe db-load data/form4              # load Form 4 filings (safe to repeat)
& $oe db-status                       # counts
```

`db-load` takes any folder `oe` downloads into: Forms 3/4/5, Form 144, Schedule 13D/G, contested-proxy headers and 10-Q/10-K pages (each file is recognised by what it is), so `& $oe db-load data` loads everything; ADR 0009 lists what is stored (no street addresses, no CUSIPs). For Forms 3/4/5, after each batch, amendments are linked to the report they amend (ADR 0005); `db-status` shows how many were loaded, held back (not a full restatement), ambiguous, or are still waiting because the report they amend is not loaded yet. A waiting amendment is tried again on every load.

### History since 2015

```powershell
& $oe db-migrate
& $oe bulk-insider --from 2015q1
```

downloads the SEC's quarterly Insider Transactions Data Sets (one zip of about 9 to 15 MB per quarter, kept in `data/bulk/` so each is downloaded once) and loads every Form 3/4/5 into the database: 2015 to 2026 took 31 minutes here (2.4 million filings, about 2.5 GB of database). Run it again after a new quarter appears; it only adds what is new. `--dry-run` reads and counts without a database, `--offline` uses the zips already downloaded. These filings carry their filing date only (no time of day) and numbers to two decimals; ADR 0008 explains. Then

```powershell
& $oe dashboard --db
```

adds a chart of insider buying and selling over the whole history to the dashboard (about 5 seconds). Rows with impossible amounts as filed (a price over $2 million a share, over $2 billion in one row, over $10 billion for one company in a day) are left out of the dollar totals and counted on the page. `& $oe alerts --db` (and `& $oe follow --alert --db`) lets the cluster-buy rule recognise insiders who trade the same month every year.

### History of the other forms

Form 144, Schedule 13D/G, proxy contests and 10-Q/10-K plan terminations have no quarterly data sets; their history comes from the daily index, one day at a time:

```powershell
& $oe daily-proxy 2015-01-02 --to 2026-11-20 --all
& $oe daily-form144 2023-01-03 --to 2026-11-20 --all --include-amendments
& $oe daily-13dg 2024-12-23 --to 2026-11-20 --all --include-amendments
& $oe daily-periodic 2023-10-02 --to 2026-11-20 --all
& $oe db-load data
```

Run them one after another, never two at once: each `oe` keeps its own count of SEC requests. Measured here: about 3 h 40 min, 7 h, 2 h 30 min and 9 h 30 min, then 21 minutes to load about 205,000 filings (ADR 0009 has the details; structured data starts at these dates). If one stops (the laptop sleeps, the network drops), run it again from the last day it finished: saved filings are skipped. A day that keeps failing is listed at the end; run that day alone later.

### Keeping it current

The quarterly data sets appear some weeks after each quarter ends. Fill the days since the last one from the daily index, then load them:

```powershell
& $oe daily-form4 2026-10-01 --to 2026-11-20 --all --include-amendments
& $oe db-load data/form4
```

(about 1,000 Forms 3/4/5 a day, downloaded 6 at a time within the SEC limit of 8 requests a second). From then on, `& $oe follow --db` loads each new Form 3/4/5 into the database as it arrives; add `--alert` for alerts too. When the next quarterly data set appears, `& $oe bulk-insider` adds it; filings already loaded from their XML are kept as they are.

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
