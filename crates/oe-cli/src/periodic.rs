//! `oe daily-periodic`: find 10-Q and 10-K reports filed on one day by any
//! company, and keep the ones whose Item 408 disclosure says a director or
//! officer terminated a Rule 10b5-1 plan.
//!
//! Per report it first fetches three small files (the header, `FilingSummary.xml`
//! and the rendered "Insider Trading Arrangements" page, together about 100 KB)
//! and only downloads the 2 MB report when that page shows a termination. The
//! other reports are logged in `checked.tsv` so a rerun does not look at them
//! again.

use super::{args, meta_path, render_meta};
use oe_core::Date;
use oe_edgar::{
    filing_folder_url, parse_filing_summary, parse_header_sgml, EdgarClient, EdgarError,
};
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const DEFAULT_ROOT: &str = "data/periodic";
const CHECKED_FILE: &str = "checked.tsv";

#[derive(Debug, PartialEq)]
pub struct PeriodicArgs {
    pub date: Date,
    /// Last day of a range (`--to`); `None` for one day.
    pub to: Option<Date>,
    /// `None` means every report of the day.
    pub limit: Option<usize>,
    pub out: PathBuf,
    pub forms: Vec<String>,
    /// Keep every report that has the insider-trading page, not only those with a termination.
    pub save_all: bool,
    /// Also download this many of the reports the cheap check skipped, to measure its error rate.
    pub audit: usize,
}

