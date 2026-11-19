//! `oe daily-form4` and `oe follow`: find ownership filings across **all**
//! companies, through the EDGAR daily index (a whole past day) or the latest
//! filings feed (what was just filed), and save them like `oe recent-form4`.
//!
//! Both fetch each filing's complete submission text file, which holds the
//! ownership XML and the exact acceptance time in one request.

use super::{args, meta_path, render_meta};
use oe_core::Date;
use oe_edgar::{
    daily_index_url, filing_folder_url, parse_atom_feed, parse_form_index, parse_full_submission,
    unique_accessions, EdgarClient, EdgarError, FormIndex, LATEST_OWNERSHIP_FEED_URL,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_ROOT: &str = "data/form4";
const DEFAULT_ROOT_144: &str = "data/form144";
const DEFAULT_ROOT_13: &str = "data/sched13";
const DEFAULT_ROOT_PROXY: &str = "data/proxy";
/// Folder (under the output root) for filings whose XML the parser rejected.
const UNPARSED_DIR: &str = "unparsed";

/// Which kind of filing a command downloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// Forms 3/4/5: an `<ownershipDocument>`.
    Ownership,
    /// Form 144: an `<edgarSubmission>`.
    Form144,
    /// Schedule 13D / 13G: an `<edgarSubmission>`.
    Schedule13,
}

impl Kind {
    fn default_root(self) -> &'static str {
        match self {
            Kind::Ownership => DEFAULT_ROOT,
            Kind::Form144 => DEFAULT_ROOT_144,
            Kind::Schedule13 => DEFAULT_ROOT_13,
        }
    }

    fn forms(self, include_amendments: bool) -> &'static [&'static str] {
        match (self, include_amendments) {
            (Kind::Ownership, false) => &["4"],
            (Kind::Ownership, true) => &["4", "4/A"],
            (Kind::Form144, false) => &["144"],
            (Kind::Form144, true) => &["144", "144/A"],
            (Kind::Schedule13, false) => &["SCHEDULE 13D", "SCHEDULE 13G"],
            (Kind::Schedule13, true) => &[
                "SCHEDULE 13D",
                "SCHEDULE 13D/A",
                "SCHEDULE 13G",
                "SCHEDULE 13G/A",
            ],
        }
    }

    fn label(self) -> &'static str {
        match self {
            Kind::Ownership => "Form 4",
            Kind::Form144 => "Form 144",
            Kind::Schedule13 => "Schedule 13D/G",
        }
    }
}

// ---------------------------------------------------------------------------
// Arguments

#[derive(Debug, PartialEq)]
pub struct DailyArgs {
    pub date: Date,
    /// `None` means every filing of the day.
    pub limit: Option<usize>,
    pub out: PathBuf,
    pub include_amendments: bool,
}

