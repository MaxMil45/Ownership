//! `oe`: command-line tool for the ownership engine.

mod alerts;
mod args;
mod bulkcmd;
mod dashboard;
#[cfg(feature = "db")]
mod dbcmd;
mod delays;
mod excerpt;
#[cfg(feature = "net")]
mod live;
mod load;
#[cfg(feature = "net")]
mod officials;
#[cfg(feature = "net")]
mod periodic;

use oe_core::FilingMeta;
use oe_events::{detect_cluster_buys, trades_from_document, ClusterBuyParams, InsiderTrade};
use oe_events::{detect_planned_sales, PlannedSaleParams};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
oe: ownership engine command-line tool

USAGE:
    oe parse-form4 <file.xml>
        Print every transaction in one Form 3/4/5 XML file.

    oe cluster-buys <folder> [--window <days>] [--min <insiders>] [--keep-routine]
        Parse every .xml file in a folder and report insider.cluster_buy@1 events.

    oe recent-form4 <TICKER or CIK> [--limit <n>] [--out <folder>] [--include-amendments]
        Download a company's most recent Form 4s from EDGAR (default 10) into
        data/form4/<CIK>/, each with a .meta.txt file recording when the SEC
        accepted it. Needs OE_USER_AGENT (see below).

    oe daily-form4 <YYYY-MM-DD> [--to <YYYY-MM-DD>] [--limit <n> | --all] [--out <folder>] [--include-amendments]
        Download Forms 3, 4 and 5 filed by ANY company on one day (or every weekday up
        to --to), found through the EDGAR daily index (default: the first 25 a day; a
        day has about 800 Form 4s). Saved under data/form4/<issuer CIK>/ like
        recent-form4. Needs OE_USER_AGENT. The other daily-* commands take --to too.

    oe follow [--forms 4,144,13,proxy] [--interval <seconds>] [--polls <n>] [--until <UTC time>]
              [--root <folder>] [--out <folder>] [--include-amendments] [--alert] [--watchlist <file>] [--db]
        Poll the EDGAR latest-filings feeds (default every 30 s, at least 10) and
        download each new Form 4, Form 144, Schedule 13D/13G and contested-proxy header
        as it appears, into <root>/form4, form144, sched13 and proxy (root: data; --out
        sets the Form 4 folder alone). --forms picks feeds. Runs until Ctrl-C, for
        --polls polls, or until the --until time. With --alert (implied by --watchlist),
        runs `oe alerts` after every poll on the filings of the last 14 days of the
        folders that got new filings (indexed once at start). --db loads each new filing
        into PostgreSQL (DATABASE_URL) after each poll, keeping the ledger current, and
        gives the rules its history. Needs OE_USER_AGENT.

    oe alert-delays [<YYYY-MM-DD>] [--since <UTC time>] [--data <folder>] [--state <sent.tsv>]
        How long after SEC acceptance filings were saved and alerts delivered on one US
        Eastern day (default today), from data/alerts/delivered.tsv and each filing's
        acceptance time. --since counts only filings accepted from then on (the follower's
        start; its first poll catches up on older filings).

    oe daily-form144 <YYYY-MM-DD> [--limit <n> | --all] [--out <folder>] [--include-amendments]
        Download Form 144 notices (planned insider sales) filed by any company on one
        day. Saved under data/form144/<issuer CIK>/. Needs OE_USER_AGENT.

    oe daily-periodic <YYYY-MM-DD> [--to <YYYY-MM-DD>] [--limit <n> | --all] [--forms 10-Q,10-K]
                      [--save-all] [--audit <n>] [--full] [--recheck-none] [--out <folder>]
        Find the 10-Q/10-K reports of one day (any company) whose Item 408 page says a director
        or officer adopted or terminated a Rule 10b5-1 plan, and save an excerpt of each (its
        insider-plan facts, a few KB; --full keeps the 2 MB report) under data/periodic/<CIK>/.
        Checks three small files per report and downloads the large one only when needed.
        --audit downloads <n> skipped reports too, to measure how often the cheap check is wrong.
        --recheck-none looks again at reports logged `none` (checked for terminations only).

    oe house-ptr <year> [--to <year>] [--out <folder>] [--limit <n>]
        Download the House Clerk's yearly disclosure index and every periodic transaction
        report (PTR, members' trades) it lists, as PDFs, into data/officials/house/<year>/
        with a .meta.txt each (ADR 0010). One request a second; saved reports are skipped.
        Use of these reports is limited by law: it refuses until you set OE_OFFICIALS_ACK=1.
        Never commit or publish them.

    oe periodic-excerpt [<folder>] [--dry-run]
        Replace each saved full 10-Q/10-K report (default folder: data/periodic) with its
        excerpt, offline. Each excerpt is checked to read exactly as the report before the
        report is replaced; the report's SHA-256 is kept in its .meta.txt.

    oe daily-13dg <YYYY-MM-DD> [--limit <n> | --all] [--include-amendments] [--out <folder>]
        Download Schedule 13D and 13G filings (large holders) filed by anyone on one day,
        saved under data/sched13/<issuer CIK>/. Needs OE_USER_AGENT.

    oe activist-events [<folder>] [--min-percent <n>] [--db]
        Run activist.new_13d@1 and activist.passive_to_active@1 over downloaded 13D/13G
        files (default folder: data/sched13) and list the events, newest first. --db runs
        activist.passive_to_active@2 instead, which also finds the holder's earlier 13Gs in
        the PostgreSQL ledger when the filer did not flag them (DATABASE_URL).

    oe daily-proxy <YYYY-MM-DD> [--limit <n> | --all] [--out <folder>]
        Download the SGML header of every contested-solicitation filing (PREC14A, DEFC14A,
        PREN14A, PRRN14A, DEFN14A, DFAN14A, ...) of one day, saved under data/proxy/<CIK>/.
        Needs OE_USER_AGENT.

    oe proxy-contests [<folder>] [--db]
        Run activist.proxy_contest@1 over downloaded headers (default folder: data/proxy).
        --db runs activist.proxy_contest@2 instead: one event per contest, using the
        PostgreSQL ledger's earlier filings to tell a new contest from a continuing one.

    oe alerts [--form4 <dir>] [--form144 <dir>] [--sched13 <dir>] [--proxy <dir>] [--periodic <dir>]
              [--watchlist <file>] [--max-age <days>] [--today <YYYY-MM-DD>] [--include-undated] [--db]
              [--state <file>] [--dry-run]
        Run every event rule over the downloaded folders (default: the data/ folders that
        exist; naming one means only those) and deliver each fresh event (default: known
        in the last 3 days) once. Destinations: OE_DISCORD_WEBHOOK_URL (a Discord webhook)
        and OE_ALERT_WEBHOOK_URL (any HTTPS endpoint, JSON POST); with neither set, new
        alerts are printed. Deliveries are logged in data/alerts/sent.tsv. A watchlist
        has lines like `ticker AAPL`, `issuer 320193`, `insider 1494730`, `name Elliott`,
        `rule activist.new_13d@1`. --db lets the cluster-buy rule's routine-trader filter
        (insiders who trade the same month every year) see the PostgreSQL ledger's history,
        and runs activist.passive_to_active@2 (earlier 13Gs from the ledger) and
        activist.proxy_contest@2 (one alert per contest), insider.planned_sale@2 (the
        seller's earlier Form 144s) and insider.plan_terminated@2 (how long the plan
        lasted) in place of their @1.

    oe bulk-insider [--from 2015q1] [--to <quarter>] [--dir data/bulk] [--dry-run] [--no-load] [--offline]
        History of Forms 3/4/5 from the SEC's quarterly Insider Transactions Data Sets: downloads
        each quarter's zip once (about 9 MB) into data/bulk/, then loads it into PostgreSQL
        (DATABASE_URL). Filing dates only, numbers to two decimals; see ADR 0008. Needs
        OE_USER_AGENT unless --offline.

    oe dashboard [--db] [--out <file.html>] [--form4 <dir>] [--form144 <dir>] [--sched13 <dir>] [--proxy <dir>] [--periodic <dir>]
        Write one HTML page (default data/report/dashboard.html) with charts and a searchable
        list of every event in the downloaded folders, each with links to its SEC filings.
        Open it in a browser; it works offline and sends nothing anywhere. --db adds a chart of
        insider buying and selling over the whole PostgreSQL ledger (DATABASE_URL).

    oe parse-13 <primary_doc.xml>
        Print the key facts of one Schedule 13D/13G XML file.

    oe parse-408 <report.htm>
        Print the insider trading arrangements (Item 408) tagged in one 10-Q or 10-K.

    oe plan-terminations <folder> [--db]
        Run insider.plan_terminated@1 over downloaded reports (default folder: data/periodic).
        --db runs insider.plan_terminated@2 instead: each termination also says when the plan
        was adopted (tagged, or from an earlier report in the ledger) and how long it lasted.

    oe parse-form144 <file.xml>
        Print the key facts of one Form 144 XML file.

    oe planned-sales <folder> [--min-value <dollars>] [--db]
        Run insider.planned_sale@1 over downloaded Form 144 files (default folder:
        data/form144) and list the events, newest first. Default minimum 1,000,000.
        --db runs insider.planned_sale@2 instead: each sale also says how many Form 144s
        the seller filed for the company in the year before (first, or routine).

    oe db-migrate
        Create or update the PostgreSQL ledger tables (migrations/). Needs DATABASE_URL.

    oe db-load <folder> [--batch <n>] [--dry-run]
        Load downloaded filings (every .xml with its .meta.txt, e.g. data/form4) into
        PostgreSQL. Safe to repeat: filings already stored are left untouched.
        --dry-run checks and counts everything without a database.
        Amendment (4/A, 5/A) filings are stored, but their transaction rows are
        held back until amendments can be linked to what they replace.

    oe db-status
        Counts what the ledger holds. Needs DATABASE_URL.

    oe fetch <url> <output-file>
        Download one EDGAR document politely.

    oe help

Commands that use the network need OE_USER_AGENT set to your name and email,
e.g. \"Jane Doe jane.doe@example.com\", as the SEC requires.
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("parse-form4") => match args.get(1) {
            Some(file) => parse_form4(Path::new(file)),
            None => Err("parse-form4 needs a file".into()),
        },
        Some("parse-13") => match args.get(1) {
            Some(file) => parse_13_file(Path::new(file)),
            None => Err("parse-13 needs a file (a Schedule 13D/13G primary_doc.xml)".into()),
        },
        Some("parse-408") => match args.get(1) {
            Some(file) => parse_408_file(Path::new(file)),
            None => Err("parse-408 needs a file (a 10-Q or 10-K .htm)".into()),
        },
        Some("parse-form144") => match args.get(1) {
            Some(file) => parse_form144_file(Path::new(file)),
            None => Err("parse-form144 needs a file".into()),
        },
        Some("activist-events") => activist_events(&args[1..]),
        Some("proxy-contests") => proxy_contests(&args[1..]),
        Some("plan-terminations") => plan_terminations(&args[1..]),
        Some("periodic-excerpt") => excerpt::periodic_excerpt(&args[1..]),
        Some("alert-delays") => delays::alert_delays(&args[1..]),
        Some("planned-sales") => planned_sales(&args[1..]),
        Some("cluster-buys") => cluster_buys(&args[1..]),
        Some("alerts") => alerts::alerts(&args[1..]),
        Some("dashboard") => dashboard::dashboard(&args[1..]),
        Some("bulk-insider") => bulkcmd::bulk_insider(&args[1..]),
        Some("fetch") => match (args.get(1), args.get(2)) {
            (Some(url), Some(out)) => fetch(url, Path::new(out)),
            _ => Err("fetch needs a URL and an output file".into()),
        },
        Some("recent-form4") => recent_form4(&args[1..]),
        #[cfg(feature = "net")]
        Some("daily-form4") => live::daily_form4(&args[1..]),
        #[cfg(feature = "net")]
        Some("daily-13dg") => live::daily_schedule13(&args[1..]),
        #[cfg(feature = "net")]
        Some("daily-proxy") => live::daily_proxy(&args[1..]),
        #[cfg(feature = "net")]
        Some("daily-periodic") => periodic::daily_periodic(&args[1..]),
        #[cfg(feature = "net")]
        Some("house-ptr") => officials::house_ptr(&args[1..]),
        #[cfg(feature = "net")]
        Some("daily-form144") => live::daily_form144(&args[1..]),
        #[cfg(feature = "net")]
        Some("follow") => live::follow(&args[1..]),
        #[cfg(not(feature = "net"))]
        Some("daily-form4")
        | Some("daily-form144")
        | Some("daily-periodic")
        | Some("house-ptr")
        | Some("daily-13dg")
        | Some("daily-proxy")
        | Some("follow") => {
            Err("this build has no network support; rebuild with the default `net` feature".into())
        }
        #[cfg(feature = "db")]
        Some("db-migrate") => dbcmd::migrate(&args[1..]),
        #[cfg(feature = "db")]
        Some("db-load") => dbcmd::load(&args[1..]),
        #[cfg(feature = "db")]
        Some("db-status") => dbcmd::status(&args[1..]),
        #[cfg(not(feature = "db"))]
        Some("db-migrate") | Some("db-load") | Some("db-status") => {
            Err("this build has no database support; rebuild with the default `db` feature".into())
        }
        Some("help") | Some("--help") | Some("-h") | None => {
            print!("{USAGE}");
            Ok(())
        }
        Some(other) => Err(format!("unknown command {other:?}\n\n{USAGE}")),
    };
    match result {
        Ok(()) => ExitCode::SUCCESS,
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

/// Folder (under a download root) for filings whose XML the parser rejected.
/// Searches skip it, so a bad file does not stop a whole run.
pub(crate) const UNPARSED_DIR: &str = "unparsed";

/// Every `.xml` under `root`, sorted by path, skipping the `unparsed` folder.
pub(crate) fn find_xml_files(root: &Path) -> Result<Vec<PathBuf>, String> {
    find_files(root, "xml")
}

/// Every file with the extension ``ext`` (any letter case) under ``root``,
/// sorted by path, skipping the ``unparsed`` folder.
pub(crate) fn find_files(root: &Path, ext: &str) -> Result<Vec<PathBuf>, String> {
    fn walk(dir: &Path, ext: &str, out: &mut Vec<PathBuf>) -> Result<(), String> {
        for entry in std::fs::read_dir(dir).map_err(|e| format!("{}: {e}", dir.display()))? {
            let path = entry.map_err(|e| e.to_string())?.path();
            if path.is_dir() {
                if path.file_name().is_some_and(|n| n == UNPARSED_DIR) {
                    continue;
                }
                walk(&path, ext, out)?;
            } else if path
                .extension()
                .is_some_and(|x| x.eq_ignore_ascii_case(ext))
            {
                out.push(path);
            }
        }
        Ok(())
    }
    let mut files = Vec::new();
    walk(root, ext, &mut files)?;
    files.sort();
    Ok(files)
}
fn read_document(path: &Path) -> Result<oe_core::OwnershipDocument, String> {
    let xml = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    oe_form4::parse(&xml).map_err(|e| format!("{}: {e}", path.display()))
}

fn opt(v: Option<f64>) -> String {
    v.map_or_else(|| "-".to_string(), |x| format!("{x}"))
}

/// Path of the metadata sidecar for a filing: `0001-26-000001.xml` →
/// `0001-26-000001.meta.txt`.
fn meta_path(xml_path: &Path) -> PathBuf {
    xml_path.with_extension("meta.txt")
}

/// Renders a sidecar: one `key=value` per line.
#[cfg_attr(not(feature = "net"), allow(dead_code))]
fn render_meta(pairs: &[(&str, &str)]) -> String {
    pairs.iter().map(|(k, v)| format!("{k}={v}\n")).collect()
}

/// Reads the sidecar next to an XML file, if there is one.
fn read_meta(xml_path: &Path) -> FilingMeta {
    let mut meta = FilingMeta {
        source: Some(xml_path.display().to_string()),
        ..FilingMeta::default()
    };
    let Ok(text) = std::fs::read_to_string(meta_path(xml_path)) else {
        return meta;
    };
    let mut stored_known_on = None;
    let mut accepted_eastern = None;
    for (key, value) in oe_core::text::key_values(&text) {
        match key {
            "accession" => meta.accession = value.parse().ok(),
            "known_on" => stored_known_on = value.parse().ok(),
            // The raw SEC timestamp is UTC. Converting it here, rather than
            // trusting the stored `known_on`, corrects sidecars written
            // before the conversion existed (they were a day late for
            // filings accepted after about 8 pm in New York).
            "accepted" => accepted_eastern = oe_core::us_eastern_date(value),
            "url" => meta.source = Some(value.to_string()),
            _ => {}
        }
    }
    meta.known_on = accepted_eastern.or(stored_known_on);
    meta
}

/// The first `max` filings that are Form 4 (and 4/A if asked), in list order.
#[cfg_attr(not(feature = "net"), allow(dead_code))]
fn select_form4s(
    filings: &[oe_edgar::FilingRef],
    include_amendments: bool,
    max: usize,
) -> Vec<oe_edgar::FilingRef> {
    filings
        .iter()
        .filter(|f| f.form == "4" || (include_amendments && f.form == "4/A"))
        .take(max)
        .cloned()
        .collect()
}

/// Older history pages, newest first. The SEC's own order is not relied on.
#[cfg_attr(not(feature = "net"), allow(dead_code))]
fn newest_pages_first(files: &[oe_edgar::HistoryFileRef]) -> Vec<&oe_edgar::HistoryFileRef> {
    let mut pages: Vec<_> = files.iter().collect();
    pages.sort_by(|a, b| b.filing_to.cmp(&a.filing_to));
    pages
}

fn parse_form4(path: &Path) -> Result<(), String> {
    let doc = read_document(path)?;
    let owner = &doc.owners[0];
    println!(
        "Form {} | issuer {} ({}) {} | owner {} {}",
        doc.document_type,
        doc.issuer.name,
        doc.issuer.ticker.as_deref().unwrap_or("no ticker"),
        doc.issuer.cik,
        owner.name,
        owner.cik
    );
    if doc.owners.len() > 1 {
        println!("joint filing with {} owners", doc.owners.len());
    }
    match doc.aff_10b5_1 {
        Some(true) => println!("10b5-1 plan box: ticked"),
        Some(false) => println!("10b5-1 plan box: not ticked"),
        None => println!("10b5-1 plan box: not present (older filing)"),
    }
    println!("date\tcode\tderivative\tshares\tprice\tshares_after\tsecurity");
    for t in &doc.transactions {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            t.date,
            t.code.as_str(),
            t.is_derivative,
            opt(t.shares),
            opt(t.price_per_share),
            opt(t.shares_owned_after),
            t.security_title
        );
    }
    Ok(())
}

