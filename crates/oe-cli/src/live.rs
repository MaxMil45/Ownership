//! `oe daily-form4` and `oe follow`: find ownership filings across **all**
//! companies, through the EDGAR daily index (a whole past day) or the latest
//! filings feed (what was just filed), and save them like `oe recent-form4`.
//!
//! Both fetch each filing's complete submission text file, which holds the
//! ownership XML and the exact acceptance time in one request.

use super::{meta_path, render_meta};
use oe_core::Date;
use oe_edgar::{
    daily_index_url, parse_atom_feed, parse_form_index, parse_full_submission, unique_accessions,
    EdgarClient, EdgarError, LATEST_OWNERSHIP_FEED_URL,
};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

const DEFAULT_ROOT: &str = "data/form4";
/// Folder (under the output root) for filings whose XML the parser rejected.
const UNPARSED_DIR: &str = "unparsed";

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

pub fn parse_daily_args(args: &[String]) -> Result<DailyArgs, String> {
    let mut date = None;
    let mut parsed = DailyArgs {
        date: Date::from_ymd(1970, 1, 1).expect("valid date"),
        limit: Some(25),
        out: PathBuf::from(DEFAULT_ROOT),
        include_amendments: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--limit" => {
                parsed.limit = Some(
                    it.next()
                        .and_then(|v| v.parse().ok())
                        .filter(|n| *n > 0)
                        .ok_or("--limit needs a positive number")?,
                )
            }
            "--all" => parsed.limit = None,
            "--out" => parsed.out = PathBuf::from(it.next().ok_or("--out needs a folder")?),
            "--include-amendments" => parsed.include_amendments = true,
            other if date.is_none() => {
                date = Some(
                    other
                        .parse::<Date>()
                        .ok()
                        .filter(|_| other.len() == 10)
                        .ok_or_else(|| format!("{other:?} is not a date like 2026-11-18"))?,
                )
            }
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    parsed.date = date.ok_or("daily-form4 needs a date like 2026-11-18")?;
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
                parsed.interval_secs = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|n| *n >= MIN_INTERVAL_SECS)
                    .ok_or(format!(
                        "--interval needs a number of seconds, at least {MIN_INTERVAL_SECS}"
                    ))?
            }
            "--polls" => {
                parsed.polls = Some(
                    it.next()
                        .and_then(|v| v.parse().ok())
                        .filter(|n| *n > 0)
                        .ok_or("--polls needs a positive number")?,
                )
            }
            "--out" => parsed.out = PathBuf::from(it.next().ok_or("--out needs a folder")?),
            "--include-amendments" => parsed.include_amendments = true,
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    Ok(parsed)
}

fn wanted_forms(include_amendments: bool) -> &'static [&'static str] {
    if include_amendments {
        &["4", "4/A"]
    } else {
        &["4"]
    }
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
            if let Some(acc) = name.strip_suffix(".xml") {
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

/// Downloads one filing's complete submission text, and saves its ownership
/// XML plus a sidecar under `root/<issuer CIK>/`.
fn save_filing(client: &EdgarClient, url: &str, root: &Path) -> Result<Saved, String> {
    let text = client.get_text(url).map_err(|e| e.to_string())?;
    let sub = parse_full_submission(&text).map_err(|e| format!("{url}: {e}"))?;
    let Some(xml) = sub.ownership_xml.as_deref() else {
        return Ok(Saved::NoOwnershipXml(format!(
            "{} is form {}, not an ownership filing",
            sub.accession, sub.form
        )));
    };

    let parsed = oe_form4::parse(xml);
    let dir = match &parsed {
        Ok(doc) => root.join(doc.issuer.cik.to_string()),
        Err(_) => root.join(UNPARSED_DIR),
    };
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let xml_path = dir.join(format!("{}.xml", sub.accession));
    std::fs::write(&xml_path, xml).map_err(|e| format!("{}: {e}", xml_path.display()))?;

    let known_on = sub.known_on().map(|d| d.to_string()).unwrap_or_default();
    let company_cik = parsed
        .as_ref()
        .map(|d| d.issuer.cik.to_string())
        .unwrap_or_default();
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

    Ok(match parsed {
        Ok(doc) => {
            let codes: Vec<&str> = doc.transactions.iter().map(|t| t.code.as_str()).collect();
            Saved::Parsed(format!(
                "{}  Form {}  {:<6} {:<28} codes [{}]",
                sub.accession,
                doc.document_type,
                doc.issuer.ticker.as_deref().unwrap_or("-"),
                doc.owners.first().map_or("?", |o| o.name.as_str()),
                codes.join(" ")
            ))
        }
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
    let args = parse_daily_args(args)?;
    let client = client()?;

    let url = daily_index_url(args.date);
    let body = client.get_text(&url).map_err(|e| match e {
        EdgarError::Status {
            code: 403 | 404, ..
        } => format!(
            "no daily index for {} (weekend, holiday, or not published yet): {url}",
            args.date
        ),
        other => other.to_string(),
    })?;
    let index = parse_form_index(&body);
    if index.skipped > 0 {
        eprintln!(
            "warning: {} index row(s) could not be read and were skipped",
            index.skipped
        );
    }
    let filings = index.unique_filings(wanted_forms(args.include_amendments));
    println!(
        "{}: {} rows in the index, {} Form 4 filing(s){}",
        args.date,
        index.entries.len(),
        filings.len(),
        if args.include_amendments {
            " (including 4/A)"
        } else {
            ""
        }
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
        match save_filing(&client, &entry.url(), &args.out) {
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
// oe follow

pub fn follow(args: &[String]) -> Result<(), String> {
    let args = parse_follow_args(args)?;
    let client = client()?;
    let forms = wanted_forms(args.include_amendments);
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
        match save_filing(client, &url, root) {
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
        let a = parse_daily_args(&args(&[
            "2026-11-18",
            "--limit",
            "5",
            "--include-amendments",
        ]))
        .unwrap();
        assert_eq!(a.date, Date::from_ymd(2026, 11, 18).unwrap());
        assert_eq!(a.limit, Some(5));
        assert!(a.include_amendments);
        assert_eq!(a.out, PathBuf::from("data/form4"));

        assert_eq!(
            parse_daily_args(&args(&["2026-11-18"])).unwrap().limit,
            Some(25)
        );
        assert_eq!(
            parse_daily_args(&args(&["--all", "2026-11-18"]))
                .unwrap()
                .limit,
            None
        );
        assert!(parse_daily_args(&[]).is_err());
        assert!(parse_daily_args(&args(&["yesterday"])).is_err());
        assert!(parse_daily_args(&args(&["2026-13-01"])).is_err());
        assert!(parse_daily_args(&args(&["2026-11-18", "--limit", "0"])).is_err());
        assert!(parse_daily_args(&args(&["2026-11-18", "2026-11-19"])).is_err());
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
