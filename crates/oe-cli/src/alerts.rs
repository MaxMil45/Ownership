//! `oe alerts`: run every event rule over the downloaded folders, keep the
//! fresh events that are on the watchlist, and deliver each one once to every
//! configured destination (Discord, a JSON webhook), or print them.
//!
//! What was delivered is appended to a log (`data/alerts/sent.tsv`), so
//! running the command again (from a scheduler, or after each `oe follow`
//! poll) delivers only what is new. ADR 0006.

use super::load;
use oe_alerts::select::{eastern_date_from_unix, now_unix, select, utc_timestamp, Selection};
use oe_alerts::sent::SentLog;
use oe_alerts::watchlist::Watchlist;
use oe_alerts::{convert, render, Alert};
use oe_core::Date;
use std::path::{Path, PathBuf};

pub const DEFAULT_STATE: &str = "data/alerts/sent.tsv";

/// The folders to read. `None` means that source is not used.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Sources {
    pub form4: Option<PathBuf>,
    pub form144: Option<PathBuf>,
    pub sched13: Option<PathBuf>,
    pub proxy: Option<PathBuf>,
    pub periodic: Option<PathBuf>,
    /// When set, only these files are read (and the folders are not walked):
    /// `oe follow --alert` passes the filings of the last days, so an alert
    /// pass does not reread every filing ever downloaded.
    pub only: Option<std::sync::Arc<std::collections::BTreeSet<PathBuf>>>,
}

