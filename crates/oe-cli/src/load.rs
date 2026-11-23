//! Reading downloaded folders into what the event rules take. Shared by the
//! listing commands (`activist-events`, `planned-sales`, ...) and `oe alerts`.
//! A file that cannot be read or parsed is reported on stderr and counted,
//! never fatal.

use super::{find_files, read_meta};
use oe_core::FilingMeta;
use oe_events::{ContestFiling, InsiderTrade, PlanTerminatedEvent};
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};

/// Only these files, when given (see `alerts::Sources::only`).
pub type Only<'a> = Option<&'a BTreeSet<PathBuf>>;

/// The files with extension `ext` under `folder`: from `only` when given
/// (no walk of the folder), else every one found.
fn files_in(folder: &Path, ext: &str, only: Only) -> Result<Vec<PathBuf>, String> {
    match only {
        None => find_files(folder, ext),
        Some(set) => Ok(set
            .iter()
            .filter(|p| p.starts_with(folder))
            .filter(|p| p.extension().is_some_and(|e| e.eq_ignore_ascii_case(ext)))
            .cloned()
            .collect()),
    }
}

pub struct Loaded<T> {
    pub items: Vec<T>,
    pub files: usize,
    pub failed: usize,
}

fn xml_folder<T>(
    folder: &Path,
    only: Only,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<Loaded<(T, FilingMeta)>, String> {
    let files = files_in(folder, "xml", only)?;
    let mut items = Vec::new();
    let mut failed = 0usize;
    for path in &files {
        let parsed = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|xml| parse(&xml));
        match parsed {
            Ok(x) => items.push((x, read_meta(path))),
            Err(e) => {
                failed += 1;
                eprintln!("  {}: {e}", path.display());
            }
        }
    }
    Ok(Loaded {
        items,
        files: files.len(),
        failed,
    })
}

/// Schedule 13D/13G primary documents.
pub fn schedule13(
    folder: &Path,
    only: Only,
) -> Result<Loaded<(oe_sched13::Schedule13, FilingMeta)>, String> {
    xml_folder(folder, only, |x| {
        oe_sched13::parse(x).map_err(|e| e.to_string())
    })
}

/// Form 144 notices.
pub fn form144(
    folder: &Path,
    only: Only,
) -> Result<Loaded<(oe_form144::Form144, FilingMeta)>, String> {
    xml_folder(folder, only, |x| {
        oe_form144::parse(x).map_err(|e| e.to_string())
    })
}

/// Every transaction of every Form 3/4/5 under `folder` (all issuers).
pub fn insider_trades(folder: &Path, only: Only) -> Result<Loaded<InsiderTrade>, String> {
    let docs = xml_folder(folder, only, |x| {
        oe_form4::parse(x).map_err(|e| e.to_string())
    })?;
    let items = docs
        .items
        .iter()
        .flat_map(|(doc, meta)| oe_events::trades_from_document(doc, meta))
        .collect();
    Ok(Loaded {
        items,
        files: docs.files,
        failed: docs.failed,
    })
}

/// Contest filings from saved SGML headers. Headers without a subject
/// company count as failed.
pub fn contest_filings(folder: &Path, only: Only) -> Result<Loaded<ContestFiling>, String> {
    let files = files_in(folder, "sgml", only)?;
    let mut items = Vec::new();
    let mut failed = 0usize;
    for path in &files {
        let header = std::fs::read_to_string(path)
            .map_err(|e| e.to_string())
            .and_then(|t| oe_edgar::parse_header_sgml(&t).map_err(|e| e.to_string()));
        match header {
            Ok(h) => match h.subject.clone() {
                Some(subject) => items.push(ContestFiling {
                    known_on: h.known_on(),
                    form: h.form,
                    accession: h.accession,
                    subject,
                    filed_by: h.filed_by,
                }),
                None => failed += 1,
            },
            Err(e) => {
                failed += 1;
                eprintln!("  {}: {e}", path.display());
            }
        }
    }
    Ok(Loaded {
        items,
        files: files.len(),
        failed,
    })
}

/// Plan-termination events from saved 10-Q/10-K pages, and how many of the
/// reports use the insider-trading (ecd) tags at all. `insider.plan_terminated@1`,
/// or with `with_db` `@2`, which looks up earlier adoptions in the ledger.
pub fn plan_termination_events(
    folder: &Path,
    with_db: bool,
) -> Result<(Loaded<PlanTerminatedEvent>, usize), String> {
    let files = find_files(folder, "htm")?;
    let mut docs = Vec::new();
    let (mut with_tags, mut failed) = (0usize, 0usize);
    for path in &files {
        let html = match std::fs::read_to_string(path) {
            Ok(h) => h,
            Err(e) => {
                failed += 1;
                eprintln!("  {}: {e}", path.display());
                continue;
            }
        };
        let doc = oe_item408::extract(&html);
        with_tags += usize::from(doc.uses_ecd_tags);
        docs.push((doc, read_meta(path)));
    }
    let mut items = Vec::new();
    if with_db {
        let mut issuers: Vec<u64> = docs
            .iter()
            .filter_map(|(d, _)| d.cik.map(|c| c.0))
            .collect();
        issuers.sort_unstable();
        issuers.dedup();
        let earlier = super::alerts::ledger_adoptions(&issuers)?;
        for (doc, meta) in &docs {
            items.extend(oe_events::detect_plan_terminations_with_ledger(
                doc, meta, &earlier,
            ));
        }
    } else {
        for (doc, meta) in &docs {
            items.extend(oe_events::detect_plan_terminations(doc, meta));
        }
    }
    Ok((
        Loaded {
            items,
            files: files.len(),
            failed,
        },
        with_tags,
    ))
}
