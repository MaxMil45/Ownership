//! `oe`: command-line tool for the ownership engine.

use oe_core::FilingMeta;
use oe_events::{detect_cluster_buys, trades_from_document, ClusterBuyParams, InsiderTrade};
use std::path::{Path, PathBuf};
use std::process::ExitCode;

const USAGE: &str = "\
oe: ownership engine command-line tool

USAGE:
    oe parse-form4 <file.xml>
        Print every transaction in one Form 3/4/5 XML file.

    oe cluster-buys <folder> [--window <days>] [--min <insiders>] [--keep-routine]
        Parse every .xml file in a folder and report insider.cluster_buy@1 events.

    oe fetch <url> <output-file>
        Download one EDGAR document politely. Needs OE_USER_AGENT set to
        your name and email, e.g. \"Jane Doe jane.doe@example.com\".

    oe help
";

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("parse-form4") => match args.get(1) {
            Some(file) => parse_form4(Path::new(file)),
            None => Err("parse-form4 needs a file".into()),
        },
        Some("cluster-buys") => cluster_buys(&args[1..]),
        Some("fetch") => match (args.get(1), args.get(2)) {
            (Some(url), Some(out)) => fetch(url, Path::new(out)),
            _ => Err("fetch needs a URL and an output file".into()),
        },
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

fn read_document(path: &Path) -> Result<oe_core::OwnershipDocument, String> {
    let xml = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    oe_form4::parse(&xml).map_err(|e| format!("{}: {e}", path.display()))
}

fn opt(v: Option<f64>) -> String {
    v.map_or_else(|| "-".to_string(), |x| format!("{x}"))
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
        let meta = FilingMeta {
            source: Some(path.display().to_string()),
            ..FilingMeta::default()
        };
        trades.extend(trades_from_document(&doc, &meta));
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
            None => println!("  knowable on: unknown (no EDGAR acceptance dates for local files)"),
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
