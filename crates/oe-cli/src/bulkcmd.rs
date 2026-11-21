//! `oe bulk-insider`: the history of Forms 3/4/5 from the SEC's quarterly
//! Insider Transactions Data Sets (ADR 0008). Downloads each quarter's zip
//! once into `data/bulk/`, then loads it into the PostgreSQL ledger.

use oe_bulk::Quarter;
use std::path::{Path, PathBuf};

pub const DEFAULT_DIR: &str = "data/bulk";
/// Filings per database transaction during a bulk load.
#[cfg(feature = "db")]
const CHUNK: usize = 5_000;

#[derive(Debug, PartialEq)]
pub struct BulkArgs {
    pub from: Quarter,
    /// `None`: the newest quarter the SEC lists.
    pub to: Option<Quarter>,
    pub dir: PathBuf,
    /// Read and count, write nothing to a database.
    pub dry_run: bool,
    /// Download only.
    pub no_load: bool,
    /// Use the zips already in `dir`; no network.
    pub offline: bool,
}

pub fn parse_args(args: &[String]) -> Result<BulkArgs, String> {
    let mut parsed = BulkArgs {
        from: Quarter { year: 2015, q: 1 },
        to: None,
        dir: PathBuf::from(DEFAULT_DIR),
        dry_run: false,
        no_load: false,
        offline: false,
    };
    let mut it = args.iter();
    let quarter = |it: &mut super::args::Args, flag: &str| -> Result<Quarter, String> {
        it.next()
            .ok_or_else(|| format!("{flag} needs a quarter such as 2015q1"))?
            .parse::<Quarter>()
            .map_err(|e| format!("{flag}: {e}"))
    };
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--from" => parsed.from = quarter(&mut it, "--from")?,
            "--to" => parsed.to = Some(quarter(&mut it, "--to")?),
            "--dir" => parsed.dir = super::args::folder(&mut it, "--dir")?,
            "--dry-run" => parsed.dry_run = true,
            "--no-load" => parsed.no_load = true,
            "--offline" => parsed.offline = true,
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    if parsed.to.is_some_and(|t| t < parsed.from) {
        return Err("--to is before --from".into());
    }
    Ok(parsed)
}

/// The quarters to handle and where each one's zip comes from. Offline, the
/// zips already in `dir` stand in for the SEC's list.
fn plan(args: &BulkArgs) -> Result<Vec<(Quarter, String)>, String> {
    let listed: Vec<(Quarter, String)> = if args.offline {
        let mut found = Vec::new();
        if let Ok(entries) = std::fs::read_dir(&args.dir) {
            for e in entries.flatten() {
                let name = e.file_name().to_string_lossy().into_owned();
                if let Some(q) = name
                    .strip_suffix("_form345.zip")
                    .and_then(|q| q.parse().ok())
                {
                    found.push((q, format!("file:{name}")));
                }
            }
        }
        found.sort();
        found
    } else {
        list_online()?
    };
    let to = args.to.or_else(|| listed.last().map(|(q, _)| *q));
    Ok(listed
        .into_iter()
        .filter(|(q, _)| *q >= args.from && to.is_some_and(|t| *q <= t))
        .collect())
}

#[cfg(feature = "net")]
fn list_online() -> Result<Vec<(Quarter, String)>, String> {
    let client = oe_edgar::EdgarClient::from_env().map_err(|e| e.to_string())?;
    let html = client
        .get_text(oe_bulk::DATA_SETS_PAGE)
        .map_err(|e| e.to_string())?;
    let links = oe_bulk::parse_data_set_links(&html);
    if links.is_empty() {
        return Err(format!(
            "no quarterly zip links found on {}; the page may have changed",
            oe_bulk::DATA_SETS_PAGE
        ));
    }
    Ok(links)
}

#[cfg(not(feature = "net"))]
fn list_online() -> Result<Vec<(Quarter, String)>, String> {
    Err("this build has no network support; use --offline with zips already downloaded".into())
}

#[cfg(feature = "net")]
fn download(url: &str, path: &Path) -> Result<u64, String> {
    let client = oe_edgar::EdgarClient::from_env().map_err(|e| e.to_string())?;
    client.save_to_file(url, path).map_err(|e| e.to_string())
}

#[cfg(not(feature = "net"))]
fn download(_url: &str, _path: &Path) -> Result<u64, String> {
    Err("this build has no network support".into())
}

