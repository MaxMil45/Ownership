//! `oe officials-load [--root data/officials]` and `oe official <name>`: the
//! members of Congress, their committees and the House reports they filed,
//! in the ledger (ADR 0010, migration `0006_officials`).

use super::args;
use oe_officials::{link_report, parse_house_index, Link};
use std::path::{Path, PathBuf};

const DEFAULT_ROOT: &str = "data/officials";
const LEGISLATOR_FILES: [&str; 2] = ["legislators-current.json", "legislators-historical.json"];

fn connect() -> Result<oe_store::Client, String> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL is not set (see `oe db-status`)".to_string())?;
    oe_store::connect(&url).map_err(|e| e.to_string())
}

fn read(path: &Path) -> Result<String, String> {
    std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", path.display()))
}

pub fn officials_load(argv: &[String]) -> Result<(), String> {
    let mut root = PathBuf::from(DEFAULT_ROOT);
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = args::folder(&mut it, "--root")?,
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    let leg = root.join("legislators");
    let mut people = Vec::new();
    let mut skipped = 0usize;
    for f in LEGISLATOR_FILES {
        let (p, s) = oe_officials::parse_legislators(&read(&leg.join(f))?)
            .map_err(|e| format!("{}: {e}", leg.join(f).display()))?;
        people.extend(p);
        skipped += s;
    }
    let committees = oe_officials::parse_committees(&read(&leg.join("committees-current.json"))?)?;
    let memberships =
        oe_officials::parse_memberships(&read(&leg.join("committee-membership-current.json"))?)?;
    let today = oe_alerts::select::eastern_date_from_unix(oe_alerts::select::now_unix());

    let mut client = connect()?;
    let m = oe_store::load_members(
        &mut client,
        &people,
        &committees,
        &memberships,
        &today.to_string(),
    )
    .map_err(|e| e.to_string())?;
    println!(
        "members: {} people read ({skipped} entries skipped); {} new, {} new term(s); {} new committee(s); {} new membership row(s) as of {today} ({} for people not in the data set)",
        people.len(),
        m.officials_new,
        m.terms_new,
        m.committees_new,
        m.memberships_new,
        m.memberships_unknown
    );

    let house = root.join("house");
    let mut years: Vec<PathBuf> = std::fs::read_dir(&house)
        .map_err(|e| format!("{}: {e}", house.display()))?
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .collect();
    years.sort();
    let (mut reports, mut linked, mut held) = (0usize, 0usize, 0usize);
    let mut written = oe_store::HouseReportsReport::default();
    for dir in years {
        let Some(year) = dir
            .file_name()
            .and_then(|n| n.to_str())
            .and_then(|n| n.parse::<i32>().ok())
        else {
            continue;
        };
        let index = dir.join(oe_officials::index_txt_name(year));
        let Ok(text) = std::fs::read_to_string(&index) else {
            continue;
        };
        let (rows, _) = parse_house_index(&text)?;
        let mut batch = Vec::new();
        for r in rows.iter().filter(|r| r.is_ptr()) {
            let pdf = dir.join(format!("{}.pdf", r.doc_id));
            let meta = std::fs::read_to_string(super::meta_path(&pdf)).unwrap_or_default();
            let kv: std::collections::HashMap<&str, &str> =
                oe_core::text::key_values(&meta).collect();
            let link = match link_report(r, &people) {
                Link::Linked { member, basis } => {
                    linked += 1;
                    Ok((member.bioguide.clone(), basis.to_string()))
                }
                Link::HeldBack { reason } => {
                    held += 1;
                    Err(reason)
                }
            };
            batch.push(oe_store::HouseReportRow {
                doc_id: r.doc_id.clone(),
                year: r.year,
                filing_type: r.filing_type.clone(),
                prefix: r.prefix.clone(),
                last: r.last.clone(),
                first: r.first.clone(),
                suffix: r.suffix.clone(),
                state_district: r.state_district.clone(),
                filed_on: r.filing_date.map(|d| d.to_string()),
                // Measured 2013-2026: seven-digit ids are scanned paper.
                scanned: r.doc_id.len() < 8,
                source_url: r.ptr_pdf_url(),
                first_seen_at: kv.get("first_seen_at").map(|s| s.to_string()),
                pdf_sha256: kv.get("sha256").map(|s| s.to_string()),
                link,
            });
        }
        reports += batch.len();
        let w = oe_store::load_house_reports(&mut client, &batch).map_err(|e| e.to_string())?;
        written.reports_new += w.reports_new;
        written.links_new += w.links_new;
        written.held_back_new += w.held_back_new;
    }
    println!(
        "House reports: {reports} read, {linked} linked to a member, {held} held back; {} new report(s), {} new link(s), {} new held back",
        written.reports_new, written.links_new, written.held_back_new
    );
    Ok(())
}

/// Which reader version filled `house_transaction` (bump when it changes).
const PTR_READER: &str = "oe-ptr 1 (2026-11-25)";

