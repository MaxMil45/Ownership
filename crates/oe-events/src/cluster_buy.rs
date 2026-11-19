//! `insider.cluster_buy@1`: several insiders buying their own company's
//! stock on the open market within a short window.
//!
//! Spec: `events/specs/insider.cluster_buy.v1.md`.

use crate::InsiderTrade;
use oe_core::{Cik, Date, TransactionCode};
use std::collections::{BTreeMap, BTreeSet, HashSet};

pub const RULE_ID: &str = "insider.cluster_buy@1";

#[derive(Debug, Clone, PartialEq)]
pub struct ClusterBuyParams {
    /// Distinct insiders needed for an event.
    pub min_insiders: usize,
    /// All qualifying buys fall within this many calendar days of the first.
    pub window_days: i32,
    /// Drop buys from filings with the 10b5-1 checkbox ticked.
    pub exclude_10b5_1: bool,
    /// An insider is "routine" if they traded this issuer in the same calendar
    /// month in each of this many previous years; their buys are dropped.
    /// Zero disables the filter.
    pub routine_lookback_years: u32,
}

impl Default for ClusterBuyParams {
    fn default() -> Self {
        ClusterBuyParams {
            min_insiders: 3,
            window_days: 10,
            exclude_10b5_1: true,
            routine_lookback_years: 3,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClusterBuyEvent {
    pub rule_id: &'static str,
    pub issuer: Cik,
    pub ticker: Option<String>,
    pub window_start: Date,
    pub window_end: Date,
    /// Distinct insiders, in order of their first buy.
    pub insiders: Vec<(Cik, String)>,
    /// Sum of shares × price where both are known.
    pub total_value: f64,
    /// Buys whose value could not be computed (price missing).
    pub trades_without_value: usize,
    pub trades: Vec<InsiderTrade>,
    /// The date the event became detectable: when the `min_insiders`-th
    /// distinct insider's filing was accepted. `None` if any of those dates
    /// is unknown.
    pub known_on: Option<Date>,
}

/// The calendar months in which each insider traded each issuer (code P or
/// S), built once from the history so the routine-trader filter is a set
/// lookup per trade instead of a scan of the whole history.
pub struct RoutineIndex {
    months: HashSet<(Cik, Cik, i32, u32)>,
}

impl RoutineIndex {
    pub fn new(history: &[InsiderTrade]) -> RoutineIndex {
        RoutineIndex {
            months: history
                .iter()
                .filter(|t| matches!(t.code, TransactionCode::P | TransactionCode::S))
                .map(|t| (t.owner, t.issuer, t.date.year(), t.date.month()))
                .collect(),
        }
    }

    /// Whether `owner` traded `issuer` in the calendar month of `on` in each
    /// of the `years` previous years. Zero years means never routine.
    pub fn is_routine(&self, owner: Cik, issuer: Cik, on: Date, years: u32) -> bool {
        years > 0
            && (1..=years as i32).all(|back| {
                self.months
                    .contains(&(owner, issuer, on.year() - back, on.month()))
            })
    }
}

/// Whether `owner` traded `issuer` (buy or sell) in the same calendar month
/// as `on` in each of the `years` previous years. For one check; to check many
/// trades, build a [`RoutineIndex`] once.
pub fn is_routine(owner: Cik, issuer: Cik, on: Date, history: &[InsiderTrade], years: u32) -> bool {
    years > 0 && RoutineIndex::new(history).is_routine(owner, issuer, on, years)
}
/// Finds cluster-buy events in `trades`.
///
/// `history` is used only for the routine-trader filter; pass earlier trades
/// for the same issuers (it may include `trades` itself).
pub fn detect_cluster_buys(
    trades: &[InsiderTrade],
    history: &[InsiderTrade],
    params: &ClusterBuyParams,
) -> Vec<ClusterBuyEvent> {
    // Built once; empty when the filter is off.
    let routine = if params.routine_lookback_years > 0 {
        RoutineIndex::new(history)
    } else {
        RoutineIndex::new(&[])
    };
    let qualifying = trades.iter().filter(|t| {
        t.is_open_market_purchase
            && !(params.exclude_10b5_1 && t.plan_10b5_1 == Some(true))
            && !routine.is_routine(t.owner, t.issuer, t.date, params.routine_lookback_years)
    });

    let mut by_issuer: BTreeMap<Cik, Vec<&InsiderTrade>> = BTreeMap::new();
    for t in qualifying {
        by_issuer.entry(t.issuer).or_default().push(t);
    }

    let mut events = Vec::new();
    for (issuer, mut buys) in by_issuer {
        buys.sort_by_key(|t| (t.date, t.owner));
        let mut i = 0;
        while i < buys.len() {
            let start = buys[i].date;
            let mut j = i;
            while j + 1 < buys.len() && start.days_until(buys[j + 1].date) < params.window_days {
                j += 1;
            }
            let window = &buys[i..=j];
            let distinct: BTreeSet<Cik> = window.iter().map(|t| t.owner).collect();
            if distinct.len() >= params.min_insiders.max(1) {
                events.push(build_event(issuer, window, params.min_insiders.max(1)));
                i = j + 1;
            } else {
                i += 1;
            }
        }
    }
    events
}

fn build_event(issuer: Cik, window: &[&InsiderTrade], min_insiders: usize) -> ClusterBuyEvent {
    let mut insiders: Vec<(Cik, String)> = Vec::new();
    let mut first_known: BTreeMap<Cik, Option<Date>> = BTreeMap::new();
    for t in window {
        if !insiders.iter().any(|(c, _)| *c == t.owner) {
            insiders.push((t.owner, t.owner_name.clone()));
        }
        let entry = first_known.entry(t.owner).or_insert(t.known_on);
        *entry = match (*entry, t.known_on) {
            (Some(a), Some(b)) => Some(a.min(b)),
            (a, b) => a.or(b),
        };
    }

    let mut known: Vec<Option<Date>> = first_known.into_values().collect();
    let known_on = if known.iter().all(Option::is_some) {
        known.sort();
        known.get(min_insiders - 1).copied().flatten()
    } else {
        None
    };

    let total_value = window.iter().filter_map(|t| t.value()).sum();
    let trades_without_value = window.iter().filter(|t| t.value().is_none()).count();

    ClusterBuyEvent {
        rule_id: RULE_ID,
        issuer,
        ticker: window.iter().find_map(|t| t.ticker.clone()),
        window_start: window[0].date,
        window_end: window[window.len() - 1].date,
        insiders,
        total_value,
        trades_without_value,
        trades: window.iter().map(|t| (*t).clone()).collect(),
        known_on,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::trades_from_document;
    use oe_core::FilingMeta;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    fn synthetic_trades() -> Vec<InsiderTrade> {
        let dir = format!(
            "{}/../../fixtures/form4/synthetic",
            env!("CARGO_MANIFEST_DIR")
        );
        let mut files: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.extension().is_some_and(|x| x == "xml"))
            .collect();
        files.sort();
        files
            .iter()
            .enumerate()
            .flat_map(|(n, path)| {
                let doc = oe_form4::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
                // Pretend each filing was accepted two days after the trade.
                let meta = FilingMeta {
                    known_on: doc.period_of_report.map(|p| p.add_days(2)),
                    source: Some(format!("synthetic-{n}")),
                    ..FilingMeta::default()
                };
                trades_from_document(&doc, &meta)
            })
            .collect()
    }

    #[test]
    fn synthetic_fixtures_produce_exactly_one_event() {
        let trades = synthetic_trades();
        let events = detect_cluster_buys(&trades, &trades, &ClusterBuyParams::default());
        assert_eq!(events.len(), 1, "{events:#?}");
        let e = &events[0];
        assert_eq!(e.rule_id, "insider.cluster_buy@1");
        assert_eq!(e.ticker.as_deref(), Some("SYNW"));
        assert_eq!(e.window_start, d(2026, 3, 2));
        assert_eq!(e.window_end, d(2026, 3, 9));
        assert_eq!(e.insiders.len(), 3);
        assert_eq!(e.insiders[1].1, "Officer Bravo & Family");
        // 10,000 × 12.50 + 20,000 × 12.8125 + 4,000 × 13.10
        assert!((e.total_value - 433_650.0).abs() < 1e-6);
        // Third insider's filing (trade on Mar 9) accepted Mar 11.
        assert_eq!(e.known_on, Some(d(2026, 3, 11)));
    }

    #[test]
    fn two_insiders_are_not_enough() {
        let trades: Vec<_> = synthetic_trades()
            .into_iter()
            .filter(|t| t.owner != Cik(9_000_003))
            .collect();
        assert!(detect_cluster_buys(&trades, &trades, &ClusterBuyParams::default()).is_empty());
    }

    #[test]
    fn routine_traders_are_excluded() {
        let trades = synthetic_trades();
        let mut history = trades.clone();
        // Director Alpha bought in March of each of the last three years.
        for back in 1..=3 {
            let mut old = trades[0].clone();
            old.date = d(2026 - back, 3, 10);
            history.push(old);
        }
        assert!(is_routine(
            Cik(9_000_001),
            Cik(9_999_901),
            d(2026, 3, 2),
            &history,
            3
        ));
        let events = detect_cluster_buys(&trades, &history, &ClusterBuyParams::default());
        assert!(events.is_empty(), "only two non-routine insiders remain");
    }

    #[test]
    fn a_wider_window_pulls_in_the_late_buy() {
        let trades = synthetic_trades();
        let params = ClusterBuyParams {
            window_days: 30,
            ..ClusterBuyParams::default()
        };
        let events = detect_cluster_buys(&trades, &trades, &params);
        assert_eq!(events[0].insiders.len(), 4);
        assert_eq!(events[0].trades_without_value, 1);
    }

    #[test]
    fn unknown_acceptance_dates_give_unknown_known_on() {
        let mut trades = synthetic_trades();
        for t in &mut trades {
            t.known_on = None;
        }
        let events = detect_cluster_buys(&trades, &trades, &ClusterBuyParams::default());
        assert_eq!(events[0].known_on, None);
    }
}