impl Sources {
    /// The default download folders that exist.
    pub fn defaults() -> Sources {
        let existing = |p: &str| Some(PathBuf::from(p)).filter(|p| p.is_dir());
        Sources {
            form4: existing("data/form4"),
            form144: existing("data/form144"),
            sched13: existing("data/sched13"),
            proxy: existing("data/proxy"),
            periodic: existing("data/periodic"),
            only: None,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct AlertArgs {
    pub sources: Sources,
    pub watchlist: Option<PathBuf>,
    pub selection: Selection,
    pub state: PathBuf,
    pub dry_run: bool,
    /// Pretend today is this date (US Eastern), to replay old data.
    pub today: Option<Date>,
    /// Give the routine-trader filter the ledger's history (`--db`).
    pub db_history: bool,
}

pub fn parse_args(args: &[String]) -> Result<AlertArgs, String> {
    let mut parsed = AlertArgs {
        sources: Sources::defaults(),
        watchlist: None,
        selection: Selection::default(),
        state: PathBuf::from(DEFAULT_STATE),
        dry_run: false,
        today: None,
        db_history: false,
    };
    let mut only: Option<Sources> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if source_flag(arg, &mut it, &mut only)? {
            continue;
        }
        match arg.as_str() {
            "--watchlist" => parsed.watchlist = Some(super::args::file(&mut it, "--watchlist")?),
            "--state" => parsed.state = super::args::file(&mut it, "--state")?,
            "--max-age" => {
                parsed.selection.max_age_days =
                    super::args::value(&mut it, "--max-age", "a number of days", |_| true)?
            }
            "--include-undated" => parsed.selection.include_undated = true,
            "--dry-run" => parsed.dry_run = true,
            "--db" => parsed.db_history = true,
            "--today" => {
                let text = it.next().ok_or("--today needs a date such as 2026-11-20")?;
                parsed.today = Some(super::args::date(text, "2026-11-20")?)
            }
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    // Naming any folder means: only the folders named.
    if let Some(only) = only {
        parsed.sources = only;
    }
    Ok(parsed)
}

/// Reads one `--form4 <dir>`-style flag into `only` (the folders named so
/// far). Returns false if `arg` is not a folder flag. Shared by `oe alerts`
/// and `oe dashboard`.
pub fn source_flag(
    arg: &str,
    it: &mut super::args::Args,
    only: &mut Option<Sources>,
) -> Result<bool, String> {
    let field: fn(&mut Sources) -> &mut Option<PathBuf> = match arg {
        "--form4" => |s| &mut s.form4,
        "--form144" => |s| &mut s.form144,
        "--sched13" => |s| &mut s.sched13,
        "--proxy" => |s| &mut s.proxy,
        "--periodic" => |s| &mut s.periodic,
        _ => return Ok(false),
    };
    let dir = super::args::folder(it, arg)?;
    *field(only.get_or_insert_with(Sources::default)) = Some(dir);
    Ok(true)
}

/// Every alert the rules find in `sources`, with a count per source for the
/// summary line.
/// With `db_history`, the cluster-buy routine-trader filter also sees the
/// ledger's earlier buys and sales of the same issuers (`DATABASE_URL`);
/// without it, only the downloaded files.
pub fn build_alerts(
    sources: &Sources,
    db_history: bool,
) -> Result<(Vec<Alert>, Vec<String>), String> {
    let mut alerts = Vec::new();
    let mut summary = Vec::new();
    let only = sources.only.as_deref();
    if let Some(dir) = &sources.form4 {
        let trades = load::insider_trades(dir, only)?;
        let params = oe_events::ClusterBuyParams::default();
        let mut history = trades.items.clone();
        let mut from_db = 0usize;
        if db_history && !trades.items.is_empty() {
            let earlier = ledger_history(&trades.items, params.routine_lookback_years)?;
            from_db = earlier.len();
            history.extend(earlier);
        }
        let events = oe_events::detect_cluster_buys(&trades.items, &history, &params);
        summary.push(format!(
            "Form 4: {} file(s), {} cluster buy(s){}",
            trades.files,
            events.len(),
            if db_history {
                format!(" (routine-trader filter saw {from_db} earlier trade(s) from the database)")
            } else {
                String::new()
            }
        ));
        alerts.extend(events.iter().map(convert::cluster_buy));
    }
    if let Some(dir) = &sources.form144 {
        let loaded = load::form144(dir, only)?;
        let params = oe_events::PlannedSaleParams::default();
        let (events, note) = if db_history {
            let earlier = ledger_notices(&loaded.items)?;
            let note = format!(
                " (planned_sale@2 saw {} earlier notice(s) from the database)",
                earlier.len()
            );
            (
                oe_events::detect_planned_sales_with_ledger(&loaded.items, &params, &earlier),
                note,
            )
        } else {
            (
                oe_events::detect_planned_sales(&loaded.items, &params),
                String::new(),
            )
        };
        summary.push(format!(
            "Form 144: {} file(s), {} planned sale(s){note}",
            loaded.files,
            events.len()
        ));
        alerts.extend(events.iter().map(convert::planned_sale));
    }
    if let Some(dir) = &sources.sched13 {
        let loaded = load::schedule13(dir, only)?;
        let params = oe_events::ActivistParams::default();
        let (events, note) = if db_history {
            let earlier = ledger_13gs(&loaded.items)?;
            let note = format!(
                " (passive_to_active@2 saw {} earlier 13G(s) from the database)",
                earlier.len()
            );
            (
                oe_events::detect_activist_events_with_ledger(&loaded.items, &params, &earlier),
                note,
            )
        } else {
            (
                oe_events::detect_activist_events(&loaded.items, &params),
                String::new(),
            )
        };
        summary.push(format!(
            "13D/G: {} file(s), {} activist event(s){note}",
            loaded.files,
            events.len()
        ));
        alerts.extend(events.iter().map(convert::activist));
    }
    if let Some(dir) = &sources.proxy {
        let loaded = load::contest_filings(dir, only)?;
        let (events, note) = if db_history {
            let earlier = ledger_contests(&loaded.items)?;
            let note = format!(
                " (proxy_contest@2 saw {} contest filing(s) from the database)",
                earlier.len()
            );
            (
                oe_events::detect_proxy_contests_with_ledger(&loaded.items, &earlier),
                note,
            )
        } else {
            (
                oe_events::detect_proxy_contests(&loaded.items),
                String::new(),
            )
        };
        summary.push(format!(
            "proxy: {} header(s), {} contest(s){note}",
            loaded.files,
            events.len()
        ));
        alerts.extend(events.iter().map(convert::proxy_contest));
    }
    if let Some(dir) = &sources.periodic {
        let (loaded, _) = load::plan_termination_events(dir, db_history)?;
        let note = if db_history {
            let known = loaded
                .items
                .iter()
                .filter(|e| e.plan_life.is_some())
                .count();
            format!(" (plan_terminated@2 knows the adoption of {known})")
        } else {
            String::new()
        };
        summary.push(format!(
            "10-Q/10-K: {} report(s), {} plan termination(s){note}",
            loaded.files,
            loaded.items.len()
        ));
        alerts.extend(loaded.items.iter().map(convert::plan_terminated));
    }
    Ok((alerts, summary))
}

/// The ledger's open-market trades of the issuers in `trades`, from `years`
/// + 1 years before the oldest of them, as history-only `InsiderTrade`s.
#[cfg(feature = "db")]
fn ledger_history(
    trades: &[oe_events::InsiderTrade],
    years: u32,
) -> Result<Vec<oe_events::InsiderTrade>, String> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "--db needs DATABASE_URL (see `oe db-status`)".to_string())?;
    let mut issuers: Vec<u64> = trades.iter().map(|t| t.issuer.0).collect();
    issuers.sort_unstable();
    issuers.dedup();
    let oldest = trades.iter().map(|t| t.date).min().expect("not empty");
    let since = oldest.add_days(-366 * (years as i32 + 1));
    let mut client = oe_store::connect(&url).map_err(|e| e.to_string())?;
    let rows = oe_store::trade_history(&mut client, &issuers, &since.to_string())
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(oe_events::InsiderTrade {
                issuer: oe_core::Cik(r.issuer_cik),
                ticker: None,
                owner: oe_core::Cik(r.owner_cik),
                owner_name: String::new(),
                co_owners: Vec::new(),
                is_director: false,
                is_officer: false,
                officer_title: None,
                date: r.date.parse().ok()?,
                code: oe_core::TransactionCode::parse(&r.code),
                // History only: never itself counted as a qualifying buy.
                is_open_market_purchase: false,
                shares: None,
                price_per_share: None,
                plan_10b5_1: None,
                from_amendment: false,
                known_on: None,
                accession: None,
            })
        })
        .collect())
}