/// `oe ptr-load [--root data/officials/house]`: reads the transactions in
/// every typed House report on disk (ADR 0011) and replaces the stored ones.
pub fn ptr_load(argv: &[String]) -> Result<(), String> {
    let mut root = PathBuf::from(DEFAULT_ROOT).join("house");
    let mut it = argv.iter();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--root" => root = args::folder(&mut it, "--root")?,
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    let mut client = connect()?;
    let started = std::time::Instant::now();
    let wanted = oe_store::house_reports_to_read(&mut client).map_err(|e| e.to_string())?;
    // A ticker names a company when exactly one company's latest Form 3/4/5
    // ticker is it.
    let mut by_ticker: std::collections::HashMap<String, Option<i64>> = Default::default();
    for (cik, t) in oe_store::latest_tickers(&mut client).map_err(|e| e.to_string())? {
        by_ticker
            .entry(t.to_ascii_uppercase())
            .and_modify(|v| *v = None)
            .or_insert(Some(cik as i64));
    }
    let (mut missing, mut unreadable, mut problems) = (0usize, 0usize, 0usize);
    let mut reads = Vec::with_capacity(wanted.len());
    for (doc, year) in &wanted {
        let path = root.join(year.to_string()).join(format!("{doc}.pdf"));
        let Ok(bytes) = std::fs::read(&path) else {
            missing += 1;
            continue;
        };
        let report = match oe_ptr::read_report(&bytes) {
            Ok(r) => r,
            Err(e) => {
                unreadable += 1;
                reads.push(oe_store::HouseRead {
                    doc_id: doc.clone(),
                    rows: Vec::new(),
                    problems: vec![e],
                });
                continue;
            }
        };
        problems += report.problems.len();
        let rows = report
            .lines
            .into_iter()
            .enumerate()
            .map(|(i, l)| oe_store::HouseTxRow {
                line_no: i as i32 + 1,
                issuer_cik: l
                    .ticker
                    .as_ref()
                    .and_then(|t| by_ticker.get(t).copied().flatten()),
                owner: l.owner,
                asset: l.asset,
                ticker: l.ticker,
                asset_type: l.asset_type,
                tx_type: l.tx_type,
                tx_date: l.tx_date,
                notified_on: l.notified_on,
                amount: l.amount,
                amount_min: l.amount_min,
                amount_max: l.amount_max,
                filing_status: l.filing_status,
                description: l.description,
                comments: l.comments,
                subholding_of: l.subholding_of,
            })
            .collect();
        reads.push(oe_store::HouseRead {
            doc_id: doc.clone(),
            rows,
            problems: report.problems,
        });
    }
    let r = oe_store::replace_house_transactions(&mut client, &reads, PTR_READER)
        .map_err(|e| e.to_string())?;
    println!(
        "House reports: {} typed reports in the ledger, {} read in {:.0} s ({missing} not on disk, {unreadable} unreadable); {} transactions, {} of them linked to a company by ticker, {problems} with a field that could not be read",
        wanted.len(),
        r.reports,
        started.elapsed().as_secs_f64(),
        r.transactions,
        r.with_issuer
    );
    println!("Scanned paper reports are not read (no OCR); they stay listed with their links.");
    println!("{}", oe_officials::USE_LIMITS.lines().next().unwrap_or(""));
    Ok(())
}

pub fn official(argv: &[String]) -> Result<(), String> {
    let query = argv.join(" ");
    if query.trim().is_empty() {
        return Err("official needs a name, for example: oe official pelosi".into());
    }
    let mut client = connect()?;
    let found = oe_store::find_officials(&mut client, &query, 10).map_err(|e| e.to_string())?;
    if found.is_empty() {
        println!("no member of Congress matches {query:?}");
        return Ok(());
    }
    for o in &found {
        println!("\n{} ({})", o.name, o.bioguide);
        for t in o.terms.iter().take(3) {
            println!("  {t}");
        }
        if o.terms.len() > 3 {
            println!("  ... {} earlier term(s)", o.terms.len() - 3);
        }
        if !o.committees.is_empty() {
            let list: Vec<String> = o
                .committees
                .iter()
                .map(|(name, title, _)| match title {
                    Some(t) => format!("{name} ({t})"),
                    None => name.clone(),
                })
                .collect();
            println!(
                "  committees (as of {}): {}",
                o.committees[0].2,
                list.join("; ")
            );
        }
        if o.reports == 0 {
            println!("  no House periodic transaction reports in the ledger");
            continue;
        }
        println!(
            "  {} House periodic transaction report(s), {} to {} ({} scanned paper, not readable as text)",
            o.reports,
            o.first_filed.as_deref().unwrap_or("?"),
            o.last_filed.as_deref().unwrap_or("?"),
            o.scanned_reports
        );
        for (filed, doc, scanned, url) in &o.recent {
            println!(
                "    {filed}  {doc}{}  {url}",
                if *scanned { " (scanned)" } else { "" }
            );
        }
    }
    println!("\nThese reports may not be used commercially (5 U.S.C. 13107); see ADR 0010.");
    Ok(())
}
