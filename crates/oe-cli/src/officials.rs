//! `oe house-ptr <year> [--to <year>] [--out <folder>] [--limit <n>]`: downloads
//! the House Clerk's yearly index and every periodic transaction report (PTR)
//! it lists, as PDFs, for members of the House (ADR 0010). Reading the PDFs is
//! a later step.
//!
//! Layout under `--out` (default `data/officials/house`), per year:
//! `<year>/<year>FD.txt` (the index as downloaded), `<year>/<DocID>.pdf` and
//! `<year>/<DocID>.meta.txt` (who, when filed, where from, when first seen,
//! SHA-256). A PDF counts as saved once its `.meta.txt` exists, so a rerun
//! fetches only what is missing. Two requests a second, a plain User-Agent.
//!
//! Refuses to run until `OE_OFFICIALS_ACK=1` is set by the user: the reports
//! may not be used commercially and are never committed or published
//! (`oe_officials::USE_LIMITS`).

use super::{args, meta_path, render_meta};
use oe_edgar::{EdgarClient, EdgarError};
use oe_officials::{index_txt_name, index_zip_url, parse_house_index, HouseIndexRow};
use std::path::{Path, PathBuf};

const DEFAULT_OUT: &str = "data/officials/house";
/// The House Clerk is not the SEC and publishes no limit: a gentle pace of
/// its own, well under the SEC's 8. At 2 a second the full history (2008 to
/// 2026, about 9,000 requests) takes about 1 h 15 min; at 1 it took 2 h 23 min
/// on GitHub (2026-11-24).
const REQUESTS_PER_SECOND: u32 = 2;

#[derive(Debug, PartialEq)]
pub struct HousePtrArgs {
    pub from: i32,
    pub to: i32,
    pub out: PathBuf,
    /// PDFs per year, for a quick try; `None` = every one.
    pub limit: Option<usize>,
}

pub fn parse_args(args: &[String]) -> Result<HousePtrArgs, String> {
    let year = |t: &str| -> Result<i32, String> {
        t.parse::<i32>()
            .ok()
            .filter(|y| (2008..=2100).contains(y))
            .ok_or(format!("{t:?} is not a year from 2008 on, such as 2025"))
    };
    let mut from = None;
    let mut parsed = HousePtrArgs {
        from: 0,
        to: 0,
        out: PathBuf::from(DEFAULT_OUT),
        limit: None,
    };
    let mut to = None;
    let mut it = args.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--to" => to = Some(year(it.next().ok_or("--to needs a year")?)?),
            "--out" => parsed.out = args::folder(&mut it, "--out")?,
            "--limit" => parsed.limit = Some(args::positive(&mut it, "--limit")?),
            other if from.is_none() => from = Some(year(other)?),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    parsed.from = from.ok_or("a year such as 2025 is needed")?;
    parsed.to = to.unwrap_or(parsed.from);
    if parsed.to < parsed.from {
        return Err("--to is before the first year".into());
    }
    Ok(parsed)
}

pub fn house_ptr(argv: &[String]) -> Result<(), String> {
    let args = parse_args(argv)?;
    if std::env::var("OE_OFFICIALS_ACK").as_deref() != Ok("1") {
        return Err(format!("not downloading.\n{}", oe_officials::USE_LIMITS));
    }
    let client = EdgarClient::other_site(REQUESTS_PER_SECOND);
    let mut failed: Vec<String> = Vec::new();
    let (mut saved_all, mut already_all, mut ptrs_all) = (0usize, 0usize, 0usize);
    for year in args.from..=args.to {
        let dir = args.out.join(year.to_string());
        std::fs::create_dir_all(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
        let rows = match fetch_index(&client, year, &dir) {
            Ok(r) => r,
            Err(e) => {
                println!("{year}: index FAILED, {e}");
                failed.push(format!("{year} index"));
                continue;
            }
        };
        let ptrs: Vec<&HouseIndexRow> = rows.iter().filter(|r| r.is_ptr()).collect();
        let take = args.limit.unwrap_or(usize::MAX);
        let (mut saved, mut already) = (0usize, 0usize);
        for row in ptrs.iter().take(take) {
            let pdf = dir.join(format!("{}.pdf", row.doc_id));
            if meta_path(&pdf).exists() {
                already += 1;
                continue;
            }
            match save_ptr(&client, row, &pdf) {
                Ok(()) => saved += 1,
                Err(e) => {
                    eprintln!("  {year} {}: {e}", row.doc_id);
                    failed.push(format!("{year}/{}", row.doc_id));
                }
            }
        }
        println!(
            "{year}: {} filing(s) in the index, {} PTR(s); {saved} saved, {already} already saved",
            rows.len(),
            ptrs.len()
        );
        saved_all += saved;
        already_all += already;
        ptrs_all += ptrs.len().min(take);
    }
    println!(
        "all years: {ptrs_all} PTR(s); {saved_all} saved, {already_all} already saved, {} failed",
        failed.len()
    );
    if failed.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{} download(s) failed; run the same command again (saved reports are skipped): {}",
            failed.len(),
            failed
                .iter()
                .take(20)
                .cloned()
                .collect::<Vec<_>>()
                .join(" ")
        ))
    }
}