/// The ledger's Schedule 13Gs for the issuers of these original 13Ds.
#[cfg(feature = "db")]
pub(crate) fn ledger_13gs(
    filings: &[(oe_sched13::Schedule13, oe_core::FilingMeta)],
) -> Result<Vec<oe_events::Earlier13G>, String> {
    let mut issuers: Vec<u64> = filings
        .iter()
        .filter(|(s, _)| s.schedule == oe_sched13::Schedule::D && !s.is_amendment())
        .map(|(s, _)| s.issuer_cik.0)
        .collect();
    issuers.sort_unstable();
    issuers.dedup();
    if issuers.is_empty() {
        return Ok(Vec::new());
    }
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "--db needs DATABASE_URL (see `oe db-status`)".to_string())?;
    let mut client = oe_store::connect(&url).map_err(|e| e.to_string())?;
    let rows = oe_store::schedule13g_history(&mut client, &issuers).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(oe_events::Earlier13G {
                issuer: oe_core::Cik(r.issuer_cik),
                accession: r.accession,
                known_on: r.known_on.parse().ok()?,
                filer: oe_core::Cik(r.filer_cik),
                holders: r.holders,
                largest_percent: r.largest_percent,
            })
        })
        .collect())
}

/// The ledger's original Form 144s about the companies in `filings`, from a
/// year before the oldest of them.
#[cfg(feature = "db")]
pub(crate) fn ledger_notices(
    filings: &[(oe_form144::Form144, oe_core::FilingMeta)],
) -> Result<Vec<oe_events::EarlierNotice>, String> {
    let mut issuers: Vec<u64> = filings.iter().map(|(f, _)| f.issuer_cik.0).collect();
    issuers.sort_unstable();
    issuers.dedup();
    let Some(oldest) = filings.iter().filter_map(|(_, m)| m.known_on).min() else {
        return Ok(Vec::new());
    };
    let since = oldest.add_days(-oe_events::planned_sale::HISTORY_DAYS);
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "--db needs DATABASE_URL (see `oe db-status`)".to_string())?;
    let mut client = oe_store::connect(&url).map_err(|e| e.to_string())?;
    let rows = oe_store::form144_history(&mut client, &issuers, &since.to_string())
        .map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(oe_events::EarlierNotice {
                issuer: oe_core::Cik(r.issuer_cik),
                seller: r.seller_name,
                accession: r.accession,
                known_on: r.known_on.parse().ok()?,
            })
        })
        .collect())
}

