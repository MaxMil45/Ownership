//! A timing check for `insider.cluster_buy@1` on a backfill-sized input.
//! Ignored by default (it measures, it does not assert behaviour):
//!
//! ```powershell
//! cargo test --release -p oe-events --test bench_cluster_buy -- --ignored --nocapture
//! ```

use oe_core::{Cik, Date, TransactionCode};
use oe_events::{detect_cluster_buys, ClusterBuyParams, InsiderTrade};
use std::time::Instant;

fn trade(issuer: u64, owner: u64, date: Date, code: TransactionCode) -> InsiderTrade {
    InsiderTrade {
        issuer: Cik(issuer),
        ticker: None,
        owner: Cik(owner),
        owner_name: format!("Owner {owner}"),
        co_owners: Vec::new(),
        is_director: true,
        is_officer: false,
        officer_title: None,
        date,
        is_open_market_purchase: code == TransactionCode::P,
        code,
        shares: Some(1_000.0),
        price_per_share: Some(10.0),
        plan_10b5_1: Some(false),
        from_amendment: false,
        known_on: Some(date.add_days(2)),
        accession: None,
    }
}

#[test]
#[ignore = "timing check, run by hand"]
fn time_cluster_buys_on_a_large_history() {
    // OE_BENCH_ISSUERS issuers (default 100) × 8 insiders × 5 years of monthly P/S trades.
    let issuers: u64 = std::env::var("OE_BENCH_ISSUERS")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(100);
    let start = Date::from_ymd(2021, 1, 5).unwrap();
    let mut trades = Vec::new();
    for issuer in 0..issuers {
        for owner in 0..8u64 {
            for month in 0..60 {
                let code = if (issuer + owner + month) % 3 == 0 {
                    TransactionCode::P
                } else {
                    TransactionCode::S
                };
                trades.push(trade(
                    1_000_000 + issuer,
                    2_000_000 + issuer * 10 + owner,
                    start.add_days(month as i32 * 30 + owner as i32),
                    code,
                ));
            }
        }
    }
    let t = Instant::now();
    let events = detect_cluster_buys(&trades, &trades, &ClusterBuyParams::default());
    println!(
        "{} trades -> {} events in {:.2?}",
        trades.len(),
        events.len(),
        t.elapsed()
    );
}