/// Fetch with one retry after a pause when the site is busy or the
/// connection drops.
fn get_bytes_retrying(client: &EdgarClient, url: &str) -> Result<Vec<u8>, EdgarError> {
    match client.get_bytes(url) {
        Err(e) if transient(&e) => {
            std::thread::sleep(std::time::Duration::from_secs(10));
            client.get_bytes(url)
        }
        other => other,
    }
}

fn transient(e: &EdgarError) -> bool {
    match e {
        EdgarError::Status { code, .. } => *code == 429 || *code >= 500,
        EdgarError::Transport { .. } | EdgarError::Body { .. } => true,
        _ => false,
    }
}

/// Downloads the year's index, keeps its `.txt` and returns its rows.
fn fetch_index(client: &EdgarClient, year: i32, dir: &Path) -> Result<Vec<HouseIndexRow>, String> {
    let url = index_zip_url(year);
    let zip = get_bytes_retrying(client, &url).map_err(|e| e.to_string())?;
    let archive = oe_bulk::zip::Archive::open(&zip).map_err(|e| format!("{url}: {}", e.0))?;
    let name = index_txt_name(year);
    let entry = archive
        .find(&name)
        .ok_or(format!("{url}: no {name} inside"))?;
    let bytes = archive.read(entry).map_err(|e| format!("{url}: {}", e.0))?;
    let path = dir.join(&name);
    std::fs::write(&path, &bytes).map_err(|e| format!("{}: {e}", path.display()))?;
    let (rows, skipped) = parse_house_index(&String::from_utf8_lossy(&bytes))?;
    if skipped > 0 {
        eprintln!("  {year}: {skipped} index row(s) could not be read and were skipped");
    }
    Ok(rows)
}

fn save_ptr(client: &EdgarClient, row: &HouseIndexRow, pdf: &Path) -> Result<(), String> {
    let url = row.ptr_pdf_url();
    let bytes = get_bytes_retrying(client, &url).map_err(|e| e.to_string())?;
    if !bytes.starts_with(b"%PDF") {
        return Err(format!("{url}: not a PDF ({} bytes)", bytes.len()));
    }
    std::fs::write(pdf, &bytes).map_err(|e| format!("{}: {e}", pdf.display()))?;
    let now = oe_alerts::select::utc_timestamp(oe_alerts::select::now_unix());
    let filed = row.filing_date.map(|d| d.to_string()).unwrap_or_default();
    let meta = render_meta(&[
        ("doc_id", &row.doc_id),
        ("source", "house"),
        ("filing_type", &row.filing_type),
        ("name", &row.display_name()),
        ("state_district", &row.state_district),
        ("year", &row.year.to_string()),
        ("filed_on", &filed),
        // When this tool first saw it: the earliest the public surely could.
        ("first_seen_at", &now),
        ("url", &url),
        ("sha256", &oe_core::sha256_hex(&bytes)),
    ]);
    // The sidecar last: a PDF without one is fetched again next time.
    std::fs::write(meta_path(pdf), meta).map_err(|e| format!("{}: {e}", pdf.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn a(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn house_ptr_args() {
        let p = parse_args(&a(&["2025"])).unwrap();
        assert_eq!((p.from, p.to, p.limit), (2025, 2025, None));
        assert_eq!(p.out, PathBuf::from("data/officials/house"));
        let p = parse_args(&a(&["2015", "--to", "2026", "--limit", "3"])).unwrap();
        assert_eq!((p.from, p.to, p.limit), (2015, 2026, Some(3)));
        for bad in [
            &[][..],
            &["25"],
            &["2007"],
            &["2025", "--to", "2024"],
            &["2025", "x"],
        ] {
            assert!(parse_args(&a(bad)).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn refuses_without_the_users_acknowledgement() {
        // The variable is not set in tests; the limits are shown instead.
        if std::env::var("OE_OFFICIALS_ACK").is_err() {
            let e = house_ptr(&a(&["2025"])).unwrap_err();
            assert!(e.contains("commercial purpose") && e.contains("OE_OFFICIALS_ACK=1"));
        }
    }
}
