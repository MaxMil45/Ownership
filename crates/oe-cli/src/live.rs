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
    unique_accessions, EdgarClient, EdgarError, FormIndex,
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
            (Kind::Ownership, false) => &["3", "4", "5"],
            (Kind::Ownership, true) => &["3", "3/A", "4", "4/A", "5", "5/A"],
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
            Kind::Ownership => "Form 3/4/5",
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
    /// Last day of a range (`--to`); `None` for one day.
    pub to: Option<Date>,
    /// `None` means every filing of the day.
    pub limit: Option<usize>,
    pub out: PathBuf,
    pub include_amendments: bool,
}

pub fn parse_daily_args(args: &[String], default_root: &str) -> Result<DailyArgs, String> {
    let mut date = None;
    let mut parsed = DailyArgs {
        date: Date::from_ymd(1970, 1, 1).expect("valid date"),
        to: None,
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
            "--to" => {
                let text = it.next().ok_or("--to needs a date such as 2026-11-20")?;
                parsed.to = Some(args::date(text, "2026-11-20")?);
            }
            other if date.is_none() => date = Some(args::date(other, "2026-11-18")?),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    parsed.date = date.ok_or("a date like 2026-11-18 is needed")?;
    if parsed.to.is_some_and(|t| t < parsed.date) {
        return Err("--to is before the first date".into());
    }
    Ok(parsed)
}

/// The days to fetch: the one date, or every weekday from it to `--to`
/// (EDGAR publishes no index on weekends).
#[cfg(test)]
fn days(args: &DailyArgs) -> Vec<Date> {
    days_between(args.date, args.to)
}

/// `from` alone, or every weekday from `from` to `to`.
pub fn days_between(from: Date, to: Option<Date>) -> Vec<Date> {
    let Some(to) = to else {
        return vec![from];
    };
    // 1970-01-05 was a Monday.
    let monday = Date::from_ymd(1970, 1, 5).expect("valid date");
    let mut out = Vec::new();
    let mut d = from;
    while d <= to {
        if monday.days_until(d).rem_euclid(7) < 5 {
            out.push(d);
        }
        d = d.add_days(1);
    }
    out
}

/// Seconds to wait before each new try of a daily index the SEC failed to
/// serve (a 5xx, a 429 or a dropped connection). Waiting only slows us down.
const INDEX_RETRY_WAITS: [u64; 3] = [5, 30, 120];

/// An error worth trying again later: the server was busy or the network
/// dropped, not a missing or refused file.
fn is_transient(e: &EdgarError) -> bool {
    match e {
        EdgarError::Status { code, .. } => *code == 429 || *code >= 500,
        EdgarError::Transport { .. } => true,
        _ => false,
    }
}

/// Runs `one` for each day of the range. In a range, a day without an index
/// (a market holiday) is reported and skipped, and a day that fails is
/// reported and the range goes on; the failed days are listed at the end as
/// an error. A single day keeps its error.
pub(crate) fn each_day_between(
    from: Date,
    to: Option<Date>,
    mut one: impl FnMut(Date) -> Result<(), String>,
) -> Result<(), String> {
    let range = to.is_some();
    let mut failed: Vec<Date> = Vec::new();
    for day in days_between(from, to) {
        match one(day) {
            Ok(()) => {}
            Err(e) if range && e.starts_with("no daily index") => println!("{day}: skipped, {e}"),
            Err(e) if range => {
                println!("{day}: FAILED, {e}");
                failed.push(day);
            }
            Err(e) => return Err(e),
        }
    }
    if failed.is_empty() {
        return Ok(());
    }
    let list: Vec<String> = failed.iter().map(Date::to_string).collect();
    Err(format!(
        "{} day(s) failed; run them again (saved filings are skipped): {}",
        failed.len(),
        list.join(" ")
    ))
}

fn each_day(args: &DailyArgs, one: impl FnMut(Date) -> Result<(), String>) -> Result<(), String> {
    each_day_between(args.date, args.to, one)
}

/// A live feed `oe follow` can poll.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LiveKind {
    Form4,
    Form144,
    Schedule13,
    Proxy,
}

impl LiveKind {
    pub const ALL: [LiveKind; 4] = [
        LiveKind::Form4,
        LiveKind::Form144,
        LiveKind::Schedule13,
        LiveKind::Proxy,
    ];