#[cfg(not(feature = "db"))]
pub(crate) fn ledger_notices(
    _filings: &[(oe_form144::Form144, oe_core::FilingMeta)],
) -> Result<Vec<oe_events::EarlierNotice>, String> {
    Err("this build has no database support; leave out --db".into())
}

/// The ledger's adopted Rule 10b5-1 arrangements at these companies.
#[cfg(feature = "db")]
pub(crate) fn ledger_adoptions(issuers: &[u64]) -> Result<Vec<oe_events::EarlierAdoption>, String> {
    if issuers.is_empty() {
        return Ok(Vec::new());
    }
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "--db needs DATABASE_URL (see `oe db-status`)".to_string())?;
    let mut client = oe_store::connect(&url).map_err(|e| e.to_string())?;
    let rows = oe_store::adoption_history(&mut client, issuers).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .filter_map(|r| {
            Some(oe_events::EarlierAdoption {
                issuer: oe_core::Cik(r.registrant_cik),
                person: r.person,
                adoption_date: r.adoption_date.and_then(|d| d.parse().ok()),
                accession: r.accession,
                known_on: r.known_on.parse().ok()?,
            })
        })
        .collect())
}

#[cfg(not(feature = "db"))]
pub(crate) fn ledger_adoptions(
    _issuers: &[u64],
) -> Result<Vec<oe_events::EarlierAdoption>, String> {
    Err("this build has no database support; leave out --db".into())
}

/// The ledger's contest filings about the companies in `filings`.
#[cfg(feature = "db")]
pub(crate) fn ledger_contests(
    filings: &[oe_events::ContestFiling],
) -> Result<Vec<oe_events::ContestFiling>, String> {
    let mut subjects: Vec<u64> = filings.iter().map(|f| f.subject.cik.0).collect();
    subjects.sort_unstable();
    subjects.dedup();
    if subjects.is_empty() {
        return Ok(Vec::new());
    }
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "--db needs DATABASE_URL (see `oe db-status`)".to_string())?;
    let mut client = oe_store::connect(&url).map_err(|e| e.to_string())?;
    let rows = oe_store::contest_history(&mut client, &subjects).map_err(|e| e.to_string())?;
    Ok(rows
        .into_iter()
        .map(|r| oe_events::ContestFiling {
            form: r.form,
            accession: r.accession,
            known_on: r.known_on.parse().ok(),
            subject: r.subject,
            filed_by: r.filed_by,
        })
        .collect())
}

#[cfg(not(feature = "db"))]
pub(crate) fn ledger_contests(
    _filings: &[oe_events::ContestFiling],
) -> Result<Vec<oe_events::ContestFiling>, String> {
    Err("this build has no database support; leave out --db".into())
}

#[cfg(not(feature = "db"))]
pub(crate) fn ledger_13gs(
    _filings: &[(oe_sched13::Schedule13, oe_core::FilingMeta)],
) -> Result<Vec<oe_events::Earlier13G>, String> {
    Err("this build has no database support; leave out --db".into())
}

