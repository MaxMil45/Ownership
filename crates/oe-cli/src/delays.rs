//! Alert delays: how long after the SEC accepted a filing its alert went out.
//!
//! Each delivery (`oe alerts`, `oe follow --alert`) appends a line to
//! `delivered.tsv` next to the sent log: when, where, which rule, and the
//! accession numbers of its receipts. `oe alert-delays` matches those with the
//! acceptance time each saved filing carries (its `.meta.txt`, or a contest
//! filing's SGML header) and reports two delays for one US Eastern day:
//!
//! * **feed**: acceptance to the filing being saved (`.meta.txt` written), for
//!   every filing accepted and saved that day;
//! * **alert**: acceptance of the alert's latest receipt to the delivery.

use oe_alerts::Alert;
use oe_core::Date;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

const DELIVERED_FILE: &str = "delivered.tsv";
/// Gate 1: alerts within five minutes of filing.
const TARGET_SECS: i64 = 300;

fn delivered_path(state: &Path) -> PathBuf {
    state
        .parent()
        .unwrap_or_else(|| Path::new(""))
        .join(DELIVERED_FILE)
}

fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

/// Appends one delivery: `utc<TAB>destination<TAB>rule<TAB>accessions<TAB>headline`.
pub fn record(state: &Path, at_unix: u64, destination: &str, a: &Alert) -> Result<(), String> {
    use std::io::Write;
    let path = delivered_path(state);
    if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    let accessions: Vec<&str> = a.receipts.iter().map(|r| r.accession.as_str()).collect();
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .map_err(|e| format!("{}: {e}", path.display()))?;
    writeln!(
        f,
        "{}\t{}\t{}\t{}\t{}",
        oe_alerts::select::utc_timestamp(at_unix),
        clean(destination),
        a.rule_id,
        accessions.join(","),
        clean(&a.headline)
    )
    .map_err(|e| format!("{}: {e}", path.display()))
}

/// One saved filing: when the SEC accepted it and when it was saved.
#[derive(Debug, Clone, PartialEq)]
struct Saved {
    kind: &'static str,
    accepted: i64,
    saved: i64,
}

fn mtime(path: &Path) -> Option<i64> {
    let t = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(t.duration_since(std::time::UNIX_EPOCH).ok()?.as_secs() as i64)
}