    /// The name used by `--forms`.
    pub fn name(self) -> &'static str {
        match self {
            LiveKind::Form4 => "4",
            LiveKind::Form144 => "144",
            LiveKind::Schedule13 => "13",
            LiveKind::Proxy => "proxy",
        }
    }

    fn parse(name: &str) -> Option<LiveKind> {
        LiveKind::ALL.into_iter().find(|k| k.name() == name)
    }

    /// Feed type prefixes to poll (the feed's filter is a prefix match). The
    /// contest forms get one feed each: they are rare, so a narrow feed's 100
    /// entries reach back days, where `DEF` would be full of ordinary proxies.
    fn type_prefixes(self) -> &'static [&'static str] {
        match self {
            LiveKind::Form4 => &["4", "3", "5"],
            LiveKind::Form144 => &["144"],
            LiveKind::Schedule13 => &["SCHEDULE 13"],
            LiveKind::Proxy => &[
                "PREC14", "DEFC14", "PREN14A", "PRRN14A", "DEFN14A", "DFAN14A",
            ],
        }
    }

    fn forms(self, include_amendments: bool) -> &'static [&'static str] {
        match self {
            LiveKind::Form4 => Kind::Ownership.forms(include_amendments),
            LiveKind::Form144 => Kind::Form144.forms(include_amendments),
            LiveKind::Schedule13 => Kind::Schedule13.forms(include_amendments),
            LiveKind::Proxy => oe_events::proxy_contest::CONTEST_FORMS,
        }
    }

    /// Folder under the data root, as the daily commands use.
    fn folder(self) -> &'static str {
        match self {
            LiveKind::Form4 => "form4",
            LiveKind::Form144 => "form144",
            LiveKind::Schedule13 => "sched13",
            LiveKind::Proxy => "proxy",
        }
    }
}

#[derive(Debug, PartialEq)]
pub struct FollowArgs {
    pub interval_secs: u64,
    /// `None` polls until stopped (Ctrl-C).
    pub polls: Option<u32>,
    /// Stop after the poll that ends at or after this time (seconds since 1970, UTC).
    pub until: Option<i64>,
    /// Which feeds to poll, in this order.
    pub kinds: Vec<LiveKind>,
    /// Data root; each kind saves under `<root>/<kind folder>`.
    pub root: PathBuf,
    /// Overrides the Form 4 folder (the option predates `--root`).
    pub out: Option<PathBuf>,
    pub include_amendments: bool,
    /// Run `oe alerts` over the folders that got new filings, after each poll.
    pub alert: bool,
    /// Watchlist for those alerts.
    pub watchlist: Option<PathBuf>,
    /// `--db`: load each new Form 3/4/5 into the PostgreSQL ledger after the
    /// poll, and give the alerts' routine-trader filter the ledger's history.
    pub db_history: bool,
}

impl FollowArgs {
    pub fn folder(&self, kind: LiveKind) -> PathBuf {
        match (kind, &self.out) {
            (LiveKind::Form4, Some(out)) => out.clone(),
            _ => self.root.join(kind.folder()),
        }
    }
}

/// Shortest allowed gap between polls. The SEC allows ten requests a second
/// overall, but there is no reason to hammer a feed that changes every minute.
const MIN_INTERVAL_SECS: u64 = 10;