#[cfg(not(feature = "db"))]
fn ledger_history(
    _trades: &[oe_events::InsiderTrade],
    _years: u32,
) -> Result<Vec<oe_events::InsiderTrade>, String> {
    Err("this build has no database support; leave out --db".into())
}

fn read_watchlist(path: &Path) -> Result<Watchlist, String> {
    let text = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let w = Watchlist::parse(&text).map_err(|e| format!("{}: {e}", path.display()))?;
    if w.is_empty() {
        return Err(format!(
            "{} has no entries; leave out --watchlist to get every alert",
            path.display()
        ));
    }
    Ok(w)
}

/// What one run did.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct RunReport {
    pub delivered: usize,
    pub failed: usize,
}

/// Selects and delivers. Destinations come from the environment
/// (`OE_DISCORD_WEBHOOK_URL`, `OE_ALERT_WEBHOOK_URL`); with none, alerts are
/// printed and recorded as delivered to `console`.
pub fn run(args: &AlertArgs) -> Result<RunReport, String> {
    let watchlist = args.watchlist.as_deref().map(read_watchlist).transpose()?;
    let (alerts, summary) = build_alerts(&args.sources, args.db_history)?;
    for line in &summary {
        println!("{line}");
    }
    let today = args
        .today
        .unwrap_or_else(|| eastern_date_from_unix(now_unix()));
    let mut log = if args.dry_run {
        SentLog::in_memory()
    } else {
        SentLog::open(&args.state)?
    };

    #[cfg(feature = "net")]
    let destinations = oe_alerts::send::destinations_from_env()?;
    #[cfg(not(feature = "net"))]
    let destinations: Vec<()> = Vec::new();

    let mut report = RunReport::default();
    if destinations.is_empty() {
        deliver_to_console(
            &alerts,
            watchlist.as_ref(),
            &mut log,
            today,
            args,
            &mut report,
        )?;
    }
    #[cfg(feature = "net")]
    {
        let sender = oe_alerts::send::Sender::new();
        for to in &destinations {
            let (chosen, skipped) = select(
                &alerts,
                watchlist.as_ref(),
                &log,
                to.name(),
                today,
                &args.selection,
            );
            println!(
                "{}: {} alert(s) to send ({})",
                to.name(),
                chosen.len(),
                describe(&skipped, args.selection.max_age_days)
            );
            for (i, a) in chosen.iter().enumerate() {
                if args.dry_run {
                    println!("  would send: {}", a.headline);
                    continue;
                }
                if i > 0 {
                    std::thread::sleep(oe_alerts::send::PACE);
                }
                match sender.send(to, a) {
                    Ok(()) => {
                        let now = now_unix();
                        log.record(to.name(), &a.key, &utc_timestamp(now))?;
                        super::delays::record(&args.state, now, to.name(), a)?;
                        report.delivered += 1;
                        println!("  sent: {}", a.headline);
                    }
                    Err(e) => {
                        report.failed += 1;
                        eprintln!("  not sent ({e}), will retry next run: {}", a.headline);
                    }
                }
            }
        }
    }
    if args.dry_run {
        println!("dry run: nothing was sent or recorded");
    }
    Ok(report)
}

/// "2 already delivered, 69 older than 3 day(s), ..." for the summary line.
fn describe(s: &oe_alerts::select::Skipped, max_age_days: i32) -> String {
    format!(
        "{} already delivered, {} older than {max_age_days} day(s), {} undated, {} not on the watchlist, {} repeated in this run",
        s.already_sent, s.too_old, s.undated, s.not_watched, s.repeated
    )
}