fn parse_13_file(path: &Path) -> Result<(), String> {
    let xml = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let s = oe_sched13::parse(&xml).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "{} | issuer {} {} | class {} | event {}{}",
        s.submission_type,
        s.issuer_name,
        s.issuer_cik,
        s.class_title,
        s.event_date.map_or("?".to_string(), |d| d.to_string()),
        s.amendment_no
            .map_or(String::new(), |n| format!(" | amendment {n}"))
    );
    if let Some(prev) = &s.previous_accession {
        println!("amends {prev}");
    }
    println!(
        "CUSIP: {}",
        if s.cusips.is_empty() {
            "-".to_string()
        } else {
            s.cusips.join(", ")
        }
    );
    if let Some(flag) = s.previously_filed_13g {
        println!(
            "previously filed a 13G for this holding: {}",
            if flag { "yes" } else { "no" }
        );
    }
    if !s.rules.is_empty() {
        println!("filed under: {}", s.rules.join("; "));
    }
    println!("person	cik	type	group	shares	percent	sole_vote	shared_vote");
    for p in &s.persons {
        println!(
            "{}	{}	{}	{}	{}	{}	{}	{}",
            p.name,
            p.cik.map_or("-".to_string(), |c| c.to_string()),
            p.person_type.as_deref().unwrap_or("-"),
            p.member_of_group.as_deref().unwrap_or("-"),
            opt(p.aggregate_shares),
            opt(p.percent_of_class),
            opt(p.sole_voting),
            opt(p.shared_voting)
        );
    }
    if let Some(purpose) = &s.purpose_of_transaction {
        let one_line: String = purpose.split_whitespace().collect::<Vec<_>>().join(" ");
        println!(
            "Item 4 (purpose): {}",
            one_line.chars().take(400).collect::<String>()
        );
    }
    Ok(())
}