pub fn parse_follow_args(args: &[String]) -> Result<FollowArgs, String> {
    let mut parsed = FollowArgs {
        interval_secs: 30,
        polls: None,
        until: None,
        kinds: LiveKind::ALL.to_vec(),
        root: PathBuf::from("data"),
        out: None,
        include_amendments: false,
        alert: false,
        watchlist: None,
        db_history: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--forms" => {
                let list = it
                    .next()
                    .ok_or("--forms needs a list such as 4,144,13,proxy")?;
                let mut kinds = Vec::new();
                for name in list.split(',').map(str::trim).filter(|n| !n.is_empty()) {
                    let kind = LiveKind::parse(name).ok_or_else(|| {
                        format!("--forms: unknown form {name:?} (use 4, 144, 13 or proxy)")
                    })?;
                    if !kinds.contains(&kind) {
                        kinds.push(kind);
                    }
                }
                if kinds.is_empty() {
                    return Err("--forms needs at least one of 4, 144, 13, proxy".into());
                }
                parsed.kinds = kinds;
            }
            "--root" => parsed.root = args::folder(&mut it, "--root")?,
            "--alert" => parsed.alert = true,
            "--db" => parsed.db_history = true,
            "--watchlist" => {
                parsed.watchlist = Some(args::file(&mut it, "--watchlist")?);
                parsed.alert = true;
            }
            "--interval" => {
                parsed.interval_secs = args::value(
                    &mut it,
                    "--interval",
                    &format!("a number of seconds, at least {MIN_INTERVAL_SECS}"),
                    |n| *n >= MIN_INTERVAL_SECS,
                )?
            }
            "--polls" => parsed.polls = Some(args::positive(&mut it, "--polls")?),
            "--until" => {
                let t = it
                    .next()
                    .ok_or("--until needs a UTC time such as 2026-11-23T23:00:00Z")?;
                parsed.until = Some(oe_core::utc_to_unix(t).ok_or(format!(
                    "--until: {t:?} is not a UTC time such as 2026-11-23T23:00:00Z"
                ))?);
            }
            "--out" => parsed.out = Some(args::folder(&mut it, "--out")?),
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
            if let Some(acc) = name.strip_suffix(".hdr.sgml") {
                found.insert(acc.to_string());
            } else if let Some(acc) = name.strip_suffix(".xml") {
                // The sidecar is written after the XML: an XML without one
                // was interrupted and is downloaded again.
                if file.path().with_extension("meta.txt").exists() {
                    found.insert(acc.to_string());
                }
            }
        }
    }
    found
}

enum Saved {
    /// Saved and parsed: a one-line description, and the saved XML file
    /// (none for proxy headers).
    Parsed(String, Option<PathBuf>),
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
        Ok(line) => Saved::Parsed(line, Some(xml_path)),
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
    let mut waits = INDEX_RETRY_WAITS.iter();
    let body = loop {
        match client.get_text(&url) {
            Ok(body) => break body,
            Err(e) if is_transient(&e) => match waits.next() {
                Some(&secs) => {
                    eprintln!("{date}: {e}; trying again in {secs} s");
                    std::thread::sleep(std::time::Duration::from_secs(secs));
                }
                None => return Err(e.to_string()),
            },
            Err(EdgarError::Status {
                code: 403 | 404, ..
            }) => {
                return Err(format!(
                    "no daily index for {date} (weekend, holiday, or not published yet): {url}"
                ))
            }
            Err(e) => return Err(e.to_string()),
        }
    };
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
    each_day(&args, |day| daily_one(&client, &args, day, kind))
}

fn daily_one(client: &EdgarClient, args: &DailyArgs, day: Date, kind: Kind) -> Result<(), String> {
    let index = fetch_daily_index(client, day)?;
    let forms = kind.forms(args.include_amendments);
    let filings = index.unique_filings(forms);
    println!(
        "{}: {} rows in the index, {} {} filing(s) ({})",
        day,
        index.entries.len(),
        filings.len(),
        kind.label(),
        forms.join(", ")
    );

    let known = saved_accessions(&args.out);
    let take = args.limit.unwrap_or(usize::MAX);
    let mut skipped = 0usize;
    let mut failed = 0usize;
    let mut todo = Vec::new();
    for entry in filings.into_iter().take(take) {
        match entry.accession() {
            None => failed += 1,
            Some(a) if known.contains(a) => skipped += 1,
            Some(a) => todo.push((a.to_string(), entry.url())),
        }
    }
    let (saved, more_failed) = download_all(&todo, |url| save_filing(client, url, &args.out, kind));
    failed += more_failed;
    println!("done: {saved} saved, {skipped} already present, {failed} failed");
    Ok(())
}

/// Parallel downloads per day. One request waits mostly on the network
/// (about 1.3 filings a second measured one at a time), so several run at
/// once; the client's shared rate limiter still spaces every request to
/// 8 a second in total (EDGAR fair access).
const DOWNLOAD_THREADS: usize = 6;

