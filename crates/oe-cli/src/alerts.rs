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
}

pub fn parse_args(args: &[String]) -> Result<AlertArgs, String> {
    let mut parsed = AlertArgs {
        sources: Sources::defaults(),
        watchlist: None,
        selection: Selection::default(),
        state: PathBuf::from(DEFAULT_STATE),
        dry_run: false,
        today: None,
    };
    let mut only: Option<Sources> = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        let mut set = |f: fn(&mut Sources) -> &mut Option<PathBuf>,
                       name: &str,
                       it: &mut std::slice::Iter<String>|
         -> Result<(), String> {
            let dir = super::args::folder(it, name)?;
            *f(only.get_or_insert_with(Sources::default)) = Some(dir);
            Ok(())
        };
        match arg.as_str() {
            "--form4" => set(|s| &mut s.form4, "--form4", &mut it)?,
            "--form144" => set(|s| &mut s.form144, "--form144", &mut it)?,
            "--sched13" => set(|s| &mut s.sched13, "--sched13", &mut it)?,
            "--proxy" => set(|s| &mut s.proxy, "--proxy", &mut it)?,
            "--periodic" => set(|s| &mut s.periodic, "--periodic", &mut it)?,
            "--watchlist" => parsed.watchlist = Some(super::args::file(&mut it, "--watchlist")?),
            "--state" => parsed.state = super::args::file(&mut it, "--state")?,
            "--max-age" => {
                parsed.selection.max_age_days =
                    super::args::value(&mut it, "--max-age", "a number of days", |_| true)?
            }
            "--include-undated" => parsed.selection.include_undated = true,
            "--dry-run" => parsed.dry_run = true,
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

/// Every alert the rules find in `sources`, with a count per source for the
/// summary line.
pub fn build_alerts(sources: &Sources) -> Result<(Vec<Alert>, Vec<String>), String> {
    let mut alerts = Vec::new();
    let mut summary = Vec::new();
    if let Some(dir) = &sources.form4 {
        let trades = load::insider_trades(dir)?;
        let events = oe_events::detect_cluster_buys(
            &trades.items,
            &trades.items,
            &oe_events::ClusterBuyParams::default(),
        );
        summary.push(format!(
            "Form 4: {} file(s), {} cluster buy(s)",
            trades.files,
            events.len()
        ));
        alerts.extend(events.iter().map(convert::cluster_buy));
    }
    if let Some(dir) = &sources.form144 {
        let loaded = load::form144(dir)?;
        let events = oe_events::detect_planned_sales(
            &loaded.items,
            &oe_events::PlannedSaleParams::default(),
        );
        summary.push(format!(
            "Form 144: {} file(s), {} planned sale(s)",
            loaded.files,
            events.len()
        ));
        alerts.extend(events.iter().map(convert::planned_sale));
    }
    if let Some(dir) = &sources.sched13 {
        let loaded = load::schedule13(dir)?;
        let events =
            oe_events::detect_activist_events(&loaded.items, &oe_events::ActivistParams::default());
        summary.push(format!(
            "13D/G: {} file(s), {} activist event(s)",
            loaded.files,
            events.len()
        ));
        alerts.extend(events.iter().map(convert::activist));
    }
    if let Some(dir) = &sources.proxy {
        let loaded = load::contest_filings(dir)?;
        let events = oe_events::detect_proxy_contests(&loaded.items);
        summary.push(format!(
            "proxy: {} header(s), {} contest(s)",
            loaded.files,
            events.len()
        ));
        alerts.extend(events.iter().map(convert::proxy_contest));
    }
    if let Some(dir) = &sources.periodic {
        let (loaded, _) = load::plan_termination_events(dir)?;
        summary.push(format!(
            "10-Q/10-K: {} report(s), {} plan termination(s)",
            loaded.files,
            loaded.items.len()
        ));
        alerts.extend(loaded.items.iter().map(convert::plan_terminated));
    }
    Ok((alerts, summary))
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
    let (alerts, summary) = build_alerts(&args.sources)?;
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
                        log.record(to.name(), &a.key, &utc_timestamp(now_unix()))?;
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
            log.record("console", &a.key, &utc_timestamp(now_unix()))?;
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
        let (alerts, summary) = build_alerts(&sources).unwrap();
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