fn parse_408_file(path: &Path) -> Result<(), String> {
    let html = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let doc = oe_item408::extract(&html);
    println!(
        "{} | {} | CIK {} | period ending {}",
        doc.document_type.as_deref().unwrap_or("?"),
        doc.registrant.as_deref().unwrap_or("?"),
        doc.cik.map_or("?".to_string(), |c| c.to_string()),
        doc.period_end.map_or("?".to_string(), |d| d.to_string())
    );
    if !doc.uses_ecd_tags {
        println!("no ecd (insider trading arrangement) tags in this report");
        return Ok(());
    }
    let f = &doc.company_flags;
    let show = |v: Option<bool>| v.map_or("-", |b| if b { "yes" } else { "no" });
    println!(
        "company-level statements: 10b5-1 adopted {}, terminated {}; other arrangements adopted {}, terminated {}",
        show(f.rule_10b5_1_adopted),
        show(f.rule_10b5_1_terminated),
        show(f.non_rule_adopted),
        show(f.non_rule_terminated)
    );
    println!("{} trading arrangement(s)", doc.arrangements.len());
    for a in &doc.arrangements {
        let status = match (a.rule_10b5_1_adopted, a.rule_10b5_1_terminated) {
            (true, false) => "ADOPTED",
            (false, true) => "TERMINATED",
            (true, true) => "ADOPTED+TERMINATED",
            _ if a.non_rule_adopted => "non-10b5-1 adopted",
            _ if a.non_rule_terminated => "non-10b5-1 terminated",
            _ => "no flag",
        };
        println!(
            "  {status:<10} {} | {} | adopted {} | terminated {} | expires {} | shares {}",
            a.person.as_deref().unwrap_or("?"),
            a.title.as_deref().unwrap_or("?"),
            a.adoption_date.map_or("-".to_string(), |d| d.to_string()),
            a.termination_date
                .map_or("-".to_string(), |d| d.to_string()),
            a.expiration_date.map_or("-".to_string(), |d| d.to_string()),
            opt(a.securities_amount)
        );
    }
    Ok(())
}

