//! Which alerts to deliver now: on the watchlist, fresh enough, and not yet
//! sent to that destination. Pure; the clock is passed in.

use crate::sent::SentLog;
use crate::watchlist::Watchlist;
use crate::Alert;
use oe_core::Date;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Selection {
    pub max_age_days: i32,
    /// Alerts whose `known_on` is unknown are skipped unless this is set: an
    /// old filing without a date would otherwise look new.
    pub include_undated: bool,
}

impl Default for Selection {
    fn default() -> Self {
        Selection {
            max_age_days: 3,
            include_undated: false,
        }
    }
}

/// Why an alert was not selected, for the summary.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct Skipped {
    pub not_watched: usize,
    pub too_old: usize,
    pub undated: usize,
    pub already_sent: usize,
    /// Same key as an alert earlier in this run (for example one person's
    /// two plans terminated in one report): delivered once.
    pub repeated: usize,
}

/// The alerts to deliver to `destination` today, oldest first, each once
/// even if two detections produced the same key.
pub fn select<'a>(
    alerts: &'a [Alert],
    watchlist: Option<&Watchlist>,
    sent: &SentLog,
    destination: &str,
    today: Date,
    rules: &Selection,
) -> (Vec<&'a Alert>, Skipped) {
    let mut skipped = Skipped::default();
    let mut chosen: Vec<&Alert> = Vec::new();
    let mut keys = std::collections::HashSet::new();
    for a in alerts {
        if watchlist.is_some_and(|w| !w.matches(a)) {
            skipped.not_watched += 1;
            continue;
        }
        match a.known_on {
            None if !rules.include_undated => {
                skipped.undated += 1;
                continue;
            }
            Some(d) if d.days_until(today) > rules.max_age_days => {
                skipped.too_old += 1;
                continue;
            }
            _ => {}
        }
        if sent.contains(destination, &a.key) {
            skipped.already_sent += 1;
            continue;
        }
        if !keys.insert(a.key.as_str()) {
            skipped.repeated += 1;
            continue;
        }
        chosen.push(a);
    }
    chosen.sort_by(|a, b| a.known_on.cmp(&b.known_on).then(a.key.cmp(&b.key)));
    (chosen, skipped)
}

/// Today's date in US Eastern time, from seconds since 1970 (UTC).
pub fn eastern_date_from_unix(secs: u64) -> Date {
    oe_core::us_eastern_date(&utc_timestamp(secs)).expect("valid timestamp")
}

/// `2026-11-20T15:21:44Z` from seconds since 1970.
pub fn utc_timestamp(secs: u64) -> String {
    let days = (secs / 86_400) as i32;
    let rest = secs % 86_400;
    let date = Date::from_ymd(1970, 1, 1).expect("valid").add_days(days);
    format!(
        "{date}T{:02}:{:02}:{:02}Z",
        rest / 3600,
        rest % 3600 / 60,
        rest % 60
    )
}

/// Seconds since 1970 now.
pub fn now_unix() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(key: &str, known_on: Option<&str>, ticker: &str) -> Alert {
        Alert {
            key: key.into(),
            rule_id: "insider.cluster_buy@1",
            known_on: known_on.map(|d| d.parse().unwrap()),
            issuer_cik: None,
            issuer_name: None,
            ticker: Some(ticker.into()),
            headline: key.into(),
            details: Vec::new(),
            people: Vec::new(),
            receipts: Vec::new(),
        }
    }

    #[test]
    fn keeps_fresh_watched_unsent_alerts_oldest_first() {
        let today: Date = "2026-11-20".parse().unwrap();
        let alerts = [
            a("new", Some("2026-11-20"), "AAPL"),
            a("older", Some("2026-11-18"), "AAPL"),
            a("stale", Some("2026-11-10"), "AAPL"),
            a("undated", None, "AAPL"),
            a("other", Some("2026-11-20"), "MSFT"),
            a("sent", Some("2026-11-19"), "AAPL"),
            a("new", Some("2026-11-20"), "AAPL"), // same key twice
        ];
        let mut sent = SentLog::in_memory();
        sent.record("discord", "sent", "t").unwrap();
        let watch = Watchlist::parse("ticker AAPL").unwrap();
        let (chosen, skipped) = select(
            &alerts,
            Some(&watch),
            &sent,
            "discord",
            today,
            &Selection::default(),
        );
        let keys: Vec<&str> = chosen.iter().map(|a| a.key.as_str()).collect();
        assert_eq!(keys, ["older", "new"]);
        assert_eq!(
            skipped,
            Skipped {
                not_watched: 1,
                too_old: 1,
                undated: 1,
                already_sent: 1,
                repeated: 1
            }
        );
        // Sent to Discord is not sent to a webhook.
        let (chosen, _) = select(
            &alerts,
            Some(&watch),
            &sent,
            "webhook",
            today,
            &Selection::default(),
        );
        assert_eq!(chosen.len(), 3);
    }

    #[test]
    fn three_days_means_three_calendar_days() {
        let today: Date = "2026-11-20".parse().unwrap();
        let alerts = [
            a("edge", Some("2026-11-17"), "X"),
            a("out", Some("2026-11-16"), "X"),
        ];
        let (chosen, _) = select(
            &alerts,
            None,
            &SentLog::in_memory(),
            "d",
            today,
            &Selection::default(),
        );
        assert_eq!(chosen.len(), 1);
        assert_eq!(chosen[0].key, "edge");
    }

    #[test]
    fn clock_helpers() {
        // 2026-11-20 15:21:44 UTC is 10:21 in New York.
        assert_eq!(utc_timestamp(1_795_188_104), "2026-11-20T15:21:44Z");
        assert_eq!(
            eastern_date_from_unix(1_795_188_104).to_string(),
            "2026-11-20"
        );
        // 2026-11-21 03:00 UTC is still the 20th in New York.
        assert_eq!(
            eastern_date_from_unix(1_795_230_000).to_string(),
            "2026-11-20"
        );
    }
}