pub fn parse_daily_args(args: &[String], default_root: &str) -> Result<DailyArgs, String> {
    let mut date = None;
    let mut parsed = DailyArgs {
        date: Date::from_ymd(1970, 1, 1).expect("valid date"),
        limit: Some(25),
        out: PathBuf::from(default_root),
        include_amendments: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--limit" => parsed.limit = Some(args::positive(&mut it, "--limit")?),
            "--all" => parsed.limit = None,
            "--out" => parsed.out = args::folder(&mut it, "--out")?,
            "--include-amendments" => parsed.include_amendments = true,
            other if date.is_none() => date = Some(args::date(other, "2026-11-18")?),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    parsed.date = date.ok_or("a date like 2026-11-18 is needed")?;
    Ok(parsed)
}

#[derive(Debug, PartialEq)]
pub struct FollowArgs {
    pub interval_secs: u64,
    /// `None` polls until stopped (Ctrl-C).
    pub polls: Option<u32>,
    pub out: PathBuf,
    pub include_amendments: bool,
}

/// Shortest allowed gap between polls. The SEC allows ten requests a second
/// overall, but there is no reason to hammer a feed that changes every minute.
const MIN_INTERVAL_SECS: u64 = 10;

pub fn parse_follow_args(args: &[String]) -> Result<FollowArgs, String> {
    let mut parsed = FollowArgs {
        interval_secs: 30,
        polls: None,
        out: PathBuf::from(DEFAULT_ROOT),
        include_amendments: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--interval" => {
                parsed.interval_secs = args::value(
                    &mut it,
                    "--interval",
                    &format!("a number of seconds, at least {MIN_INTERVAL_SECS}"),
                    |n| *n >= MIN_INTERVAL_SECS,
                )?
            }
            "--polls" => parsed.polls = Some(args::positive(&mut it, "--polls")?),
            "--out" => parsed.out = args::folder(&mut it, "--out")?,
            "--include-amendments" => parsed.include_amendments = true,
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    Ok(parsed)
}

// ---------------------------------------------------------------------------
// Saving one filing

/// Accession numbers already saved anywhere under `root` (one folder per
/// issuer CIK), so filings are not downloaded twice.
fn saved_accessions(root: &Path) -> HashSet<String> {
    let mut found = HashSet::new();
    let Ok(dirs) = std::fs::read_dir(root) else {
        return found;
    };
    for dir in dirs.filter_map(|d| d.ok()).filter(|d| d.path().is_dir()) {
        let Ok(files) = std::fs::read_dir(dir.path()) else {
            continue;
        };
        for file in files.filter_map(|f| f.ok()) {
            let name = file.file_name().to_string_lossy().into_owned();
            if let Some(acc) = name
                .strip_suffix(".xml")
                .or_else(|| name.strip_suffix(".hdr.sgml"))
            {
                found.insert(acc.to_string());
            }
        }
    }
    found
}

enum Saved {
    /// Saved and parsed; the string is a one-line description.
    Parsed(String),
    /// Saved, but the parser rejected it. Keep it: it is a new test case.
    ParserFailed(String),
    /// The submission has no ownership XML (not a Form 3/4/5).
    NoOwnershipXml(String),
}

/// What parsing a downloaded filing gave: the issuer's CIK (for the folder
/// name) and a one-line description, or the parser's error message.
struct ParsedFiling {
    issuer_cik: Option<String>,
    summary: Result<String, String>,
}

fn parse_ownership(accession: &str, xml: &str) -> ParsedFiling {
    match oe_form4::parse(xml) {
        Ok(doc) => {
            let codes: Vec<&str> = doc.transactions.iter().map(|t| t.code.as_str()).collect();
            ParsedFiling {
                issuer_cik: Some(doc.issuer.cik.to_string()),
                summary: Ok(format!(
                    "{accession}  Form {}  {:<6} {:<28} codes [{}]",
                    doc.document_type,
                    doc.issuer.ticker.as_deref().unwrap_or("-"),
                    doc.owners.first().map_or("?", |o| o.name.as_str()),
                    codes.join(" ")
                )),
            }
        }
        Err(e) => ParsedFiling {
            issuer_cik: None,
            summary: Err(e.to_string()),
        },
    }
}

fn parse_schedule13(accession: &str, xml: &str) -> ParsedFiling {
    match oe_sched13::parse(xml) {
        Ok(s) => ParsedFiling {
            issuer_cik: Some(s.issuer_cik.to_string()),
            summary: Ok(format!(
                "{accession}  {:<15} {:<26.26} {:<24.24} {:>6}%",
                s.submission_type,
                s.issuer_name,
                s.persons.first().map_or("?", |p| p.name.as_str()),
                s.largest_percent()
                    .map_or("-".to_string(), |p| format!("{p:.1}"))
            )),
        },
        Err(e) => ParsedFiling {
            issuer_cik: None,
            summary: Err(e.to_string()),
        },
    }
}

fn parse_form144(accession: &str, xml: &str) -> ParsedFiling {
    match oe_form144::parse(xml) {
        Ok(f) => ParsedFiling {
            issuer_cik: Some(f.issuer_cik.to_string()),
            summary: Ok(format!(
                "{accession}  Form {}  {:<28.28} {:<24.24} {:>12} units ${:>12.0}  {}",
                f.submission_type,
                f.issuer_name,
                f.seller_name,
                f.total_units()
                    .map_or("-".to_string(), |u| format!("{u:.0}")),
                f.total_market_value().unwrap_or(0.0),
                if f.under_plan() { "plan" } else { "no plan" }
            )),
        },
        Err(e) => ParsedFiling {
            issuer_cik: None,
            summary: Err(e.to_string()),
        },
    }
}

/// Downloads one filing's complete submission text, and saves its XML plus a
/// sidecar under `root/<issuer CIK>/`.
fn save_filing(client: &EdgarClient, url: &str, root: &Path, kind: Kind) -> Result<Saved, String> {
    let text = client.get_text(url).map_err(|e| e.to_string())?;
    let sub = parse_full_submission(&text).map_err(|e| format!("{url}: {e}"))?;
    let xml = match kind {
        Kind::Ownership => sub.ownership_xml.as_deref(),
        Kind::Form144 | Kind::Schedule13 => sub.primary_xml.as_deref(),
    };
    let Some(xml) = xml else {
        return Ok(Saved::NoOwnershipXml(format!(
            "{} is form {}, with no {} XML in it",
            sub.accession,
            sub.form,
            kind.label()
        )));
    };

    let ParsedFiling {
        issuer_cik: company_cik,
        summary,
    } = match kind {
        Kind::Ownership => parse_ownership(&sub.accession, xml),
        Kind::Form144 => parse_form144(&sub.accession, xml),
        Kind::Schedule13 => parse_schedule13(&sub.accession, xml),
    };
    let dir = match &company_cik {
        Some(cik) => root.join(cik),
        None => root.join(UNPARSED_DIR),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let xml_path = dir.join(format!("{}.xml", sub.accession));
    std::fs::write(&xml_path, xml).map_err(|e| format!("{}: {e}", xml_path.display()))?;

    let known_on = sub.known_on().map(|d| d.to_string()).unwrap_or_default();
    let company_cik = company_cik.unwrap_or_default();
    let meta = render_meta(&[
        ("accession", &sub.accession),
        ("form", &sub.form),
        ("known_on", &known_on),
        // The submission header is US Eastern time, unlike the API's UTC.
        ("accepted_eastern", &sub.accepted_eastern),
        ("company_cik", &company_cik),
        ("url", url),
    ]);
    std::fs::write(meta_path(&xml_path), meta)
        .map_err(|e| format!("{}: {e}", xml_path.display()))?;

    Ok(match summary {
        Ok(line) => Saved::Parsed(line),
        Err(e) => Saved::ParserFailed(format!(
            "{}: saved to {}, but the parser failed: {e}  <- keep this file, it is a new test case",
            sub.accession,
            xml_path.display()
        )),
    })
}

fn client() -> Result<EdgarClient, String> {
    EdgarClient::from_env().map_err(|e| e.to_string())
}

// ---------------------------------------------------------------------------
// oe daily-form4

pub fn daily_form4(args: &[String]) -> Result<(), String> {
    daily(args, Kind::Ownership)
}

pub fn daily_form144(args: &[String]) -> Result<(), String> {
    daily(args, Kind::Form144)
}

pub fn daily_schedule13(args: &[String]) -> Result<(), String> {
    daily(args, Kind::Schedule13)
}

/// Downloads and reads EDGAR's form index for one day, warning about any row
/// it could not read.
pub(crate) fn fetch_daily_index(client: &EdgarClient, date: Date) -> Result<FormIndex, String> {
    let url = daily_index_url(date);
    let body = client.get_text(&url).map_err(|e| match e {
        EdgarError::Status {
            code: 403 | 404, ..
        } => format!("no daily index for {date} (weekend, holiday, or not published yet): {url}"),
        other => other.to_string(),
    })?;
    let index = parse_form_index(&body);
    if index.skipped > 0 {
        eprintln!(
            "warning: {} index row(s) could not be read and were skipped",
            index.skipped
        );
    }
    Ok(index)
}

fn daily(args: &[String], kind: Kind) -> Result<(), String> {
    let args = parse_daily_args(args, kind.default_root())?;
    let client = client()?;

    let index = fetch_daily_index(&client, args.date)?;
    let forms = kind.forms(args.include_amendments);
    let filings = index.unique_filings(forms);
    println!(
        "{}: {} rows in the index, {} {} filing(s) ({})",
        args.date,
        index.entries.len(),
        filings.len(),
        kind.label(),
        forms.join(", ")
    );

    let mut known = saved_accessions(&args.out);
    let take = args.limit.unwrap_or(usize::MAX);
    let (mut saved, mut skipped, mut failed) = (0usize, 0usize, 0usize);
    for entry in filings.into_iter().take(take) {
        let Some(accession) = entry.accession() else {
            failed += 1;
            continue;
        };
        if known.contains(accession) {
            skipped += 1;
            continue;
        }
        match save_filing(&client, &entry.url(), &args.out, kind) {
            Ok(Saved::Parsed(line)) => {
                println!("  {line}");
                saved += 1;
                known.insert(accession.to_string());
            }
            Ok(Saved::ParserFailed(msg)) => {
                eprintln!("  {msg}");
                saved += 1;
                known.insert(accession.to_string());
            }
            Ok(Saved::NoOwnershipXml(msg)) => {
                eprintln!("  {msg}");
                failed += 1;
            }
            Err(e) => {
                eprintln!("  {accession}: {e}");
                failed += 1;
            }
        }
    }
    println!("done: {saved} saved, {skipped} already present, {failed} failed");
    Ok(())
}

// ---------------------------------------------------------------------------
// oe daily-proxy

/// Downloads the SGML header (under 1 KB) of every contested-solicitation
/// filing of one day and saves it under `data/proxy/<subject CIK>/`. The header
/// names the subject company and everyone who filed; the documents themselves
/// (HTML letters and proxy statements) are not fetched.
pub fn daily_proxy(args: &[String]) -> Result<(), String> {
    let args = parse_daily_args(args, DEFAULT_ROOT_PROXY)?;
    let client = client()?;
    let index = fetch_daily_index(&client, args.date)?;
    let filings = index.unique_filings(oe_events::proxy_contest::CONTEST_FORMS);
    println!(
        "{}: {} rows in the index, {} contested-solicitation filing(s)",
        args.date,
        index.entries.len(),
        filings.len()
    );

    let mut known = saved_accessions(&args.out);
    let (mut saved, mut skipped, mut failed) = (0usize, 0usize, 0usize);
    for entry in filings.into_iter().take(args.limit.unwrap_or(usize::MAX)) {
        let Some(accession) = entry.accession() else {
            failed += 1;
            continue;
        };
        if known.contains(accession) {
            skipped += 1;
            continue;
        }
        let url = format!(
            "{}{accession}.hdr.sgml",
            filing_folder_url(entry.cik, &accession.replace('-', ""))
        );
        let header = client
            .get_text(&url)
            .map_err(|e| e.to_string())
            .and_then(|text| {
                oe_edgar::parse_header_sgml(&text)
                    .map(|h| (h, text))
                    .map_err(|e| e.to_string())
            });
        let (h, text) = match header {
            Ok(x) => x,
            Err(e) => {
                eprintln!("  {accession}: {e}");
                failed += 1;
                continue;
            }
        };
        let Some(subject) = &h.subject else {
            eprintln!("  {accession}: the header names no subject company");
            failed += 1;
            continue;
        };
        let dir = args.out.join(subject.cik.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let path = dir.join(format!("{accession}.hdr.sgml"));
        std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
        let filers: Vec<&str> = h.filed_by.iter().map(|p| p.name.as_str()).collect();
        println!(
            "  {accession}  {:<8} {:<34.34} filed by {}",
            h.form,
            subject.name,
            filers.join("; ")
        );
        saved += 1;
        known.insert(accession.to_string());
    }
    println!("done: {saved} saved, {skipped} already present, {failed} failed");
    Ok(())
}

// ---------------------------------------------------------------------------
// oe follow

pub fn follow(args: &[String]) -> Result<(), String> {
    let args = parse_follow_args(args)?;
    let client = client()?;
    let forms = Kind::Ownership.forms(args.include_amendments);
    let mut known = saved_accessions(&args.out);
    println!(
        "following the EDGAR latest-filings feed every {}s; {} filing(s) already saved in {}",
        args.interval_secs,
        known.len(),
        args.out.display()
    );

    let mut poll = 0u32;
    loop {
        poll += 1;
        match poll_once(&client, forms, &args.out, &mut known) {
            Ok((entries, new)) => {
                println!("poll {poll}: {entries} feed entries, {new} new filing(s)")
            }
            // A failed poll (the feed does time out now and then) is not fatal.
            Err(e) => eprintln!(
                "poll {poll} failed: {e}; trying again in {}s",
                args.interval_secs
            ),
        }
        if args.polls.is_some_and(|max| poll >= max) {
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(args.interval_secs));
    }
}

/// One poll. Returns (entries in the feed, new filings handled).
fn poll_once(
    client: &EdgarClient,
    forms: &[&str],
    root: &Path,
    known: &mut HashSet<String>,
) -> Result<(usize, usize), String> {
    let body = client
        .get_text(LATEST_OWNERSHIP_FEED_URL)
        .map_err(|e| e.to_string())?;
    let entries = parse_atom_feed(&body).map_err(|e| e.to_string())?;
    let fresh: Vec<_> = unique_accessions(&entries, forms)
        .into_iter()
        .filter(|e| !known.contains(&e.accession))
        .collect();

    // The feed is newest first; handle the oldest new filing first.
    for entry in fresh.iter().rev() {
        let url = format!(
            "https://www.sec.gov/Archives/edgar/data/{}/{}.txt",
            entry.cik, entry.accession
        );
        match save_filing(client, &url, root, Kind::Ownership) {
            Ok(Saved::Parsed(line)) => {
                println!("  {line}  (feed time {})", entry.updated);
                known.insert(entry.accession.clone());
            }
            Ok(Saved::ParserFailed(msg)) => {
                eprintln!("  {msg}");
                known.insert(entry.accession.clone());
            }
            Ok(Saved::NoOwnershipXml(msg)) => {
                eprintln!("  {msg}");
                known.insert(entry.accession.clone());
            }
            // Not marked as seen: the next poll tries again.
            Err(e) => eprintln!("  {}: {e}", entry.accession),
        }
    }
    Ok((entries.len(), fresh.len()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn daily_args() {
        let a = parse_daily_args(
            &args(&["2026-11-18", "--limit", "5", "--include-amendments"]),
            DEFAULT_ROOT,
        )
        .unwrap();
        assert_eq!(a.date, Date::from_ymd(2026, 11, 18).unwrap());
        assert_eq!(a.limit, Some(5));
        assert!(a.include_amendments);
        assert_eq!(a.out, PathBuf::from("data/form4"));

        assert_eq!(
            parse_daily_args(&args(&["2026-11-18"]), DEFAULT_ROOT)
                .unwrap()
                .limit,
            Some(25)
        );
        assert_eq!(
            parse_daily_args(&args(&["--all", "2026-11-18"]), DEFAULT_ROOT)
                .unwrap()
                .limit,
            None
        );
        assert!(parse_daily_args(&[], DEFAULT_ROOT).is_err());
        // Form 144 downloads go to their own folder by default.
        assert_eq!(
            parse_daily_args(&args(&["2026-11-18"]), Kind::Form144.default_root())
                .unwrap()
                .out,
            PathBuf::from("data/form144")
        );
        assert_eq!(Kind::Ownership.forms(true), ["4", "4/A"]);
        assert_eq!(Kind::Form144.forms(false), ["144"]);
        assert!(parse_daily_args(&args(&["yesterday"]), DEFAULT_ROOT).is_err());
        assert!(parse_daily_args(&args(&["2026-13-01"]), DEFAULT_ROOT).is_err());
        assert!(parse_daily_args(&args(&["2026-11-18", "--limit", "0"]), DEFAULT_ROOT).is_err());
        assert!(parse_daily_args(&args(&["2026-11-18", "2026-11-19"]), DEFAULT_ROOT).is_err());
    }

    #[test]
    fn follow_args() {
        let a = parse_follow_args(&[]).unwrap();
        assert_eq!((a.interval_secs, a.polls), (30, None));
        let a = parse_follow_args(&args(&["--interval", "60", "--polls", "3"])).unwrap();
        assert_eq!((a.interval_secs, a.polls), (60, Some(3)));
        assert!(parse_follow_args(&args(&["--interval", "5"])).is_err());
        assert!(parse_follow_args(&args(&["--polls", "0"])).is_err());
        assert!(parse_follow_args(&args(&["--nope"])).is_err());
    }

    #[test]
    fn saved_accessions_scan_every_issuer_folder() {
        let root = std::env::temp_dir().join(format!("oe-live-test-{}", std::process::id()));
        let a = root.join("0000000001");
        let b = root.join("0000000002");
        std::fs::create_dir_all(&a).unwrap();
        std::fs::create_dir_all(&b).unwrap();
        std::fs::write(a.join("0000000001-26-000001.xml"), "x").unwrap();
        std::fs::write(a.join("0000000001-26-000001.meta.txt"), "x").unwrap();
        std::fs::write(b.join("0000000002-26-000007.xml"), "x").unwrap();
        std::fs::write(root.join("stray.xml"), "x").unwrap();
        let found = saved_accessions(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found.len(), 2);
        assert!(found.contains("0000000001-26-000001"));
        assert!(found.contains("0000000002-26-000007"));
        assert!(saved_accessions(Path::new("definitely/not/here")).is_empty());
    }

    #[test]
    fn xml_pulled_from_a_submission_parses_as_form4() {
        let path = format!(
            "{}/../../fixtures/edgar/submission_synthetic.txt",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(path).unwrap();
        let sub = parse_full_submission(&text).unwrap();
        let doc = oe_form4::parse(sub.ownership_xml.as_deref().unwrap()).unwrap();
        assert_eq!(doc.issuer.ticker.as_deref(), Some("SYNW"));
        assert_eq!(doc.owners[0].name, "Insider, Alpha");
    }
}
