//! `oe prices-import <zip or folder>` and `oe outcomes-build`: daily prices
//! from a file the user downloaded (Stooq's ASCII database), and what
//! happened after each event and insider trade (migration `0010_prices`).

use std::collections::HashSet;
use std::path::PathBuf;

fn connect() -> Result<oe_store::Client, String> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL is not set (see `oe db-status`)".to_string())?;
    oe_store::connect(&url).map_err(|e| e.to_string())
}

pub fn prices_import(argv: &[String]) -> Result<(), String> {
    let [path] = argv else {
        return Err(
            "prices-import needs the Stooq file or folder, for example: oe prices-import data\\prices\\d_us_txt.zip"
                .into(),
        );
    };
    let path = PathBuf::from(path);
    if !path.exists() {
        return Err(format!("{} does not exist", path.display()));
    }
    let mut client = connect()?;
    let started = std::time::Instant::now();
    let mut wanted: HashSet<String> = oe_store::wanted_symbols(&mut client)
        .map_err(|e| e.to_string())?
        .into_iter()
        .map(|(_, s)| s)
        .collect();
    let companies = wanted.len();
    wanted.insert(oe_store::BENCHMARK.to_string());
    println!(
        "reading {} for {companies} companies' tickers and {} (the market)",
        path.display(),
        oe_store::BENCHMARK
    );
    let mut read = oe_prices::ReadReport::default();
    let mut skipped_rows = 0usize;
    let imported = oe_store::import_prices(&mut client, &path.display().to_string(), |sink| {
        let mut first_error: Option<String> = None;
        read = oe_prices::read_wanted(&path, &wanted, |sym, series| {
            skipped_rows += series.skipped;
            if first_error.is_none() {
                if let Err(e) = sink(sym, &series.days) {
                    first_error = Some(e);
                }
            }
        })?;
        first_error.map_or(Ok(()), Err)
    })
    .map_err(|e| e.to_string())??;
    println!(
        "{} price files in the source; {} wanted series read, {} stored with {} days since {} ({} rows that were not a date and a price skipped)",
        read.files,
        read.read,
        imported.symbols,
        imported.days,
        oe_store::prices::SINCE,
        skipped_rows
    );
    for (sym, why) in read.failed.iter().take(5) {
        eprintln!("  {sym}: {why}");
    }
    if imported.symbols == 0 {
        return Err(
            "no wanted series found: is this Stooq's US daily ASCII file (files named like aapl.us.txt)?"
                .into(),
        );
    }
    let c = oe_store::check_series(&mut client).map_err(|e| e.to_string())?;
    println!(
        "checked against insiders' reported prices: {} companies ok, {} mismatched (left out), {} with too few trades to check (left out), {} with no series in the file",
        c.ok, c.mismatch, c.unchecked, c.no_prices
    );
    outcomes(&mut client)?;
    println!("done in {:.0} s", started.elapsed().as_secs_f64());
    Ok(())
}

fn outcomes(client: &mut oe_store::Client) -> Result<(), String> {
    let t = std::time::Instant::now();
    let o = oe_store::build_outcomes(client).map_err(|e| e.to_string())?;
    println!(
        "outcomes measured in {:.0} s: {} events, {} insider buy days, {} sale days ({} old enough for a 12-month result)",
        t.elapsed().as_secs_f64(),
        o.events,
        o.buys,
        o.sales,
        o.with_12m
    );
    Ok(())
}

pub fn outcomes_build(argv: &[String]) -> Result<(), String> {
    if let Some(a) = argv.first() {
        return Err(format!("unexpected argument {a:?}"));
    }
    let mut client = connect()?;
    let st = oe_store::price_status(&mut client).map_err(|e| e.to_string())?;
    if st.symbols == 0 {
        println!("no prices imported yet (oe prices-import <file>); nothing to measure");
        return Ok(());
    }
    // Tickers and filings change between runs: check the series again first.
    let c = oe_store::check_series(&mut client).map_err(|e| e.to_string())?;
    println!(
        "price series: {} companies ok, {} mismatched, {} with too few trades to check",
        c.ok, c.mismatch, c.unchecked
    );
    outcomes(&mut client)
}
