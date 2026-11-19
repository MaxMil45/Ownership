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

    oe recent-form4 <TICKER or CIK> [--limit <n>] [--out <folder>] [--include-amendments]
        Download a company's most recent Form 4s from EDGAR (default 10) into
        data/form4/<CIK>/, each with a .meta.txt file recording when the SEC
        accepted it. Needs OE_USER_AGENT (see below).

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
        Some("cluster-buys") => cluster_buys(&args[1..]),
        Some("fetch") => match (args.get(1), args.get(2)) {
            (Some(url), Some(out)) => fetch(url, Path::new(out)),
            _ => Err("fetch needs a URL and an output file".into()),
        },
        Some("recent-form4") => recent_form4(&args[1..]),
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
    for line in text.lines() {
        let Some((key, value)) = line.split_once('=') else {
            continue;
        };
        match key.trim() {
            "accession" => meta.accession = value.trim().parse().ok(),
            "known_on" => meta.known_on = value.trim().parse().ok(),
            "url" => meta.source = Some(value.trim().to_string()),
            _ => {}
        }
    }
    meta
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
    use oe_edgar::{find_ticker, parse_company_tickers, parse_submissions, submissions_url};

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
    let wanted: Vec<_> = subs
        .recent
        .iter()
        .filter(|f| f.form == "4" || (args.include_amendments && f.form == "4/A"))
        .take(args.limit)
        .collect();
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
        let meta = render_meta(&[
            ("accession", &filing.accession),
            ("form", &filing.form),
            ("known_on", filing.known_on().unwrap_or("")),
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
                    filing.known_on().unwrap_or("?"),
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