pub fn bulk_insider(args: &[String]) -> Result<(), String> {
    let args = parse_args(args)?;
    let quarters = plan(&args)?;
    if quarters.is_empty() {
        return Err("no quarters in that range".into());
    }
    std::fs::create_dir_all(&args.dir).map_err(|e| format!("{}: {e}", args.dir.display()))?;
    println!(
        "{} quarter(s): {} to {}",
        quarters.len(),
        quarters[0].0,
        quarters[quarters.len() - 1].0
    );

    #[cfg(feature = "db")]
    let mut client = if args.dry_run || args.no_load {
        None
    } else {
        let url = std::env::var("DATABASE_URL").map_err(|_| {
            "set DATABASE_URL to load into PostgreSQL (or use --no-load / --dry-run)".to_string()
        })?;
        let mut c = oe_store::connect(&url).map_err(|e| e.to_string())?;
        oe_store::check_schema(&mut c).map_err(|e| e.to_string())?;
        Some(c)
    };
    #[cfg(not(feature = "db"))]
    if !(args.dry_run || args.no_load) {
        return Err("this build has no database support; use --no-load or --dry-run".into());
    }

    let mut totals = (0usize, 0usize, 0u64);
    for (q, url) in &quarters {
        let path = args.dir.join(q.file_name());
        if !path.exists() {
            let started = std::time::Instant::now();
            let n = download(url, &path)?;
            println!(
                "{q}: downloaded {:.1} MB in {:.1} s",
                n as f64 / 1e6,
                started.elapsed().as_secs_f64()
            );
        }
        if args.no_load {
            continue;
        }
        let started = std::time::Instant::now();
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (filings, skipped) =
            oe_bulk::read_quarter(&bytes).map_err(|e| format!("{}: {e}", path.display()))?;
        let source_url = if url.starts_with("file:") {
            format!("https://www.sec.gov/files/{}", q.file_name())
        } else {
            url.clone()
        };
        let mut prepared = Vec::with_capacity(filings.len());
        let mut rejected = 0usize;
        for f in &filings {
            match oe_store_prepare(f, &source_url) {
                Some(p) => prepared.push(p),
                None => rejected += 1,
            }
        }
        let rows: usize = prepared.iter().map(|p| p.transactions.len()).sum();
        totals.0 += prepared.len();
        totals.1 += rows;
        #[cfg_attr(not(feature = "db"), allow(unused_mut))]
        let mut line = format!(
            "{q}: {} filing(s), {rows} transaction row(s); {} filing(s) and {} row(s) unreadable, {rejected} rejected",
            prepared.len(),
            skipped.filings.len(),
            skipped.rows
        );
        #[cfg(feature = "db")]
        if let Some(c) = client.as_mut() {
            let mut new = (0u64, 0u64);
            for chunk in prepared.chunks(CHUNK) {
                let r = oe_store::load_batch_with(c, chunk, false).map_err(|e| e.to_string())?;
                new.0 += r.filings_new;
                new.1 += r.rows_new;
            }
            let l = oe_store::link_pass(c).map_err(|e| e.to_string())?;
            totals.2 += new.0;
            line.push_str(&format!(
                "; loaded {} new filing(s), {} new row(s); amendments: {} loaded, {} held back, {} ambiguous, {} waiting",
                new.0, new.1, l.loaded, l.held_back, l.ambiguous, l.waiting
            ));
        }
        println!("{line} ({:.1} s)", started.elapsed().as_secs_f64());
    }
    println!(
        "done: {} filing(s) and {} row(s) read{}",
        totals.0,
        totals.1,
        if args.dry_run || args.no_load {
            "; nothing written to a database".to_string()
        } else {
            format!(", {} new filing(s) in the database", totals.2)
        }
    );
    Ok(())
}

#[cfg(feature = "db")]
fn oe_store_prepare(f: &oe_bulk::BulkFiling, url: &str) -> Option<oe_store::Prepared> {
    oe_store::prepare_bulk(f, url).ok()
}

/// Without the database feature only the count matters.
#[cfg(not(feature = "db"))]
fn oe_store_prepare(f: &oe_bulk::BulkFiling, _url: &str) -> Option<NoDb> {
    (!f.document.owners.is_empty()).then(|| NoDb {
        transactions: f.document.transactions.clone(),
    })
}

#[cfg(not(feature = "db"))]
struct NoDb {
    transactions: Vec<oe_core::Transaction>,
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn args() {
        let p = parse_args(&[]).unwrap();
        assert_eq!(p.from.to_string(), "2015q1");
        assert_eq!(p.to, None);
        assert_eq!(p.dir, PathBuf::from(DEFAULT_DIR));
        let p = parse_args(&a(&[
            "--from",
            "2020q2",
            "--to",
            "2021Q1",
            "--dry-run",
            "--offline",
        ]))
        .unwrap();
        assert_eq!(
            (p.from.to_string(), p.to.unwrap().to_string()),
            ("2020q2".into(), "2021q1".into())
        );
        assert!(p.dry_run && p.offline);
        assert!(parse_args(&a(&["--from", "2021q1", "--to", "2020q4"])).is_err());
        assert!(parse_args(&a(&["--from", "2020"])).is_err());
        assert!(parse_args(&a(&["--nope"])).is_err());
    }

    #[test]
    fn offline_plan_uses_the_zips_in_the_folder() {
        let dir = std::env::temp_dir().join(format!("oe-bulk-plan-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        for f in [
            "2014q4_form345.zip",
            "2015q1_form345.zip",
            "2015q2_form345.zip",
            "notes.txt",
        ] {
            std::fs::write(dir.join(f), b"x").unwrap();
        }
        let args = BulkArgs {
            from: "2015q1".parse().unwrap(),
            to: None,
            dir: dir.clone(),
            dry_run: true,
            no_load: false,
            offline: true,
        };
        let p = plan(&args).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        let qs: Vec<String> = p.iter().map(|(q, _)| q.to_string()).collect();
        assert_eq!(qs, ["2015q1", "2015q2"]);
    }
}