fn deliver_to_console(
    alerts: &[Alert],
    watchlist: Option<&Watchlist>,
    log: &mut SentLog,
    today: Date,
    args: &AlertArgs,
    report: &mut RunReport,
) -> Result<(), String> {
    let (chosen, skipped) = select(alerts, watchlist, log, "console", today, &args.selection);
    println!(
        "{} new alert(s) ({}); no OE_DISCORD_WEBHOOK_URL or OE_ALERT_WEBHOOK_URL set, so printing them",
        chosen.len(),
        describe(&skipped, args.selection.max_age_days)
    );
    for a in chosen {
        println!("\n{}", render::text(a).trim_end());
        if !args.dry_run {
            let now = now_unix();
            log.record("console", &a.key, &utc_timestamp(now))?;
            super::delays::record(&args.state, now, "console", a)?;
        }
        report.delivered += 1;
    }
    Ok(())
}

pub fn alerts(args: &[String]) -> Result<(), String> {
    let args = parse_args(args)?;
    let report = run(&args)?;
    if report.failed > 0 {
        return Err(format!(
            "{} alert(s) could not be delivered; they will be tried again next run",
            report.failed
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn naming_a_folder_uses_only_the_folders_named() {
        let p = parse_args(&a(&["--form4", "x/f4", "--proxy", "x/p", "--dry-run"])).unwrap();
        assert_eq!(
            p.sources,
            Sources {
                form4: Some("x/f4".into()),
                proxy: Some("x/p".into()),
                ..Sources::default()
            }
        );
        assert!(p.dry_run);
        assert_eq!(p.state, PathBuf::from(DEFAULT_STATE));
    }

    #[test]
    fn options() {
        let p = parse_args(&a(&[
            "--max-age",
            "7",
            "--today",
            "2026-11-19",
            "--include-undated",
            "--watchlist",
            "w.txt",
            "--state",
            "s.tsv",
        ]))
        .unwrap();
        assert_eq!(p.selection.max_age_days, 7);
        assert!(p.selection.include_undated);
        assert_eq!(p.today, Some("2026-11-19".parse().unwrap()));
        assert_eq!(p.watchlist, Some(PathBuf::from("w.txt")));
        assert_eq!(p.state, PathBuf::from("s.tsv"));
        assert!(parse_args(&a(&["--max-age"])).is_err());
        assert!(parse_args(&a(&["--today", "yesterday"])).is_err());
        assert!(parse_args(&a(&["--nope"])).is_err());
    }

    #[test]
    fn real_fixtures_become_alerts_with_receipts() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let sources = Sources {
            form144: Some(root.join("form144/real")),
            sched13: Some(root.join("sched13/real")),
            proxy: Some(root.join("proxy/real")),
            periodic: Some(root.join("item408/real")),
            ..Sources::default()
        };
        let (alerts, summary) = build_alerts(&sources, false).unwrap();
        assert_eq!(summary.len(), 4);
        for rule in [
            "insider.planned_sale@1",
            "activist.new_13d@1",
            "activist.proxy_contest@1",
            "insider.plan_terminated@1",
        ] {
            assert!(
                alerts.iter().any(|a| a.rule_id == rule),
                "no {rule} alert from the fixtures"
            );
        }
        for al in &alerts {
            assert!(!al.receipts.is_empty(), "{} has no receipt", al.key);
            assert!(al.receipts.iter().all(|r| r
                .url
                .starts_with("https://www.sec.gov/Archives/edgar/data/")));
        }
        // Keys are unique, except that ARS Pharmaceuticals tags two plans each
        // for two people in one report: one alert per person is intended.
        let mut seen = std::collections::HashMap::new();
        let mut repeated: Vec<&str> = Vec::new();
        for al in &alerts {
            if seen.insert(al.key.as_str(), ()).is_some() {
                repeated.push(al.key.as_str());
            }
        }
        repeated.sort_unstable();
        assert_eq!(
            repeated,
            [
                "insider.plan_terminated@1|0001193125-26-449778|Richard Lowenthal",
                "insider.plan_terminated@1|0001193125-26-449778|Sarina Tanimoto"
            ]
        );
    }
}
