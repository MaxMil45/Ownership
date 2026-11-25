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

At start it indexes every saved filing's acceptance date (about 80 seconds with the full history), and each alert pass then reads only the filings of the last 14 days; with `--db` the rules take older history from the database. `--until 2026-11-23T23:00:00Z` stops it at a time.

**How fast are the alerts?** Every delivery is logged in `data/alerts/delivered.tsv`. `& $oe alert-delays` reports, for one day, how long after SEC acceptance the filings were saved and the alerts went out (median, 90%, slowest, share within 5 minutes); `--since <UTC time>` leaves out the filings the first poll caught up on. To measure a whole trading day, start this on a weekday morning; it follows every feed until 18:00 New York time and writes the report to `data/alerts/trading_day_<date>.txt`:

```powershell
.\scripts\measure-trading-day.ps1
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

## 3d. Public officials: House members' trade reports (first step)

Members of the House report their trades in periodic transaction reports (PTRs), published by the House Clerk as PDFs. `oe house-ptr` downloads the Clerk's yearly index and every PTR it lists into `data/officials/house/<year>/`, each PDF with a `.meta.txt` (who, filing date, where from, when first seen, SHA-256). Reading the PDFs comes later (ADR 0010).

**The law limits what you may do with these reports** (Ethics in Government Act, section 105(c)): no commercial use other than news media, no credit ratings, no fundraising. They stay on your machine: never commit, publish or share them. `oe` refuses to download until you set `$env:OE_OFFICIALS_ACK = "1"` yourself after reading the limits it prints.

Locally (two requests a second; 2008 to now is about 9,000 requests, about 1 h 15 min):

```powershell
$env:OE_OFFICIALS_ACK = "1"
& $oe house-ptr 2008 --to 2026
```

Or on GitHub's machines with the workflow *Download House PTRs* (`.github/workflows/officials-house.yml`). Because anyone signed in to GitHub can read a public repository's downloads and logs, the workflow **encrypts** the result with a password only you hold, and its logs show counts only. Once, in the repository's Settings, Secrets and variables, Actions: add the secret `OE_OFFICIALS_KEY` (a long password; keep a copy, without it the download cannot be opened) and, on the Variables tab, the variable `OE_OFFICIALS_ACK` with the value `1`. Then Actions, *Download House PTRs*, Run workflow (`test`: three 2025 reports; `full`: 2008 to this year, about 1 h 15 min; `update`: last year and this year, about 10 minutes, to keep up afterwards). Download the `officials-house-encrypted` artifact and open it (an update unpacks over the same folder, adding the new reports):

```powershell
.\scripts\import-officials-house.ps1 -From "$HOME\Downloads\officials-house-encrypted.zip"
```

It asks for the password and unpacks the reports into `data\officials\house`.

**Members and the database.** The members of Congress come from the public-domain [congress-legislators](https://github.com/unitedstates/congress-legislators) data set (about 16 MB; only names, terms and committees are used, never birthdays, addresses or donor ids):

```powershell
New-Item -ItemType Directory -Force data\officials\legislators | Out-Null
foreach ($f in "legislators-current.json", "legislators-historical.json", "committees-current.json", "committee-membership-current.json") {
    Invoke-WebRequest "https://unitedstates.github.io/congress-legislators/$f" -OutFile "data\officials\legislators\$f"
}
& $oe db-migrate
& $oe officials-load        # members, committees, and every downloaded House report, each linked to its member
& $oe official pelosi       # terms, committees and the reports filed, with links
```

`officials-load` links each report to the member who filed it by seat (state and district), surname and dates, and holds back with a reason any report it cannot place for certain (on the 2013 to 2026 reports: 8,461 of 8,465 linked to 396 members). Committee membership is the data set's current one, stored with the date of the load, so committee history builds up from the first load. The reports' contents are not read yet (ADR 0010: the text PDFs forbid text extraction).

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

adds a chart of insider buying and selling over the whole history to the dashboard (about 5 seconds). Rows with impossible amounts as filed (a price over $2 million a share, over $2 billion in one row, over $10 billion for one company in a day) are left out of the dollar totals and counted on the page. `& $oe alerts --db` (and `& $oe follow --alert --db`) lets the cluster-buy rule recognise insiders who trade the same month every year, and runs `activist.passive_to_active@2`, which also finds a holder's earlier 13G in the ledger when the new 13D does not mention it (`& $oe activist-events --db` lists them, with the earlier 13G), and `activist.proxy_contest@2`, which alerts once per proxy contest and says when it started (`& $oe proxy-contests --db` lists every contest), and `insider.planned_sale@2`, which marks each planned sale as the seller's first in a year or routine (`& $oe planned-sales data/form144 --db`), and `insider.plan_terminated@2`, which says when an ended 10b5-1 plan was adopted and flags plans ended within the 90-day cooling-off (`& $oe plan-terminations data/periodic --db`).

### History of the other forms

Form 144, Schedule 13D/G, proxy contests and 10-Q/10-K plan terminations have no quarterly data sets; their history comes from the daily index, one day at a time:

```powershell
& $oe daily-proxy 2015-01-02 --to 2026-11-20 --all
& $oe daily-form144 2023-01-03 --to 2026-11-20 --all --include-amendments
& $oe daily-13dg 2024-12-23 --to 2026-11-20 --all --include-amendments
& $oe daily-periodic 2023-10-02 --to 2026-11-20 --all
& $oe db-load data
```

`daily-periodic` keeps each 10-Q/10-K that discloses a plan adoption or termination as a small excerpt (its insider-plan facts; `--full` keeps the 2 MB report). Folders downloaded before 2026-11-23 hold full reports with terminations only: `& $oe periodic-excerpt data/periodic` shrinks them offline (3.4 GB to 17 MB here), and `& $oe daily-periodic 2023-10-02 --to 2026-11-20 --all --recheck-none` looks again at the reports that were checked for terminations only (about 57,000, roughly 7 to 8 hours; `data/history-job/recheck_periodic.ps1` runs it resumably and loads the result).

**Without keeping a computer on:** the GitHub workflow *Recheck 10-Q/10-K history* (`.github/workflows/recheck-periodic.yml`) does the recheck on GitHub's machines, in three parts run one after another (about 10 hours in all). Once: add your SEC contact line as the repository secret `OE_USER_AGENT` (Settings, Secrets and variables, Actions). Then Actions, *Recheck 10-Q/10-K history*, Run workflow: `test` checks one day in about five minutes, `full` the whole history. When it has finished, download the `periodic-recheck` artifact from the run's page (it is kept 7 days) and load it:

```powershell
.\scripts\import-periodic-recheck.ps1 -From "$HOME\Downloads\periodic-recheck.zip"
```

It copies the excerpts into `data\periodic`, adds the run's check log, loads the database and reruns `plan-terminations --db`; running it twice adds nothing. Free for a public repository; a private one uses about 600 of its 2,000 free minutes a month. GitHub's machines share internet addresses with others, so run `test` first to see that the SEC answers them.

Run them one after another, never two at once: each `oe` keeps its own count of SEC requests. Measured here: about 3 h 40 min, 7 h, 2 h 30 min and 9 h 30 min, then 21 minutes to load about 205,000 filings (ADR 0009 has the details; structured data starts at these dates). If one stops (the laptop sleeps, the network drops), run it again from the last day it finished: saved filings are skipped. A day that keeps failing is listed at the end; run that day alone later.

### The web app, in one command

```powershell
.\scripts\start.ps1
```

starts Docker Desktop and the database if needed, builds `oe` and the web app when their sources changed (the first run installs the app's packages), applies any new migration, and runs `oe serve --open`: the app opens in your browser at <http://127.0.0.1:8730>, already signed in. Keep the window open while you use it; Ctrl+C stops it. `-Update` first runs `scripts\update.ps1` (every filing since the last run), `-NoBrowser` prints the link instead of opening it.

The app has an overview (headline numbers, events per month and insider buying and selling, over one year, five years or everything), every event with filters and SEC links, a page per company (its insiders and their roles, latest trades, buying and selling per month, its events), and the name lookup. Names and companies link to each other.

Signing in: `oe serve` prints a login link with a code made fresh for each run. Opening it stores the API token in an `HttpOnly`, `SameSite=Strict` cookie, which the page's scripts cannot read and other sites cannot send; the token never appears in a page. The cookie lasts 30 days and works across restarts (until `--new-token`); if the app says it is not signed in, open the latest link. To work on the app itself with live reloading, see `web/README.md`.

### Keeping it current (one command)

```powershell
.\scripts\update.ps1
```

catches up from the last time it finished to yesterday (US Eastern): it downloads every Form 3/4/5, Form 144, Schedule 13D/G, contested-proxy filing and 10-Q/10-K since then, loads only the files just written (`db-load --changed-within`), and rebuilds the events and the name index. Run it whenever you like, after a day or after a month; there is nothing to schedule. It remembers the last day it finished in `data\update\last-day.txt` and moves it forward only when every download worked (rerun to retry failed days; saved filings are skipped). The first run starts 3 days before the newest filing in the database. `-ThroughToday` also fetches today (the SEC's index for a day appears in the evening), `-From 2026-11-10` starts from a given day. It refuses to run beside another download or `oe follow` (they share the SEC's request limit). It does not fetch the House reports (those need your own `OE_OFFICIALS_ACK`) or a new quarterly Form 3/4/5 data set (`& $oe bulk-insider` when one appears).

The steps it takes, if you prefer to do them by hand:

The quarterly data sets appear some weeks after each quarter ends. Fill the days since the last one from the daily index, then load them:

```powershell
& $oe daily-form4 2026-10-01 --to 2026-11-20 --all --include-amendments
& $oe db-load data/form4
```

(about 1,000 Forms 3/4/5 a day, downloaded 6 at a time within the SEC limit of 8 requests a second). From then on, `& $oe follow --db` loads each new Form 3/4/5 into the database as it arrives; add `--alert` for alerts too. When the next quarterly data set appears, `& $oe bulk-insider` adds it; filings already loaded from their XML are kept as they are.

### Who is this name? (`oe lookup`)

```powershell
.\scripts\backfill-people.ps1      # once, after upgrading to migration 0007_people (about 30 minutes)
& $oe lookup elon musk
& $oe lookup elliott investment management
```

`oe lookup` answers what a name is in the ledger (ADR 0010): officer, director or 10% owner of which companies (with the title as filed and the dates), the latest open-market buys and sales, 13D/G holdings, contested-proxy filings, and for members of Congress their terms, committees and House reports, each fact with its SEC link. Every identifier is its own candidate (two people with the same name are listed apart, never merged); a single word with more than 10 candidates asks for a first name or initial. Word order, accents, punctuation, "Jr." and "Inc." do not matter.

It reads a name index built from the ledger by `& $oe people-index` (about a minute on the full history). The index is derived, not part of the ledger: rebuild it after loading new filings. Form 3/4/5 filings loaded before migration `0007_people` have no names until their source is loaded again, which is what `scripts\backfill-people.ps1` does (from `data\bulk` and `data\form4`, no SEC requests; safe to stop and rerun, `-From 2020q1` resumes from a quarter). Not in the index yet: Form 144 sellers and the names 10-Q/10-K reports give for plan adoptions.

### The local API (`oe serve`)

```powershell
& $oe serve
```

serves the ledger as JSON on this computer only (`http://127.0.0.1:8730/v1/`), for the browser extension and the local app. It prints a token (kept in `data\serve\token.txt`, the same every time; `--new-token` replaces it). Every call except `GET /v1/health` needs it as `Authorization: Bearer <token>`:

| Call | Answer |
| --- | --- |
| `GET /v1/health` | `{"ok":true,...}`, no token needed |
| `POST /v1/lookup` with `{"name": "elon musk"}` (`Content-Type: application/json`) | what `oe lookup` shows, as JSON: candidates with roles, trades, holdings, contest filings, House reports and every receipt (about 0.05 to 0.1 s) |
| `GET /v1/status` | what `oe db-status` counts (kept 10 minutes; about 3 s the first time) |
| `GET /v1/flow` | insider open-market buying and selling per day since 2006, as on the dashboard (kept 10 minutes; about 4 s the first time) |
| `GET /v1/events` | the events the rules found, newest first, each as the alert JSON (headline, details, people, receipts, amount, percent). Filters, all optional: `since` and `until` (`YYYY-MM-DD`), `rule` (`insider.cluster_buy`, or one version: `insider.cluster_buy@1`), `ticker`, `cik` (the company's), `min_amount` (dollars), `q` (words in the headline or company name), `limit` (1 to 500, default 50), `offset`. The answer has `total` (all matches) and `built_at` |
| `GET /v1/events/counts` | events per day and rule, for charts |
| `GET /v1/company?cik=1318605` | one company: name and ticker as filed, its latest 30 insiders with their roles, its latest 20 open-market trades with receipts, and its buying and selling per day |
| `GET /v1/companies?q=tes` | companies with events whose name starts with the text, or whose ticker is it (for a search box) |

The events come from `& $oe events-build`, which runs every rule (their ledger versions, as `oe alerts --db`) over the downloaded folders and stores the result in the database, replacing what was there (about 4 minutes on the full history). Run it after downloading new filings. Events whose filings name no ticker (Form 144, 13D/G, proxy and 10-Q/10-K events) get the company's latest ticker from its Form 3/4/5 filings, so `ticker=TSLA` finds them too. Cluster buys are found over every open-market purchase in the database since 2015 (about 363,000 purchases; 8,800 with impossible transaction dates, such as the year 0024 or 2033, are left out and counted), with the routine-trader filter reading the database's trades from 2011, so events before 2018 had less than the full three years to look back on. A cluster whose dollar total passes $1 billion is kept without a total, because the prices were typed as totals in the filing ("1,400,000 shares at $1400"). `--form4 <folder>` builds them from those files instead, as `oe alerts` does.

Try it from PowerShell:

```powershell
$t = (Get-Content data\serve\token.txt).Trim()
Invoke-RestMethod http://127.0.0.1:8730/v1/lookup -Method Post -Headers @{Authorization = "Bearer $t"} -ContentType application/json -Body '{"name": "elon musk"}'
```

It refuses requests for any other host name and any web page's origin; browser extensions are allowed, and `--allow-origin http://localhost:5173` allows a local development server. Names are sent in the request body, never in a URL, and the window logs only the method, path, status and time. Ctrl+C stops it.

After upgrading to migration `0002_amendments`, load your folders once more (for example `& $oe db-load data/form4`): filings loaded earlier only then get their co-owners and amendment details. Nothing already stored changes. `docker-compose -f docker/compose.yaml down` stops it and keeps the data.

### Keeping the database fast

`docker/compose.yaml` gives PostgreSQL memory for a 4 GB ledger (1 GB shared buffers, 64 MB per sort, SSD costs, parallel scans, no JIT) and a minute to shut down cleanly; run `docker-compose -f docker/compose.yaml up -d` once after upgrading to apply it (the data stays). `& $oe db-maintain` refreshes the statistics the query planner uses (`VACUUM (ANALYZE)`, about 10 seconds); `scripts\update.ps1` runs it after every load. `docker\bench.sql` times the queries the app uses:

```powershell
Get-Content docker\bench.sql | docker exec -i oe-postgres psql -U oe -d ownership
```

Measured 2026-11-25 on the full ledger, before and after: lookups 35-43 ms to 3-4 ms, a 13D/G holder by name 41 to 0.4 ms, event word search 128 to 1.4 ms, the joint-filing count in `/v1/status` 4.1 s to 0.06 s, the daily flow 2.2 to 1.3 s.

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