pub fn parse_periodic_args(args: &[String]) -> Result<PeriodicArgs, String> {
    let mut date = None;
    let mut parsed = PeriodicArgs {
        date: Date::from_ymd(1970, 1, 1).expect("valid date"),
        to: None,
        limit: Some(25),
        out: PathBuf::from(DEFAULT_ROOT),
        forms: vec!["10-Q".into(), "10-K".into()],
        save_all: false,
        audit: 0,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--limit" => parsed.limit = Some(args::positive(&mut it, "--limit")?),
            "--all" => parsed.limit = None,
            "--out" => parsed.out = args::folder(&mut it, "--out")?,
            "--forms" => {
                parsed.forms = it
                    .next()
                    .ok_or("--forms needs a list such as 10-Q,10-K")?
                    .split(',')
                    .map(|f| f.trim().to_string())
                    .filter(|f| !f.is_empty())
                    .collect();
                if parsed.forms.is_empty() {
                    return Err("--forms needs at least one form".into());
                }
            }
            "--save-all" => parsed.save_all = true,
            "--to" => {
                let text = it.next().ok_or("--to needs a date such as 2026-11-20")?;
                parsed.to = Some(args::date(text, "2026-11-20")?);
            }
            "--audit" => {
                parsed.audit = args::value(&mut it, "--audit", "a number of reports", |_| true)?
            }
            other if date.is_none() => date = Some(args::date(other, "2026-11-09")?),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    parsed.date = date.ok_or("a date like 2026-11-09 is needed")?;
    if parsed.to.is_some_and(|t| t < parsed.date) {
        return Err("--to is before the first date".into());
    }
    Ok(parsed)
}

/// `accession<TAB>status` lines of the log of reports already looked at.
pub fn parse_checked(text: &str) -> HashMap<String, String> {
    text.lines()
        .filter_map(|l| l.split_once('\t'))
        .map(|(a, s)| (a.to_string(), s.trim().to_string()))
        .collect()
}

#[derive(Debug, Default, Clone)]
struct Tally {
    looked_at: usize,
    already: usize,
    no_xbrl: usize,
    no_report: usize,
    none: usize,
    saved: usize,
    errors: usize,
    audited: usize,
    audit_missed: usize,
}

impl Tally {
    fn add(&mut self, o: &Tally) {
        self.looked_at += o.looked_at;
        self.already += o.already;
        self.no_xbrl += o.no_xbrl;
        self.no_report += o.no_report;
        self.none += o.none;
        self.saved += o.saved;
        self.errors += o.errors;
        self.audited += o.audited;
        self.audit_missed += o.audit_missed;
    }
}

/// Reports checked at once. Each check waits mostly on the network; the
/// client's shared rate limiter keeps the total at 8 requests a second.
const CHECK_THREADS: usize = 6;

pub fn daily_periodic(args: &[String]) -> Result<(), String> {
    let args = parse_periodic_args(args)?;
    let client = EdgarClient::from_env().map_err(|e| e.to_string())?;
    let range = args.to.is_some();
    let mut total = Tally::default();
    for day in super::live::days_between(args.date, args.to) {
        match periodic_day(&client, &args, day) {
            Ok(t) => total.add(&t),
            Err(e) if range && e.starts_with("no daily index") => println!("{day}: skipped, {e}"),
            Err(e) => return Err(e),
        }
    }
    if range {
        println!(
            "all days: {} looked at ({} already logged); {} with a termination saved, {} with the page but no termination, {} without the page, {} without XBRL, {} errors",
            total.looked_at, total.already, total.saved, total.none, total.no_report, total.no_xbrl, total.errors
        );
    }
    Ok(())
}

fn periodic_day(client: &EdgarClient, args: &PeriodicArgs, day: Date) -> Result<Tally, String> {
    let index = super::live::fetch_daily_index(client, day)?;
    let forms: Vec<&str> = args.forms.iter().map(String::as_str).collect();
    let filings = index.unique_filings(&forms);
    println!(
        "{}: {} rows in the index, {} report(s) of form {}",
        day,
        index.entries.len(),
        filings.len(),
        forms.join(", ")
    );

    std::fs::create_dir_all(&args.out).map_err(|e| format!("{}: {e}", args.out.display()))?;
    let checked_path = args.out.join(CHECKED_FILE);
    let checked = parse_checked(&std::fs::read_to_string(&checked_path).unwrap_or_default());
    let mut tally = Tally::default();
    let take = args.limit.unwrap_or(usize::MAX);
    let mut todo: Vec<(u64, String)> = Vec::new();
    for entry in filings.into_iter().take(take) {
        let Some(accession) = entry.accession() else {
            tally.errors += 1;
            continue;
        };
        if checked.contains_key(accession) {
            tally.already += 1;
            continue;
        }
        todo.push((entry.cik, accession.to_string()));
    }

    // The audit counts across reports, so it runs on one thread.
    let threads = if args.audit > 0 { 1 } else { CHECK_THREADS };
    let next = std::sync::atomic::AtomicUsize::new(0);
    let log = std::sync::Mutex::new(());
    let results: Vec<Result<Tally, String>> = std::thread::scope(|scope| {
        let workers: Vec<_> = (0..threads.min(todo.len()))
            .map(|_| {
                scope.spawn(|| -> Result<Tally, String> {
                    let mut t = Tally::default();
                    loop {
                        let i = next.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let Some((cik, accession)) = todo.get(i) else {
                            return Ok(t);
                        };
                        t.looked_at += 1;
                        let (status, line) = match check_one(client, *cik, accession, args, &mut t)
                        {
                            Ok(r) => r,
                            Err(e) => {
                                // Not logged: a transient error should be retried next time.
                                eprintln!("  {accession}: {e}");
                                t.errors += 1;
                                continue;
                            }
                        };
                        match status.as_str() {
                            "no_xbrl" => t.no_xbrl += 1,
                            "no_report" => t.no_report += 1,
                            "none" => t.none += 1,
                            "saved" => t.saved += 1,
                            _ => {}
                        }
                        let _guard = log.lock().unwrap_or_else(|e| e.into_inner());
                        if let Some(line) = line {
                            println!("  {line}");
                        }
                        append_checked(&checked_path, accession, &status)?;
                    }
                })
            })
            .collect();
        workers
            .into_iter()
            .map(|w| {
                w.join()
                    .unwrap_or_else(|_| Err("a worker thread panicked".into()))
            })
            .collect()
    });
    for r in results {
        tally.add(&r?);
    }

    println!(
        "done: {} looked at ({} already logged); {} with a termination saved, {} reports with the page but no termination, {} without the page, {} without XBRL, {} errors",
        tally.looked_at, tally.already, tally.saved, tally.none, tally.no_report, tally.no_xbrl, tally.errors
    );
    if args.audit > 0 {
        println!(
            "audit: downloaded {} report(s) the cheap check skipped; the full extractor found a termination in {} of them",
            tally.audited, tally.audit_missed
        );
    }
    Ok(tally)
}

fn append_checked(path: &Path, accession: &str, status: &str) -> Result<(), String> {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(f, "{accession}\t{status}").map_err(|e| format!("{}: {e}", path.display()))
}

/// Looks at one report. Returns its status for the log, and a line to print
/// if it was kept.
fn check_one(
    client: &EdgarClient,
    cik: u64,
    accession: &str,
    args: &PeriodicArgs,
    tally: &mut Tally,
) -> Result<(String, Option<String>), String> {
    let folder = filing_folder_url(cik, &accession.replace('-', ""));
    let header = client
        .get_text(&format!("{folder}{accession}.hdr.sgml"))
        .map_err(|e| e.to_string())
        .and_then(|t| parse_header_sgml(&t).map_err(|e| e.to_string()))?;

    let summary = match client.get_text(&format!("{folder}FilingSummary.xml")) {
        Ok(t) => parse_filing_summary(&t),
        Err(EdgarError::Status { code: 404, .. }) => return Ok(("no_xbrl".into(), None)),
        Err(e) => return Err(e.to_string()),
    };
    let Some(report) = summary.insider_trading_report() else {
        return Ok(("no_report".into(), None));
    };
    let page = client
        .get_text(&format!("{folder}{}", report.html_file))
        .map_err(|e| e.to_string())?;
    let shows_termination = oe_item408::report_page_shows_termination(&page);

    let audit_this = !shows_termination && tally.audited < args.audit;
    if !(shows_termination || args.save_all || audit_this) {
        return Ok(("none".into(), None));
    }
    let main = summary
        .main_document
        .as_deref()
        .ok_or("FilingSummary.xml does not name the main document")?;
    let html = client
        .get_text(&format!("{folder}{main}"))
        .map_err(|e| e.to_string())?;
    let doc = oe_item408::extract(&html);
    let terminated = doc
        .arrangements
        .iter()
        .filter(|a| a.rule_10b5_1_terminated)
        .count();

    if audit_this {
        tally.audited += 1;
        if terminated > 0 {
            tally.audit_missed += 1;
            eprintln!("  AUDIT: {accession} was skipped by the page check but the extractor found {terminated} termination(s)");
        }
        if !args.save_all {
            return Ok(("none".into(), None));
        }
    }

    // Keep the report.
    let company = doc
        .cik
        .map(|c| c.to_string())
        .unwrap_or_else(|| format!("{cik:010}"));
    let dir = args.out.join(&company);
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{accession}.htm"));
    std::fs::write(&path, &html).map_err(|e| format!("{}: {e}", path.display()))?;
    let known_on = header.known_on().map(|d| d.to_string()).unwrap_or_default();
    let period = header.period.map(|d| d.to_string()).unwrap_or_default();
    let filing_date = header
        .filing_date
        .map(|d| d.to_string())
        .unwrap_or_default();
    let url = format!("{folder}{main}");
    let meta = render_meta(&[
        ("accession", accession),
        ("form", &header.form),
        // The header time is US Eastern; EDGAR's filing date can be a day later.
        ("known_on", &known_on),
        ("accepted_eastern", &header.accepted_eastern),
        ("filing_date", &filing_date),
        ("period", &period),
        ("company_cik", &company),
        ("url", &url),
    ]);
    std::fs::write(meta_path(&path), meta).map_err(|e| format!("{}: {e}", path.display()))?;

    let line = format!(
        "{accession}  {:<5} {:<30.30} accepted {}  {} arrangement(s), {terminated} terminated",
        header.form,
        doc.registrant.as_deref().unwrap_or("?"),
        header.accepted_eastern,
        doc.arrangements.len()
    );
    Ok(("saved".into(), Some(line)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn periodic_args() {
        let a = parse_periodic_args(&args(&["2026-11-09"])).unwrap();
        assert_eq!(a.date, Date::from_ymd(2026, 11, 9).unwrap());
        assert_eq!((a.limit, a.save_all, a.audit), (Some(25), false, 0));
        assert_eq!(a.forms, ["10-Q", "10-K"]);
        assert_eq!(a.out, PathBuf::from("data/periodic"));

        let b = parse_periodic_args(&args(&[
            "--all",
            "2026-11-09",
            "--forms",
            "10-Q",
            "--audit",
            "5",
            "--save-all",
        ]))
        .unwrap();
        assert_eq!((b.limit, b.save_all, b.audit), (None, true, 5));
        assert_eq!(b.forms, ["10-Q"]);

        assert!(parse_periodic_args(&[]).is_err());
        assert!(parse_periodic_args(&args(&["tomorrow"])).is_err());
        assert!(parse_periodic_args(&args(&["2026-11-09", "--forms", ","])).is_err());
        assert!(parse_periodic_args(&args(&["2026-11-09", "--limit", "0"])).is_err());
        assert!(parse_periodic_args(&args(&["2026-11-09", "2026-11-10"])).is_err());

        let r = parse_periodic_args(&args(&["2023-10-02", "--to", "2023-10-09"])).unwrap();
        assert_eq!(a.to, None);
        assert_eq!(r.to, Some(Date::from_ymd(2023, 10, 9).unwrap()));
        // Monday to the next Monday: six weekdays.
        assert_eq!(super::super::live::days_between(r.date, r.to).len(), 6);
        assert!(parse_periodic_args(&args(&["2023-10-09", "--to", "2023-10-02"])).is_err());
    }

    #[test]
    fn the_checked_log_round_trips() {
        let log = "0000000001-26-000001\tnone\n0000000002-26-000002\tsaved\ngarbage line\n";
        let m = parse_checked(log);
        assert_eq!(m.len(), 2);
        assert_eq!(m["0000000002-26-000002"], "saved");
        assert!(parse_checked("").is_empty());

        let dir = std::env::temp_dir().join(format!("oe-periodic-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(CHECKED_FILE);
        append_checked(&path, "0000000003-26-000003", "no_report").unwrap();
        append_checked(&path, "0000000004-26-000004", "none").unwrap();
        let back = parse_checked(&std::fs::read_to_string(&path).unwrap());
        std::fs::remove_dir_all(&dir).unwrap();
        assert_eq!(back.len(), 2);
        assert_eq!(back["0000000003-26-000003"], "no_report");
    }
}