/// Saves every `(accession, url)` with `save`, on [`DOWNLOAD_THREADS`]
/// threads. Returns (saved, failed). Order of the printed lines may vary.
fn download_all(
    todo: &[(String, String)],
    save: impl Fn(&str) -> Result<Saved, String> + Sync,
) -> (usize, usize) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let next = AtomicUsize::new(0);
    let saved = AtomicUsize::new(0);
    let failed = AtomicUsize::new(0);
    std::thread::scope(|scope| {
        for _ in 0..DOWNLOAD_THREADS.min(todo.len()) {
            scope.spawn(|| loop {
                let i = next.fetch_add(1, Ordering::Relaxed);
                let Some((accession, url)) = todo.get(i) else {
                    break;
                };
                match save(url) {
                    Ok(Saved::Parsed(line, _)) => {
                        println!("  {line}");
                        saved.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(Saved::ParserFailed(msg)) => {
                        eprintln!("  {msg}");
                        saved.fetch_add(1, Ordering::Relaxed);
                    }
                    Ok(Saved::NoOwnershipXml(msg)) => {
                        eprintln!("  {msg}");
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                    Err(e) => {
                        eprintln!("  {accession}: {e}");
                        failed.fetch_add(1, Ordering::Relaxed);
                    }
                }
            });
        }
    });
    (saved.into_inner(), failed.into_inner())
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
    each_day(&args, |day| daily_proxy_one(&client, &args, day))
}

fn daily_proxy_one(client: &EdgarClient, args: &DailyArgs, day: Date) -> Result<(), String> {
    let index = fetch_daily_index(client, day)?;
    let filings = index.unique_filings(oe_events::proxy_contest::CONTEST_FORMS);
    println!(
        "{}: {} rows in the index, {} contested-solicitation filing(s)",
        day,
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
        match save_proxy_header(client, entry.cik, accession, &args.out) {
            Ok((line, _)) => {
                println!("  {line}");
                saved += 1;
                known.insert(accession.to_string());
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

/// Downloads one filing's SGML header (under 1 KB) and saves it under
/// `root/<subject CIK>/<accession>.hdr.sgml`. `cik` is any party to the
/// filing (the archive lists it under each). Returns a one-line description.
fn save_proxy_header(
    client: &EdgarClient,
    cik: u64,
    accession: &str,
    root: &Path,
) -> Result<(String, PathBuf), String> {
    let url = format!(
        "{}{accession}.hdr.sgml",
        filing_folder_url(cik, &accession.replace('-', ""))
    );
    let text = client.get_text(&url).map_err(|e| e.to_string())?;
    let h = oe_edgar::parse_header_sgml(&text).map_err(|e| e.to_string())?;
    let subject = h
        .subject
        .as_ref()
        .ok_or("the header names no subject company")?;
    let dir = root.join(subject.cik.to_string());
    std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    let path = dir.join(format!("{accession}.hdr.sgml"));
    std::fs::write(&path, &text).map_err(|e| format!("{}: {e}", path.display()))?;
    let filers: Vec<&str> = h.filed_by.iter().map(|p| p.name.as_str()).collect();
    let line = format!(
        "{accession}  {:<8} {:<34.34} filed by {}",
        h.form,
        subject.name,
        filers.join("; ")
    );
    Ok((line, path))
}

// ---------------------------------------------------------------------------
// oe follow

/// Days of filings an alert pass reads: the cluster-buy window (10 days) and
/// the alert freshness (3 days) with room to spare. Older history, where a
/// rule needs it, comes from the ledger (`--db`).
const RECENT_DAYS: i32 = 14;

/// Each saved filing's acceptance date, built once when `follow --alert`
/// starts and kept up to date, so an alert pass reads only recent filings.
/// Measured with the full history on disk (2026-11-23): one pass over all of
/// `data/form144` took 90 s, `data/sched13` 47 s, `data/form4` 15 s.
#[derive(Debug, Default)]
struct RecentIndex {
    known_on: std::collections::BTreeMap<PathBuf, Date>,
}

impl RecentIndex {
    fn build(args: &FollowArgs) -> RecentIndex {
        let mut index = RecentIndex::default();
        for kind in &args.kinds {
            let folder = args.folder(*kind);
            let found = match kind {
                LiveKind::Proxy => crate::find_files(&folder, "sgml"),
                _ => crate::find_xml_files(&folder),
            };
            index.add(&found.unwrap_or_default());
        }
        index
    }

    fn add(&mut self, files: &[PathBuf]) {
        for path in files {
            if let Some(day) = known_on_of(path) {
                self.known_on.insert(path.clone(), day);
            }
        }
    }

    /// The filings accepted on or after `today - days`.
    fn recent(&self, today: Date, days: i32) -> std::collections::BTreeSet<PathBuf> {
        let from = today.add_days(-days);
        self.known_on
            .iter()
            .filter(|(_, d)| **d >= from)
            .map(|(p, _)| p.clone())
            .collect()
    }
}

/// A saved filing's US Eastern acceptance date: from its `.meta.txt`, or for
/// a contest filing from its SGML header.
fn known_on_of(path: &Path) -> Option<Date> {
    if path.to_string_lossy().ends_with(".hdr.sgml") {
        let text = std::fs::read_to_string(path).ok()?;
        return oe_edgar::parse_header_sgml(&text).ok()?.known_on();
    }
    crate::read_meta(path).known_on
}

pub fn follow(args: &[String]) -> Result<(), String> {
    let args = parse_follow_args(args)?;
    let client = client()?;
    let mut recent = if args.alert {
        let started = std::time::Instant::now();
        let index = RecentIndex::build(&args);
        println!(
            "indexed {} saved filing(s) in {} s; each alert pass reads those of the last {RECENT_DAYS} days",
            index.known_on.len(),
            started.elapsed().as_secs()
        );
        Some(index)
    } else {
        None
    };
    let mut known: Vec<HashSet<String>> = args
        .kinds
        .iter()
        .map(|k| saved_accessions(&args.folder(*k)))
        .collect();
    let names: Vec<String> = args
        .kinds
        .iter()
        .zip(&known)
        .map(|(k, seen)| {
            format!(
                "{} ({} saved in {})",
                k.name(),
                seen.len(),
                args.folder(*k).display()
            )
        })
        .collect();
    println!(
        "following the EDGAR latest-filings feeds every {}s: {}",
        args.interval_secs,
        names.join(", ")
    );

    let mut poll = 0u32;
    loop {
        poll += 1;
        let mut summary = Vec::new();
        let mut changed = Vec::new();
        let mut saved_files = Vec::new();
        for (kind, seen) in args.kinds.iter().zip(known.iter_mut()) {
            match poll_kind(&client, *kind, &args, seen) {
                Ok((new, files)) => {
                    summary.push(format!("{} new {}", new, kind.name()));
                    if new > 0 {
                        changed.push(*kind);
                    }
                    saved_files.extend(files);
                }
                // A failed poll (the feed does time out now and then) is not fatal.
                Err(e) => summary.push(format!("{} failed ({e})", kind.name())),
            }
        }
        println!("poll {poll}: {}", summary.join(", "));
        if args.db_history && !saved_files.is_empty() {
            load_into_ledger(&saved_files);
        }
        if let Some(index) = recent.as_mut() {
            index.add(&saved_files);
            if !changed.is_empty() {
                let today =
                    oe_alerts::select::eastern_date_from_unix(oe_alerts::select::now_unix());
                let only = std::sync::Arc::new(index.recent(today, RECENT_DAYS));
                alert_pass(&args, &changed, only);
            }
        }
        if args.polls.is_some_and(|max| poll >= max) {
            return Ok(());
        }
        if args
            .until
            .is_some_and(|t| oe_alerts::select::now_unix() as i64 >= t)
        {
            println!("stopped: --until reached");
            return Ok(());
        }
        std::thread::sleep(Duration::from_secs(args.interval_secs));
    }
}

/// Loads the filings just saved (every kind) into the PostgreSQL ledger
/// (`--db`). A failure is reported, never fatal; `oe db-load` can load them
/// later.
#[cfg(feature = "db")]
fn load_into_ledger(files: &[PathBuf]) {
    match crate::dbcmd::load_paths(files) {
        Ok(r) => println!(
            "database: {} new Form 3/4/5 filing(s), {} new row(s), amendments {} loaded, {} waiting; {} new other filing(s)",
            r.ownership.filings_new,
            r.ownership.rows_new,
            r.ownership.links.loaded,
            r.ownership.links.waiting,
            r.other.filings_new
        ),
        Err(e) => eprintln!("database load failed ({e}); run `oe db-load` later"),
    }
}

#[cfg(not(feature = "db"))]
fn load_into_ledger(_files: &[PathBuf]) {
    eprintln!("this build has no database support; --db cannot load filings");
}

/// `oe alerts` over the folders that got new filings in this poll. A failure
/// is reported, never fatal: following the feeds matters more than one run.
fn alert_pass(
    args: &FollowArgs,
    changed: &[LiveKind],
    only: std::sync::Arc<std::collections::BTreeSet<PathBuf>>,
) {
    let started = std::time::Instant::now();
    let mut sources = crate::alerts::Sources {
        only: Some(only),
        ..crate::alerts::Sources::default()
    };
    for kind in changed {
        let dir = Some(args.folder(*kind));
        match kind {
            LiveKind::Form4 => sources.form4 = dir,
            LiveKind::Form144 => sources.form144 = dir,
            LiveKind::Schedule13 => sources.sched13 = dir,
            LiveKind::Proxy => sources.proxy = dir,
        }
    }
    let alert_args = crate::alerts::AlertArgs {
        sources,
        watchlist: args.watchlist.clone(),
        selection: oe_alerts::select::Selection::default(),
        state: PathBuf::from(crate::alerts::DEFAULT_STATE),
        dry_run: false,
        today: None,
        db_history: args.db_history,
    };
    match crate::alerts::run(&alert_args) {
        Ok(r) => println!(
            "alerts: {} delivered, {} failed ({} ms)",
            r.delivered,
            r.failed,
            started.elapsed().as_millis()
        ),
        Err(e) => eprintln!("alerts failed: {e}"),
    }
}

/// One poll of one kind's feeds. Returns the number of new filings handled
/// and the XML files saved.
fn poll_kind(
    client: &EdgarClient,
    kind: LiveKind,
    args: &FollowArgs,
    known: &mut HashSet<String>,
) -> Result<(usize, Vec<PathBuf>), String> {
    let forms = kind.forms(args.include_amendments);
    let root = args.folder(kind);
    let mut new = 0usize;
    let mut saved = Vec::new();
    for prefix in kind.type_prefixes() {
        let url = oe_edgar::latest_filings_url(prefix, kind == LiveKind::Form4);
        let body = client.get_text(&url).map_err(|e| e.to_string())?;
        let entries = parse_atom_feed(&body).map_err(|e| e.to_string())?;
        let fresh: Vec<_> = unique_accessions(&entries, forms)
            .into_iter()
            .filter(|e| !known.contains(&e.accession))
            .collect();
        new += fresh.len();
        // The feed is newest first; handle the oldest new filing first.
        for entry in fresh.iter().rev() {
            let (seen, path) = save_feed_entry(client, kind, entry, &root);
            if seen {
                known.insert(entry.accession.clone());
            }
            saved.extend(path);
        }
    }
    Ok((new, saved))
}

/// Saves one new feed entry. Returns whether it counts as seen (a download
/// error does not, so the next poll tries again) and the XML file saved, if
/// it parsed.
fn save_feed_entry(
    client: &EdgarClient,
    kind: LiveKind,
    entry: &oe_edgar::FeedEntry,
    root: &Path,
) -> (bool, Option<PathBuf>) {
    let saved = match kind {
        LiveKind::Proxy => save_proxy_header(client, entry.cik, &entry.accession, root)
            .map(|(line, path)| Saved::Parsed(line, Some(path))),
        _ => {
            let url = format!(
                "https://www.sec.gov/Archives/edgar/data/{}/{}.txt",
                entry.cik, entry.accession
            );
            let k = match kind {
                LiveKind::Form144 => Kind::Form144,
                LiveKind::Schedule13 => Kind::Schedule13,
                _ => Kind::Ownership,
            };
            save_filing(client, &url, root, k)
        }
    };
    match saved {
        Ok(Saved::Parsed(line, path)) => {
            println!("  {line}  (feed time {})", entry.updated);
            (true, path)
        }
        Ok(Saved::ParserFailed(msg)) | Ok(Saved::NoOwnershipXml(msg)) => {
            eprintln!("  {msg}");
            (true, None)
        }
        Err(e) => {
            eprintln!("  {}: {e}", entry.accession);
            (false, None)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_failed_day_does_not_stop_the_range() {
        let d = |s: &str| s.parse::<Date>().unwrap();
        let mut seen = Vec::new();
        // 2023-11-06 (Mon) .. 2023-11-10 (Fri): a holiday and a server error.
        let r = each_day_between(d("2023-11-06"), Some(d("2023-11-10")), |day| {
            seen.push(day);
            match day.to_string().as_str() {
                "2023-11-07" => Err("HTTP 503 from https://www.sec.gov/x".into()),
                "2023-11-10" => Err("no daily index for 2023-11-10".into()),
                _ => Ok(()),
            }
        });
        assert_eq!(seen.len(), 5);
        let e = r.unwrap_err();
        assert!(e.starts_with("1 day(s) failed"), "{e}");
        assert!(e.ends_with("2023-11-07"), "{e}");
        // A single day keeps its own error.
        let one = each_day_between(d("2023-11-07"), None, |_| Err("HTTP 503".into()));
        assert_eq!(one.unwrap_err(), "HTTP 503");
    }

    #[test]
    fn busy_and_dropped_are_transient_missing_is_not() {
        let status = |code| EdgarError::Status {
            url: "u".into(),
            code,
        };
        assert!(is_transient(&status(503)));
        assert!(is_transient(&status(429)));
        assert!(!is_transient(&status(404)));
        assert!(!is_transient(&status(403)));
        assert!(is_transient(&EdgarError::Transport {
            url: "u".into(),
            message: "reset".into()
        }));
    }

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
        assert_eq!(
            Kind::Ownership.forms(true),
            ["3", "3/A", "4", "4/A", "5", "5/A"]
        );
        assert_eq!(Kind::Form144.forms(false), ["144"]);
        assert!(parse_daily_args(&args(&["yesterday"]), DEFAULT_ROOT).is_err());
        assert!(parse_daily_args(&args(&["2026-13-01"]), DEFAULT_ROOT).is_err());
        assert!(parse_daily_args(&args(&["2026-11-18", "--limit", "0"]), DEFAULT_ROOT).is_err());
        assert!(parse_daily_args(&args(&["2026-11-18", "2026-11-19"]), DEFAULT_ROOT).is_err());
    }

    #[test]
    fn a_date_range_is_its_weekdays() {
        let a =
            parse_daily_args(&args(&["2026-11-13", "--to", "2026-11-23"]), DEFAULT_ROOT).unwrap();
        let d: Vec<String> = days(&a).iter().map(|d| d.to_string()).collect();
        // Friday 13th to Monday 23rd: the weekends of the 14th-15th and 21st-22nd drop out.
        assert_eq!(
            d,
            [
                "2026-11-13",
                "2026-11-16",
                "2026-11-17",
                "2026-11-18",
                "2026-11-19",
                "2026-11-20",
                "2026-11-23"
            ]
        );
        // One date, even a Saturday, is fetched as asked.
        let one = parse_daily_args(&args(&["2026-11-21"]), DEFAULT_ROOT).unwrap();
        assert_eq!(days(&one).len(), 1);
        assert!(
            parse_daily_args(&args(&["2026-11-20", "--to", "2026-11-19"]), DEFAULT_ROOT).is_err()
        );
        assert!(parse_daily_args(&args(&["2026-11-20", "--to"]), DEFAULT_ROOT).is_err());
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
        let a = parse_follow_args(&args(&["--until", "2026-11-23T23:00:00Z"])).unwrap();
        assert_eq!(a.until, oe_core::utc_to_unix("2026-11-23T23:00:00Z"));
        assert!(parse_follow_args(&args(&["--until", "6pm"])).is_err());
        assert!(parse_follow_args(&args(&["--until"])).is_err());
        assert!(!parse_follow_args(&[]).unwrap().alert);
        assert!(parse_follow_args(&args(&["--alert"])).unwrap().alert);
        let a = parse_follow_args(&args(&["--watchlist", "w.txt"])).unwrap();
        assert!(a.alert, "a watchlist implies --alert");
        assert_eq!(a.watchlist, Some(PathBuf::from("w.txt")));
        assert!(parse_follow_args(&args(&["--watchlist"])).is_err());
    }

    #[test]
    fn follow_polls_every_kind_by_default_and_forms_picks_some() {
        let a = parse_follow_args(&[]).unwrap();
        assert_eq!(a.kinds, LiveKind::ALL);
        assert_eq!(a.folder(LiveKind::Form144), PathBuf::from("data/form144"));
        assert_eq!(a.folder(LiveKind::Proxy), PathBuf::from("data/proxy"));

        let a = parse_follow_args(&args(&["--forms", "144, 13,144", "--root", "x"])).unwrap();
        assert_eq!(a.kinds, [LiveKind::Form144, LiveKind::Schedule13]);
        assert_eq!(
            a.folder(LiveKind::Schedule13),
            PathBuf::from("x").join("sched13")
        );

        // --out still names the Form 4 folder, and only that one.
        let a = parse_follow_args(&args(&["--out", "f4"])).unwrap();
        assert_eq!(a.folder(LiveKind::Form4), PathBuf::from("f4"));
        assert_eq!(a.folder(LiveKind::Form144), PathBuf::from("data/form144"));

        assert!(parse_follow_args(&args(&["--forms", "13F"])).is_err());
        assert!(parse_follow_args(&args(&["--forms", ","])).is_err());
        assert!(parse_follow_args(&args(&["--forms"])).is_err());
    }

    #[test]
    fn live_kinds_match_the_daily_commands() {
        assert_eq!(LiveKind::Form4.forms(false), ["3", "4", "5"]);
        assert_eq!(
            LiveKind::Schedule13.forms(false),
            ["SCHEDULE 13D", "SCHEDULE 13G"]
        );
        assert_eq!(LiveKind::Form144.forms(true), ["144", "144/A"]);
        // Every contest form is covered by one of the proxy feed prefixes.
        for form in oe_events::proxy_contest::CONTEST_FORMS {
            assert!(
                LiveKind::Proxy
                    .type_prefixes()
                    .iter()
                    .any(|p| form.starts_with(p)),
                "{form} is not polled"
            );
        }
        for kind in LiveKind::ALL {
            assert_eq!(LiveKind::parse(kind.name()), Some(kind));
        }
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
        std::fs::write(b.join("0000000002-26-000007.meta.txt"), "x").unwrap();
        // Interrupted between the XML and its sidecar: not counted as saved.
        std::fs::write(b.join("0000000002-26-000008.xml"), "x").unwrap();
        std::fs::write(b.join("0000000002-26-000009.hdr.sgml"), "x").unwrap();
        std::fs::write(root.join("stray.xml"), "x").unwrap();
        let found = saved_accessions(&root);
        std::fs::remove_dir_all(&root).unwrap();
        assert_eq!(found.len(), 3);
        assert!(found.contains("0000000001-26-000001"));
        assert!(found.contains("0000000002-26-000007"));
        assert!(found.contains("0000000002-26-000009"));
        assert!(!found.contains("0000000002-26-000008"));
        assert!(saved_accessions(Path::new("definitely/not/here")).is_empty());
    }

    #[test]
    fn the_recent_index_keeps_only_the_last_days_and_takes_new_files() {
        let root = std::env::temp_dir().join(format!("oe-recent-{}", std::process::id()));
        let dir = root.join("form4").join("0000000001");
        std::fs::create_dir_all(&dir).unwrap();
        let file = |acc: &str, known_on: &str| {
            let xml = dir.join(format!("{acc}.xml"));
            std::fs::write(&xml, "x").unwrap();
            std::fs::write(
                meta_path(&xml),
                format!("accession={acc}\nknown_on={known_on}\n"),
            )
            .unwrap();
            xml
        };
        let old = file("0000000001-26-000001", "2026-11-01");
        let edge = file("0000000001-26-000002", "2026-11-09");
        let new = file("0000000001-26-000003", "2026-11-20");
        let args = FollowArgs {
            kinds: vec![LiveKind::Form4],
            root: root.clone(),
            ..parse_follow_args(&[]).unwrap()
        };
        let mut index = RecentIndex::build(&args);
        assert_eq!(index.known_on.len(), 3);
        let today = Date::from_ymd(2026, 11, 23).unwrap();
        let recent = index.recent(today, RECENT_DAYS);
        assert_eq!(recent.iter().collect::<Vec<_>>(), [&edge, &new]);
        assert!(!recent.contains(&old));
        // A filing saved by a later poll joins.
        let later = file("0000000001-26-000004", "2026-11-23");
        index.add(std::slice::from_ref(&later));
        assert!(index.recent(today, RECENT_DAYS).contains(&later));
        // The loaders read only the files given, without walking the folder.
        let only: std::collections::BTreeSet<PathBuf> = [new.clone()].into();
        let loaded = crate::load::insider_trades(&root.join("form4"), Some(&only)).unwrap();
        assert_eq!(loaded.files, 1);
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn download_all_does_each_item_once_and_counts() {
        use std::sync::Mutex;
        let todo: Vec<(String, String)> = (0..50)
            .map(|i| (format!("acc-{i}"), format!("url-{i}")))
            .collect();
        let seen = Mutex::new(Vec::new());
        let (saved, failed) = download_all(&todo, |url| {
            seen.lock().unwrap().push(url.to_string());
            if url.ends_with('7') {
                Err("boom".into())
            } else {
                Ok(Saved::Parsed(url.to_string(), None))
            }
        });
        let mut seen = seen.into_inner().unwrap();
        seen.sort();
        seen.dedup();
        assert_eq!(seen.len(), 50, "every item, once");
        // url-7, url-17, url-27, url-37, url-47 fail.
        assert_eq!((saved, failed), (45, 5));
        assert_eq!(download_all(&[], |_| unreachable!()), (0, 0));
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