fn parse_form144_file(path: &Path) -> Result<(), String> {
    let xml = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let f = oe_form144::parse(&xml).map_err(|e| format!("{}: {e}", path.display()))?;
    println!(
        "Form {} | issuer {} {} | seller {} ({})",
        f.submission_type,
        f.issuer_name,
        f.issuer_cik,
        f.seller_name,
        f.relationships.join(", ")
    );
    println!(
        "to sell: {} units, ${} | plan adopted: {}",
        opt(f.total_units()),
        opt(f.total_market_value()),
        if f.plan_adoption_dates.is_empty() {
            "no plan listed".to_string()
        } else {
            f.plan_adoption_dates
                .iter()
                .map(|d| d.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        }
    );
    println!("class\tunits\tvalue\tapprox_sale_date\tof_outstanding");
    for s in &f.securities {
        println!(
            "{}\t{}\t{}\t{}\t{}",
            s.class_title,
            opt(s.units),
            opt(s.aggregate_market_value),
            s.approx_sale_date
                .map_or("-".to_string(), |d| d.to_string()),
            opt(s.units_outstanding)
        );
    }
    println!(
        "sold in the past 3 months: {} units in {} sale(s)",
        f.units_sold_past_3_months(),
        f.sold_past_3_months.len()
    );
    Ok(())
}

fn activist_events(args: &[String]) -> Result<(), String> {
    let mut folder: Option<PathBuf> = None;
    let mut params = oe_events::ActivistParams::default();
    let mut with_db = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--min-percent" => {
                params.min_percent =
                    args::non_negative(&mut it, "--min-percent", "a number such as 5")?
            }
            "--db" => with_db = true,
            other if folder.is_none() => folder = Some(PathBuf::from(other)),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    let folder = folder.unwrap_or_else(|| PathBuf::from("data/sched13"));

    let loaded = load::schedule13(&folder, None)?;
    let (files, failed) = (loaded.files, loaded.failed);
    let (mut events, pta_id) = if with_db {
        let earlier = alerts::ledger_13gs(&loaded.items)?;
        println!("{} earlier 13G(s) from the database", earlier.len());
        (
            oe_events::detect_activist_events_with_ledger(&loaded.items, &params, &earlier),
            oe_events::activist::PASSIVE_TO_ACTIVE_V2_RULE_ID,
        )
    } else {
        (
            oe_events::detect_activist_events(&loaded.items, &params),
            oe_events::activist::PASSIVE_TO_ACTIVE_RULE_ID,
        )
    };
    // Newest first; within a day, the larger holding first.
    events.sort_by(|a, b| {
        b.known_on.cmp(&a.known_on).then(
            b.largest_percent
                .partial_cmp(&a.largest_percent)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    let count = |id: &str| events.iter().filter(|e| e.rule_id == id).count();
    println!(
        "{files} file(s), {failed} unreadable: {} event(s) from activist.new_13d@1 (at least {}%), {} from {pta_id}",
        count(oe_events::activist::NEW_13D_RULE_ID),
        params.min_percent,
        count(pta_id)
    );
    println!("known_on\trule\tissuer\tholders\tpercent\tshares\tgroup\tevent_date\tearlier_13g");
    for e in &events {
        let holders = e.holders.join("; ");
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
            e.known_on.map_or("?".to_string(), |d| d.to_string()),
            e.rule_id.trim_start_matches("activist."),
            e.issuer_name,
            holders.chars().take(70).collect::<String>(),
            e.largest_percent
                .map_or("-".to_string(), |p| format!("{p:.1}")),
            opt(e.largest_shares),
            if e.is_group { "group" } else { "-" },
            e.event_date.map_or("-".to_string(), |d| d.to_string()),
            e.earlier_13g
                .as_ref()
                .map_or("-".to_string(), |(a, d, _)| format!("{a} ({d})"))
        );
    }
    Ok(())
}
fn proxy_contests(args: &[String]) -> Result<(), String> {
    let with_db = args.iter().any(|a| a == "--db");
    let rest: Vec<&String> = args.iter().filter(|a| *a != "--db").collect();
    let folder = match rest.as_slice() {
        [] => PathBuf::from("data/proxy"),
        [one] => PathBuf::from(one),
        _ => return Err("proxy-contests takes one folder, for example data/proxy".into()),
    };
    let loaded = load::contest_filings(&folder, None)?;
    let (mut events, rule) = if with_db {
        let earlier = alerts::ledger_contests(&loaded.items)?;
        println!("{} contest filing(s) from the database", earlier.len());
        (
            oe_events::detect_proxy_contests_with_ledger(&loaded.items, &earlier),
            oe_events::proxy_contest::RULE_ID_V2,
        )
    } else {
        (
            oe_events::detect_proxy_contests(&loaded.items),
            oe_events::proxy_contest::RULE_ID,
        )
    };
    events.sort_by_key(|e| std::cmp::Reverse(e.last_known_on));
    println!(
        "{} header(s), {} unreadable or without a subject: {} event(s) from {rule}",
        loaded.files,
        loaded.failed,
        events.len()
    );
    println!(
        "last_known_on\tissuer\tchallengers\tfilings\tchallenger_proxy\tcompany_contested_proxy\tcontest_start"
    );
    for e in &events {
        let challengers: Vec<&str> = e.challengers.iter().map(|p| p.name.as_str()).collect();
        let forms: Vec<&str> = e.filings.iter().map(|f| f.form.as_str()).collect();
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}",
            e.last_known_on.map_or("?".to_string(), |d| d.to_string()),
            e.issuer_name,
            if challengers.is_empty() {
                "-".to_string()
            } else {
                challengers.join("; ")
            },
            forms.join(","),
            if e.challenger_proxy_statement {
                "yes"
            } else {
                "-"
            },
            if e.company_contested_proxy {
                "yes"
            } else {
                "-"
            },
            e.contest_start.map_or("-".to_string(), |d| d.to_string())
        );
    }
    Ok(())
}
fn plan_terminations(args: &[String]) -> Result<(), String> {
    let with_db = args.iter().any(|a| a == "--db");
    let rest: Vec<&String> = args.iter().filter(|a| *a != "--db").collect();
    let folder = match rest.as_slice() {
        [] => PathBuf::from("data/periodic"),
        [one] => PathBuf::from(one),
        _ => return Err("plan-terminations takes one folder, for example data/periodic".into()),
    };
    let (loaded, with_tags) = load::plan_termination_events(&folder, with_db)?;
    let mut events = loaded.items;
    let rule = if with_db {
        oe_events::plan_terminated::RULE_ID_V2
    } else {
        oe_events::plan_terminated::RULE_ID
    };

    // Newest first.
    events.sort_by_key(|e| std::cmp::Reverse(e.known_on));
    println!(
        "{} report(s), {} with insider-trading tags, {} unreadable, {} event(s) from {rule}",
        loaded.files,
        with_tags,
        loaded.failed,
        events.len()
    );
    println!(
        "known_on\tissuer\tperson\ttitle\tterminated\tperiod_end\treplaced\tform\tadopted\tdays"
    );
    for e in &events {
        let (adopted, days) = match &e.plan_life {
            None => ("-".to_string(), "-".to_string()),
            Some(l) => (
                format!(
                    "{}{}",
                    l.adopted_on,
                    l.adoption_report
                        .as_ref()
                        .map_or(String::new(), |a| format!(" (report {a})"))
                ),
                match l.days {
                    None => "adopted after the end?".to_string(),
                    Some(d) => format!(
                        "{}{d}{}",
                        if l.days_at_most { "at most " } else { "" },
                        if l.within_cooling_off {
                            ", within cooling-off"
                        } else {
                            ""
                        }
                    ),
                },
            ),
        };
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\t{adopted}\t{days}",
            e.known_on.map_or("?".to_string(), |d| d.to_string()),
            e.issuer_name.as_deref().unwrap_or("?"),
            e.person.as_deref().unwrap_or("?"),
            e.title.as_deref().unwrap_or("?"),
            match (e.termination_date, e.rejected_termination_date) {
                (Some(d), _) => d.to_string(),
                (None, Some(bad)) => format!("tag {bad} rejected (after the period)"),
                (None, None) => "not tagged".to_string(),
            },
            e.period_end.map_or("-".to_string(), |d| d.to_string()),
            if e.replaced_in_filing {
                "new plan adopted"
            } else {
                "no new plan"
            },
            e.form.as_deref().unwrap_or("?")
        );
    }
    Ok(())
}
fn planned_sales(args: &[String]) -> Result<(), String> {
    let mut folder: Option<PathBuf> = None;
    let mut params = PlannedSaleParams::default();
    let mut with_db = false;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--min-value" => {
                params.min_value =
                    args::non_negative(&mut it, "--min-value", "a number of dollars")?
            }
            "--db" => with_db = true,
            other if folder.is_none() => folder = Some(PathBuf::from(other)),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    let folder = folder.ok_or("planned-sales needs a folder, for example data/form144")?;

    let loaded = load::form144(&folder, None)?;
    let (files, failed) = (loaded.files, loaded.failed);
    let (mut events, rule) = if with_db {
        let earlier = alerts::ledger_notices(&loaded.items)?;
        println!("{} earlier notice(s) from the database", earlier.len());
        (
            oe_events::detect_planned_sales_with_ledger(&loaded.items, &params, &earlier),
            oe_events::planned_sale::RULE_ID_V2,
        )
    } else {
        (
            detect_planned_sales(&loaded.items, &params),
            oe_events::planned_sale::RULE_ID,
        )
    };
    // Newest first; within a day, the largest sale first.
    events.sort_by(|a, b| {
        b.known_on.cmp(&a.known_on).then(
            b.aggregate_market_value
                .partial_cmp(&a.aggregate_market_value)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    println!(
        "{files} file(s), {failed} not Form 144 XML, {} event(s) from {rule} (at least ${:.0})",
        events.len(),
        params.min_value
    );
    println!("known_on	issuer	seller	units	value	%outstanding	planned_for	plan	past_year");
    for e in &events {
        let past = match &e.history {
            None => "-".to_string(),
            Some(h) if h.notices_past_year == 0 => "first".to_string(),
            Some(h) => format!(
                "{} in {} month(s){}",
                h.notices_past_year,
                h.months_past_year,
                if h.routine { ", routine" } else { "" }
            ),
        };
        println!(
            "{}	{}	{}	{}	{:.0}	{}	{}	{}	{past}",
            e.known_on.map_or("?".to_string(), |d| d.to_string()),
            e.issuer_name,
            e.seller,
            opt(e.units),
            e.aggregate_market_value,
            e.percent_of_outstanding
                .map_or("-".to_string(), |p| format!("{p:.3}")),
            e.approx_sale_date
                .map_or("-".to_string(), |d| d.to_string()),
            e.plan_adopted_on
                .map_or("no plan".to_string(), |d| format!("plan {d}"))
        );
    }
    Ok(())
}

fn cluster_buys(args: &[String]) -> Result<(), String> {
    let mut folder: Option<PathBuf> = None;
    let mut params = ClusterBuyParams::default();
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--window" => {
                params.window_days = args::value(&mut it, "--window", "a number of days", |_| true)?
            }
            "--min" => {
                params.min_insiders =
                    args::value(&mut it, "--min", "a number of insiders", |_| true)?
            }
            "--keep-routine" => params.routine_lookback_years = 0,
            other if folder.is_none() => folder = Some(PathBuf::from(other)),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    let folder = folder.ok_or("cluster-buys needs a folder")?;

    let mut paths: Vec<PathBuf> = std::fs::read_dir(&folder)
        .map_err(|e| format!("{}: {e}", folder.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("xml")))
        .collect();
    paths.sort();

    let mut trades: Vec<InsiderTrade> = Vec::new();
    for path in &paths {
        let doc = read_document(path)?;
        trades.extend(trades_from_document(&doc, &read_meta(path)));
    }

    let events = detect_cluster_buys(&trades, &trades, &params);
    println!(
        "{} files, {} transactions, {} event(s) (window {} days, at least {} insiders)",
        paths.len(),
        trades.len(),
        events.len(),
        params.window_days,
        params.min_insiders
    );
    for e in &events {
        println!(
            "\n{} | {} {} | {} to {} | {} insiders | ${:.0} bought{}",
            e.rule_id,
            e.ticker.as_deref().unwrap_or("?"),
            e.issuer,
            e.window_start,
            e.window_end,
            e.insiders.len(),
            e.total_value,
            if e.trades_without_value > 0 {
                format!(" (+{} buys without a price)", e.trades_without_value)
            } else {
                String::new()
            }
        );
        match e.known_on {
            Some(d) => println!("  knowable on {d}"),
            None => println!(
                "  knowable on: unknown (some files have no .meta.txt; download with recent-form4)"
            ),
        }
        for t in &e.trades {
            println!(
                "  {}  {:<28} {:>10} @ {:<8} {}",
                t.date,
                t.owner_name,
                opt(t.shares),
                opt(t.price_per_share),
                t.officer_title
                    .as_deref()
                    .unwrap_or(if t.is_director { "Director" } else { "" })
            );
        }
    }
    Ok(())
}

#[cfg(feature = "net")]
fn fetch(url: &str, out: &Path) -> Result<(), String> {
    let client = oe_edgar::EdgarClient::from_env().map_err(|e| e.to_string())?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    // Bytes as served: a zip or a PDF must not go through text decoding.
    let n = client.save_to_file(url, out).map_err(|e| e.to_string())?;
    println!("saved {n} bytes to {}", out.display());
    Ok(())
}

#[cfg(not(feature = "net"))]
fn fetch(_url: &str, _out: &Path) -> Result<(), String> {
    Err("this build has no network support; rebuild with the default `net` feature".into())
}

struct RecentArgs {
    company: String,
    limit: usize,
    out: Option<PathBuf>,
    include_amendments: bool,
}

fn parse_recent_args(args: &[String]) -> Result<RecentArgs, String> {
    let mut parsed = RecentArgs {
        company: String::new(),
        limit: 10,
        out: None,
        include_amendments: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--limit" => parsed.limit = args::positive(&mut it, "--limit")?,
            "--out" => parsed.out = Some(args::folder(&mut it, "--out")?),
            "--include-amendments" => parsed.include_amendments = true,
            other if parsed.company.is_empty() => parsed.company = other.to_string(),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    if parsed.company.is_empty() {
        return Err("recent-form4 needs a ticker (e.g. AAPL) or a CIK".into());
    }
    Ok(parsed)
}

#[cfg(feature = "net")]
fn recent_form4(args: &[String]) -> Result<(), String> {
    use oe_edgar::{
        find_ticker, history_page_url, parse_company_tickers, parse_history_page,
        parse_submissions, submissions_url,
    };

    let args = parse_recent_args(args)?;
    let client = oe_edgar::EdgarClient::from_env().map_err(|e| e.to_string())?;

    let cik: u64 = if args.company.bytes().all(|b| b.is_ascii_digit()) {
        args.company.parse().map_err(|_| "invalid CIK")?
    } else {
        let list = client
            .get_text(oe_edgar::COMPANY_TICKERS_URL)
            .map_err(|e| e.to_string())?;
        let entries = parse_company_tickers(&list).map_err(|e| e.to_string())?;
        let entry = find_ticker(&entries, &args.company)
            .ok_or_else(|| format!("ticker {:?} not found in the SEC list", args.company))?;
        println!("{} is {} (CIK {})", entry.ticker, entry.title, entry.cik);
        entry.cik
    };

    let body = client
        .get_text(&submissions_url(cik))
        .map_err(|e| e.to_string())?;
    let subs = parse_submissions(&body).map_err(|e| e.to_string())?;
    let mut wanted = select_form4s(&subs.recent, args.include_amendments, args.limit);
    // The recent list holds about the latest thousand filings. If it does not
    // contain enough Form 4s, follow the older history pages, newest first.
    for page in newest_pages_first(&subs.older_files) {
        if wanted.len() >= args.limit {
            break;
        }
        println!(
            "only {} found so far; reading older history {} ({} to {})",
            wanted.len(),
            page.name,
            page.filing_from,
            page.filing_to
        );
        let rows = client
            .get_text(&history_page_url(&page.name))
            .map_err(|e| e.to_string())
            .and_then(|body| parse_history_page(&body).map_err(|e| e.to_string()));
        match rows {
            Ok(rows) => wanted.extend(select_form4s(
                &rows,
                args.include_amendments,
                args.limit - wanted.len(),
            )),
            Err(e) => {
                eprintln!(
                    "  could not read {}: {e}; stopping the history search",
                    page.name
                );
                break;
            }
        }
    }
    if wanted.is_empty() {
        println!("{} has no Form 4s in its recent filings", subs.name);
        return Ok(());
    }

    let out = args
        .out
        .unwrap_or_else(|| PathBuf::from(format!("data/form4/{cik:010}")));
    std::fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
    println!(
        "{}: downloading {} Form 4(s) into {}",
        subs.name,
        wanted.len(),
        out.display()
    );

    let (mut saved, mut skipped, mut failed) = (0, 0, 0);
    for filing in wanted {
        let xml_path = out.join(format!("{}.xml", filing.accession));
        if xml_path.exists() {
            skipped += 1;
            continue;
        }
        let url = filing.raw_document_url(cik);
        let xml = match client.get_text(&url) {
            Ok(x) if x.contains("<ownershipDocument") => x,
            Ok(_) => {
                eprintln!(
                    "  {}: not an ownership XML document, skipped ({url})",
                    filing.accession
                );
                failed += 1;
                continue;
            }
            Err(e) => {
                eprintln!("  {}: {e}", filing.accession);
                failed += 1;
                continue;
            }
        };
        std::fs::write(&xml_path, &xml).map_err(|e| format!("{}: {e}", xml_path.display()))?;
        let known_on = filing.known_on().map(|d| d.to_string()).unwrap_or_default();
        let meta = render_meta(&[
            ("accession", &filing.accession),
            ("form", &filing.form),
            ("known_on", &known_on),
            ("accepted", &filing.acceptance_date_time),
            ("filing_date", &filing.filing_date),
            ("report_date", &filing.report_date),
            ("company_cik", &format!("{cik:010}")),
            ("url", &url),
        ]);
        std::fs::write(meta_path(&xml_path), meta)
            .map_err(|e| format!("{}: {e}", xml_path.display()))?;
        saved += 1;

        match oe_form4::parse(&xml) {
            Ok(doc) => {
                let codes: Vec<&str> = doc.transactions.iter().map(|t| t.code.as_str()).collect();
                println!(
                    "  {}  accepted {}  {:<30} codes [{}]",
                    filing.accession,
                    if known_on.is_empty() { "?" } else { &known_on },
                    doc.owners.first().map_or("?", |o| o.name.as_str()),
                    codes.join(" ")
                );
            }
            Err(e) => eprintln!(
                "  {}: saved, but the parser failed: {e}  <- please keep this file, it is a new test case",
                filing.accession
            ),
        }
    }
    println!("done: {saved} saved, {skipped} already present, {failed} failed");
    if saved > 0 {
        println!(
            "next: cargo run -p oe-cli -- cluster-buys {}",
            out.display()
        );
    }
    Ok(())
}

#[cfg(not(feature = "net"))]
fn recent_form4(args: &[String]) -> Result<(), String> {
    parse_recent_args(args)?;
    Err("this build has no network support; rebuild with the default `net` feature".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sidecar_round_trip() {
        let dir = std::env::temp_dir().join(format!("oe-cli-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let xml = dir.join("0009999901-26-000003.xml");
        std::fs::write(&xml, "<ownershipDocument/>").unwrap();
        std::fs::write(
            meta_path(&xml),
            render_meta(&[
                ("accession", "0009999901-26-000003"),
                ("known_on", "2026-03-11"),
                ("url", "https://example.invalid/x.xml"),
            ]),
        )
        .unwrap();
        let meta = read_meta(&xml);
        assert_eq!(
            meta.accession.map(|a| a.to_string()).as_deref(),
            Some("0009999901-26-000003")
        );
        assert_eq!(meta.known_on, oe_core::Date::from_ymd(2026, 3, 11));
        assert_eq!(
            meta.source.as_deref(),
            Some("https://example.invalid/x.xml")
        );

        let missing = read_meta(&dir.join("absent.xml"));
        assert!(missing.known_on.is_none());
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn sidecar_known_on_is_rederived_from_the_utc_timestamp() {
        let dir = std::env::temp_dir().join(format!("oe-cli-tz-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let xml = dir.join("0009999901-25-000001.xml");
        // An old sidecar: known_on is the UTC date, a day late.
        std::fs::write(
            meta_path(&xml),
            render_meta(&[
                ("accession", "0009999901-25-000001"),
                ("known_on", "2026-01-01"),
                ("accepted", "2026-01-01T02:09:16.000Z"),
            ]),
        )
        .unwrap();
        assert_eq!(
            read_meta(&xml).known_on,
            oe_core::Date::from_ymd(2025, 12, 31)
        );
        std::fs::remove_dir_all(&dir).unwrap();
    }

    fn filing(accession: &str, form: &str) -> oe_edgar::FilingRef {
        oe_edgar::FilingRef {
            accession: accession.into(),
            form: form.into(),
            filing_date: "2026-01-01".into(),
            report_date: String::new(),
            acceptance_date_time: String::new(),
            primary_document: "x.xml".into(),
        }
    }

    #[test]
    fn selects_form4s_in_order_up_to_the_limit() {
        let list = [
            filing("a", "10-K"),
            filing("b", "4"),
            filing("c", "4/A"),
            filing("d", "5"),
            filing("e", "4"),
        ];
        let accs = |v: Vec<oe_edgar::FilingRef>| -> Vec<String> {
            v.into_iter().map(|f| f.accession).collect()
        };
        assert_eq!(accs(select_form4s(&list, false, 10)), ["b", "e"]);
        assert_eq!(accs(select_form4s(&list, true, 10)), ["b", "c", "e"]);
        assert_eq!(accs(select_form4s(&list, true, 2)), ["b", "c"]);
        assert!(select_form4s(&list, true, 0).is_empty());
    }

    #[test]
    fn history_pages_are_read_newest_first_whatever_the_sec_order() {
        let page = |name: &str, to: &str| oe_edgar::HistoryFileRef {
            name: name.into(),
            filing_from: String::new(),
            filing_to: to.into(),
        };
        let files = [
            page("old", "2010-01-01"),
            page("new", "2018-06-16"),
            page("mid", "2014-03-03"),
        ];
        let order: Vec<&str> = newest_pages_first(&files)
            .into_iter()
            .map(|p| p.name.as_str())
            .collect();
        assert_eq!(order, ["new", "mid", "old"]);
    }

    #[test]
    fn recent_args() {
        let a: Vec<String> = ["AAPL", "--limit", "5", "--include-amendments"]
            .iter()
            .map(|s| s.to_string())
            .collect();
        let p = parse_recent_args(&a).unwrap();
        assert_eq!(
            (p.company.as_str(), p.limit, p.include_amendments),
            ("AAPL", 5, true)
        );
        assert!(parse_recent_args(&[]).is_err());
        assert!(parse_recent_args(&["X".into(), "--limit".into(), "0".into()]).is_err());
    }
}