/// Every saved filing under `root/<kind>/`, by accession.
fn saved_filings(root: &Path) -> Result<HashMap<String, Saved>, String> {
    let mut out = HashMap::new();
    for kind in ["form4", "form144", "sched13"] {
        let dir = root.join(kind);
        if !dir.is_dir() {
            continue;
        }
        for path in super::find_files(&dir, "txt")? {
            if !path.to_string_lossy().ends_with(".meta.txt") {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let kv: HashMap<&str, &str> = oe_core::text::key_values(&text).collect();
            let (Some(acc), Some(accepted)) = (
                kv.get("accession"),
                kv.get("accepted_eastern")
                    .and_then(|t| oe_core::eastern_to_unix(t)),
            ) else {
                continue;
            };
            if let Some(saved) = mtime(&path) {
                out.insert(
                    acc.to_string(),
                    Saved {
                        kind,
                        accepted,
                        saved,
                    },
                );
            }
        }
    }
    let proxy = root.join("proxy");
    if proxy.is_dir() {
        for path in super::find_files(&proxy, "sgml")? {
            let Ok(text) = std::fs::read_to_string(&path) else {
                continue;
            };
            let Ok(h) = oe_edgar::parse_header_sgml(&text) else {
                continue;
            };
            if let (Some(accepted), Some(saved)) =
                (oe_core::eastern_to_unix(&h.accepted_eastern), mtime(&path))
            {
                out.insert(
                    h.accession,
                    Saved {
                        kind: "proxy",
                        accepted,
                        saved,
                    },
                );
            }
        }
    }
    Ok(out)
}

/// One delivery from `delivered.tsv`.
#[derive(Debug, Clone, PartialEq)]
struct Delivery {
    at: i64,
    destination: String,
    rule: String,
    accessions: Vec<String>,
    headline: String,
}

fn parse_deliveries(text: &str) -> Vec<Delivery> {
    text.lines()
        .filter_map(|l| {
            let c: Vec<&str> = l.splitn(5, '\t').collect();
            let [at, destination, rule, accessions, headline] = c[..] else {
                return None;
            };
            Some(Delivery {
                at: oe_core::utc_to_unix(at)?,
                destination: destination.to_string(),
                rule: rule.to_string(),
                accessions: accessions
                    .split(',')
                    .filter(|a| !a.is_empty())
                    .map(str::to_string)
                    .collect(),
                headline: headline.to_string(),
            })
        })
        .collect()
}

/// The US Eastern date of an instant.
fn eastern_day(unix: i64) -> Option<Date> {
    oe_core::us_eastern_date(&oe_alerts::select::utc_timestamp(unix.max(0) as u64))
}

/// Count, median, 90th percentile, maximum and share within the target.
#[derive(Debug, PartialEq)]
struct Stats {
    n: usize,
    median: i64,
    p90: i64,
    max: i64,
    within_target: usize,
}

fn stats(mut secs: Vec<i64>) -> Option<Stats> {
    if secs.is_empty() {
        return None;
    }
    secs.sort_unstable();
    let at = |q: f64| secs[((secs.len() - 1) as f64 * q).round() as usize];
    Some(Stats {
        n: secs.len(),
        median: at(0.5),
        p90: at(0.9),
        max: *secs.last().expect("not empty"),
        within_target: secs.iter().filter(|s| **s <= TARGET_SECS).count(),
    })
}

fn line(label: &str, s: &Stats) -> String {
    format!(
        "{label}: {} — median {}, 90% within {}, slowest {}; {} of {} ({:.0}%) within 5 minutes",
        s.n,
        mmss(s.median),
        mmss(s.p90),
        mmss(s.max),
        s.within_target,
        s.n,
        100.0 * s.within_target as f64 / s.n as f64
    )
}

fn mmss(secs: i64) -> String {
    if secs < 0 {
        return format!("-{}", mmss(-secs));
    }
    format!("{}m{:02}s", secs / 60, secs % 60)
}

/// `oe alert-delays [<YYYY-MM-DD>] [--since <UTC time>] [--data <folder>] [--state <sent.tsv>]`
pub fn alert_delays(args: &[String]) -> Result<(), String> {
    let mut day: Option<Date> = None;
    let mut since: Option<i64> = None;
    let mut root = PathBuf::from("data");
    let mut state = PathBuf::from(super::alerts::DEFAULT_STATE);
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--data" => root = super::args::folder(&mut it, "--data")?,
            "--state" => state = super::args::file(&mut it, "--state")?,
            "--since" => {
                let t = it
                    .next()
                    .ok_or("--since needs a UTC time such as 2026-11-23T14:25:00Z")?;
                since = Some(oe_core::utc_to_unix(t).ok_or(format!(
                    "--since: {t:?} is not a UTC time such as 2026-11-23T14:25:00Z"
                ))?);
            }
            other if day.is_none() => day = Some(super::args::date(other, "2026-11-23")?),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    let day = day.unwrap_or_else(|| {
        oe_alerts::select::eastern_date_from_unix(oe_alerts::select::now_unix())
    });
    let saved = saved_filings(&root)?;
    let report = report(day, since, &saved, &read_deliveries(&state)?);
    print!("{report}");
    Ok(())
}

fn read_deliveries(state: &Path) -> Result<Vec<Delivery>, String> {
    let path = delivered_path(state);
    match std::fs::read_to_string(&path) {
        Ok(t) => Ok(parse_deliveries(&t)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Vec::new()),
        Err(e) => Err(format!("{}: {e}", path.display())),
    }
}

/// With `since` (when the follower started), only filings accepted from then
/// on count: the first poll catches up on filings accepted earlier, which is
/// not a live delay.
fn report(
    day: Date,
    since: Option<i64>,
    saved: &HashMap<String, Saved>,
    deliveries: &[Delivery],
) -> String {
    let live = |accepted: i64| since.is_none_or(|t| accepted >= t);
    let mut out = format!("Delays on {day} (US Eastern), from SEC acceptance\n");
    if let Some(t) = since {
        out.push_str(&format!(
            "only filings accepted from {} on (when the follower started)\n",
            oe_alerts::select::utc_timestamp(t.max(0) as u64)
        ));
    }
    // Feed: filings accepted and saved that day (later backfills are not live).
    let mut by_kind: HashMap<&str, Vec<i64>> = HashMap::new();
    for s in saved.values() {
        if eastern_day(s.accepted) == Some(day)
            && eastern_day(s.saved) == Some(day)
            && live(s.accepted)
        {
            by_kind
                .entry(s.kind)
                .or_default()
                .push(s.saved - s.accepted);
        }
    }
    let mut all: Vec<i64> = by_kind.values().flatten().copied().collect();
    match stats(std::mem::take(&mut all)) {
        None => out.push_str("feed: no filing accepted and saved that day\n"),
        Some(s) => {
            out.push_str(&line("feed, filings saved", &s));
            out.push('\n');
            let mut kinds: Vec<_> = by_kind.into_iter().collect();
            kinds.sort_by_key(|(k, _)| *k);
            for (kind, v) in kinds {
                if let Some(s) = stats(v) {
                    out.push_str(&format!("  {}\n", line(kind, &s)));
                }
            }
        }
    }
    // Alerts: delivered that day, against the latest receipt accepted before.
    let mut rows: Vec<(i64, &Delivery)> = Vec::new();
    let mut unmatched = 0usize;
    let mut caught_up = 0usize;
    for d in deliveries.iter().filter(|d| eastern_day(d.at) == Some(day)) {
        let latest = d
            .accessions
            .iter()
            .filter_map(|a| saved.get(a))
            .map(|s| s.accepted)
            .filter(|t| *t <= d.at)
            .max();
        match latest {
            Some(t) if live(t) => rows.push((d.at - t, d)),
            Some(_) => caught_up += 1,
            None => unmatched += 1,
        }
    }
    match stats(rows.iter().map(|(s, _)| *s).collect()) {
        None => out.push_str("alerts: none delivered that day with a saved receipt\n"),
        Some(s) => {
            out.push_str(&line("alerts delivered", &s));
            out.push('\n');
            rows.sort_by_key(|(s, _)| std::cmp::Reverse(*s));
            out.push_str("  slowest:\n");
            for (secs, d) in rows.iter().take(5) {
                out.push_str(&format!(
                    "    {}  {}  {}  {}\n",
                    mmss(*secs),
                    d.destination,
                    d.rule,
                    d.headline.chars().take(90).collect::<String>()
                ));
            }
        }
    }
    if caught_up > 0 {
        out.push_str(&format!(
            "  {caught_up} delivery(ies) rest on filings accepted before the follower started (catching up, not counted)\n"
        ));
    }
    if unmatched > 0 {
        out.push_str(&format!(
            "  {unmatched} delivery(ies) that day had no saved receipt to time against\n"
        ));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(t: &str) -> i64 {
        oe_core::utc_to_unix(t).unwrap()
    }

    #[test]
    fn deliveries_round_trip_and_bad_lines_are_skipped() {
        let dir = std::env::temp_dir().join(format!("oe-delays-{}", std::process::id()));
        let state = dir.join("sent.tsv");
        let alert = Alert {
            key: "k".into(),
            rule_id: "insider.cluster_buy@1",
            amount_usd: None,
            percent: None,
            known_on: None,
            issuer_cik: None,
            issuer_name: None,
            ticker: None,
            headline: "Cluster\tbuy".into(),
            details: Vec::new(),
            people: Vec::new(),
            receipts: vec![
                oe_alerts::receipt(oe_core::Cik(1), "0000000001-26-000001", None),
                oe_alerts::receipt(oe_core::Cik(1), "0000000001-26-000002", None),
            ],
        };
        record(&state, 1_795_188_104, "console", &alert).unwrap();
        let text = std::fs::read_to_string(dir.join("delivered.tsv")).unwrap();
        let d = parse_deliveries(&format!("{text}garbage line\n"));
        assert_eq!(d.len(), 1);
        assert_eq!(d[0].at, 1_795_188_104);
        assert_eq!(d[0].rule, "insider.cluster_buy@1");
        assert_eq!(d[0].accessions.len(), 2);
        assert_eq!(d[0].headline, "Cluster buy");
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn the_report_times_each_alert_from_its_latest_receipt() {
        let day = Date::from_ymd(2026, 11, 23).unwrap();
        let f = |kind, accepted: &str, saved: &str| Saved {
            kind,
            accepted: at(accepted),
            saved: at(saved),
        };
        let saved: HashMap<String, Saved> = [
            // 10:00:00 and 10:01:00 Eastern (EST, UTC-5).
            (
                "a1".into(),
                f("form4", "2026-11-23T15:00:00Z", "2026-11-23T15:01:10Z"),
            ),
            (
                "a2".into(),
                f("form4", "2026-11-23T15:01:00Z", "2026-11-23T15:02:00Z"),
            ),
            (
                "b1".into(),
                f("form144", "2026-11-23T16:00:00Z", "2026-11-23T16:09:00Z"),
            ),
            // Accepted the day before: a backfill, not in the feed numbers.
            (
                "old".into(),
                f("form4", "2026-11-20T15:00:00Z", "2026-11-23T15:00:00Z"),
            ),
        ]
        .into_iter()
        .collect();
        let delivery = |t: &str, accs: &[&str]| Delivery {
            at: at(t),
            destination: "console".into(),
            rule: "r".into(),
            accessions: accs.iter().map(|a| a.to_string()).collect(),
            headline: "h".into(),
        };
        let deliveries = [
            // Timed from a2, the latest receipt: 2m00s.
            delivery("2026-11-23T15:03:00Z", &["a1", "a2"]),
            // 10m00s: over the target.
            delivery("2026-11-23T16:10:00Z", &["b1"]),
            // No saved receipt.
            delivery("2026-11-23T17:00:00Z", &["zzz"]),
            // Another day.
            delivery("2026-11-24T15:00:00Z", &["a1"]),
        ];
        let r = report(day, None, &saved, &deliveries);
        assert!(
            r.contains("feed, filings saved: 3 — median 1m10s, 90% within 9m00s, slowest 9m00s; 2 of 3 (67%) within 5 minutes"),
            "{r}"
        );
        assert!(r.contains("  form144: 1 —"), "{r}");
        assert!(
            r.contains("alerts delivered: 2 — median 10m00s, 90% within 10m00s, slowest 10m00s; 1 of 2 (50%) within 5 minutes"),
            "{r}"
        );
        assert!(
            r.contains("1 delivery(ies) that day had no saved receipt"),
            "{r}"
        );
        // From 10:30 Eastern on: only b1 and its alert count.
        let r = report(day, Some(at("2026-11-23T15:30:00Z")), &saved, &deliveries);
        assert!(r.contains("feed, filings saved: 1 —"), "{r}");
        assert!(r.contains("alerts delivered: 1 —"), "{r}");
        assert!(
            r.contains("1 delivery(ies) rest on filings accepted before"),
            "{r}"
        );
        assert_eq!(mmss(125), "2m05s");
        assert_eq!(
            stats(vec![5, 1, 3]).unwrap(),
            Stats {
                n: 3,
                median: 3,
                p90: 5,
                max: 5,
                within_target: 3
            }
        );
    }
}
