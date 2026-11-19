//! `oe`: command-line tool for the ownership engine.

#[cfg(feature = "db")]
mod dbcmd;
#[cfg(feature = "net")]
mod live;
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

    oe daily-form4 <YYYY-MM-DD> [--limit <n> | --all] [--out <folder>] [--include-amendments]
        Download Form 4s filed by ANY company on one day, found through the EDGAR
        daily index (default: the first 25; a day has about 800). Saved under
        data/form4/<issuer CIK>/ like recent-form4. Needs OE_USER_AGENT.

    oe follow [--interval <seconds>] [--polls <n>] [--out <folder>] [--include-amendments]
        Poll the EDGAR latest-filings feed (default every 30 s, at least 10) and
        download each new Form 4 as it appears. Runs until stopped with Ctrl-C,
        or for --polls polls. Needs OE_USER_AGENT.

    oe daily-form144 <YYYY-MM-DD> [--limit <n> | --all] [--out <folder>] [--include-amendments]
        Download Form 144 notices (planned insider sales) filed by any company on one
        day. Saved under data/form144/<issuer CIK>/. Needs OE_USER_AGENT.

    oe daily-periodic <YYYY-MM-DD> [--limit <n> | --all] [--forms 10-Q,10-K] [--save-all] [--audit <n>] [--out <folder>]
        Find the 10-Q/10-K reports of one day (any company) whose Item 408 page says a director
        or officer terminated a Rule 10b5-1 plan, and save them under data/periodic/<CIK>/.
        Checks three small files per report and downloads the large one only when needed.
        --audit downloads <n> skipped reports too, to measure how often the cheap check is wrong.

    oe parse-408 <report.htm>
        Print the insider trading arrangements (Item 408) tagged in one 10-Q or 10-K.

    oe plan-terminations <folder>
        Run insider.plan_terminated@1 over downloaded reports (default folder: data/periodic).

    oe parse-form144 <file.xml>
        Print the key facts of one Form 144 XML file.

    oe planned-sales <folder> [--min-value <dollars>]
        Run insider.planned_sale@1 over downloaded Form 144 files (default folder:
        data/form144) and list the events, newest first. Default minimum 1,000,000.

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
        Some("parse-408") => match args.get(1) {
            Some(file) => parse_408_file(Path::new(file)),
            None => Err("parse-408 needs a file (a 10-Q or 10-K .htm)".into()),
        },
        Some("parse-form144") => match args.get(1) {
            Some(file) => parse_form144_file(Path::new(file)),
            None => Err("parse-form144 needs a file".into()),
        },
        Some("plan-terminations") => plan_terminations(&args[1..]),
        Some("planned-sales") => planned_sales(&args[1..]),
        Some("cluster-buys") => cluster_buys(&args[1..]),
        Some("fetch") => match (args.get(1), args.get(2)) {
            (Some(url), Some(out)) => fetch(url, Path::new(out)),
            _ => Err("fetch needs a URL and an output file".into()),
        },
        Some("recent-form4") => recent_form4(&args[1..]),
        #[cfg(feature = "net")]
        Some("daily-form4") => live::daily_form4(&args[1..]),
        #[cfg(feature = "net")]
        Some("daily-periodic") => periodic::daily_periodic(&args[1..]),
        #[cfg(feature = "net")]
        Some("daily-form144") => live::daily_form144(&args[1..]),
        #[cfg(feature = "net")]
        Some("follow") => live::follow(&args[1..]),
        #[cfg(not(feature = "net"))]
        Some("daily-form4") | Some("daily-form144") | Some("daily-periodic") | Some("follow") => {
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
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "accession" => meta.accession = value.trim().parse().ok(),
            "known_on" => stored_known_on = value.trim().parse().ok(),
            // The raw SEC timestamp is UTC. Converting it here, rather than
            // trusting the stored `known_on`, corrects sidecars written
            // before the conversion existed (they were a day late for
            // filings accepted after about 8 pm in New York).
            "accepted" => accepted_eastern = oe_core::us_eastern_date(value),
            "url" => meta.source = Some(value.trim().to_string()),
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

fn plan_terminations(args: &[String]) -> Result<(), String> {
    let folder = match args {
        [] => PathBuf::from("data/periodic"),
        [one] => PathBuf::from(one),
        _ => return Err("plan-terminations takes one folder, for example data/periodic".into()),
    };
    let files = find_files(&folder, "htm")?;
    let mut events = Vec::new();
    let (mut with_tags, mut failed) = (0usize, 0usize);
    for path in &files {
        let html = match std::fs::read_to_string(path) {
            Ok(h) => h,
            Err(e) => {
                failed += 1;
                eprintln!("  {}: {e}", path.display());
                continue;
            }
        };
        let doc = oe_item408::extract(&html);
        with_tags += usize::from(doc.uses_ecd_tags);
        events.extend(oe_events::detect_plan_terminations(&doc, &read_meta(path)));
    }

    // Newest first.
    events.sort_by_key(|e| std::cmp::Reverse(e.known_on));
    println!(
        "{} report(s), {} with insider-trading tags, {failed} unreadable, {} event(s) from insider.plan_terminated@1",
        files.len(),
        with_tags,
        events.len()
    );
    println!("known_on\tissuer\tperson\ttitle\tterminated\tperiod_end\treplaced\tform");
    for e in &events {
        println!(
            "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}",
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
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--min-value" => {
                params.min_value = it
                    .next()
                    .and_then(|v| v.replace(',', "").parse().ok())
                    .filter(|v: &f64| v.is_finite() && *v >= 0.0)
                    .ok_or("--min-value needs a number of dollars")?
            }
            other if folder.is_none() => folder = Some(PathBuf::from(other)),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    let folder = folder.ok_or("planned-sales needs a folder, for example data/form144")?;

    let files = find_xml_files(&folder)?;
    let mut filings = Vec::new();
    let mut failed = 0usize;
    for path in &files {
        let parsed = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|xml| oe_form144::parse(&xml).map_err(|e| e.to_string()));
        match parsed {
            Ok(form) => filings.push((form, read_meta(path))),
            Err(e) => {
                failed += 1;
                eprintln!("  {}: {e}", path.display());
            }
        }
    }

    let mut events = detect_planned_sales(&filings, &params);
    // Newest first; within a day, the largest sale first.
    events.sort_by(|a, b| {
        b.known_on.cmp(&a.known_on).then(
            b.aggregate_market_value
                .partial_cmp(&a.aggregate_market_value)
                .unwrap_or(std::cmp::Ordering::Equal),
        )
    });
    println!(
        "{} file(s), {} not Form 144 XML, {} event(s) from insider.planned_sale@1 (at least ${:.0})",
        files.len(),
        failed,
        events.len(),
        params.min_value
    );
    println!("known_on	issuer	seller	units	value	%outstanding	planned_for	plan");
    for e in &events {
        println!(
            "{}	{}	{}	{}	{:.0}	{}	{}	{}",
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
                params.window_days = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--window needs a number of days")?
            }
            "--min" => {
                params.min_insiders = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .ok_or("--min needs a number of insiders")?
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
    let body = client.get_text(url).map_err(|e| e.to_string())?;
    if let Some(parent) = out.parent().filter(|p| !p.as_os_str().is_empty()) {
        std::fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
    }
    std::fs::write(out, &body).map_err(|e| format!("{}: {e}", out.display()))?;
    println!("saved {} bytes to {}", body.len(), out.display());
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
            "--limit" => {
                parsed.limit = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|n| *n > 0)
                    .ok_or("--limit needs a positive number")?
            }
            "--out" => parsed.out = Some(PathBuf::from(it.next().ok_or("--out needs a folder")?)),
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
