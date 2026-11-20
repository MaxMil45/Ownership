//! Reading downloaded folders into what the event rules take. Shared by the
//! listing commands (`activist-events`, `planned-sales`, ...) and `oe alerts`.
//! A file that cannot be read or parsed is reported on stderr and counted,
//! never fatal.

use super::{find_files, find_xml_files, read_meta};
use oe_core::FilingMeta;
use oe_events::{ContestFiling, InsiderTrade, PlanTerminatedEvent};
use std::path::Path;

pub struct Loaded<T> {
    pub items: Vec<T>,
    pub files: usize,
    pub failed: usize,
}

fn xml_folder<T>(
    folder: &Path,
    parse: impl Fn(&str) -> Result<T, String>,
) -> Result<Loaded<(T, FilingMeta)>, String> {
    let files = find_xml_files(folder)?;
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
pub fn schedule13(folder: &Path) -> Result<Loaded<(oe_sched13::Schedule13, FilingMeta)>, String> {
    xml_folder(folder, |x| oe_sched13::parse(x).map_err(|e| e.to_string()))
}

/// Form 144 notices.
pub fn form144(folder: &Path) -> Result<Loaded<(oe_form144::Form144, FilingMeta)>, String> {
    xml_folder(folder, |x| oe_form144::parse(x).map_err(|e| e.to_string()))
}

/// Every transaction of every Form 3/4/5 under `folder` (all issuers).
pub fn insider_trades(folder: &Path) -> Result<Loaded<InsiderTrade>, String> {
    let docs = xml_folder(folder, |x| oe_form4::parse(x).map_err(|e| e.to_string()))?;
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
pub fn contest_filings(folder: &Path) -> Result<Loaded<ContestFiling>, String> {
    let files = find_files(folder, "sgml")?;
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

/// `insider.plan_terminated@1` events from saved 10-Q/10-K pages, and how
/// many of the reports use the insider-trading (ecd) tags at all.
pub fn plan_termination_events(
    folder: &Path,
) -> Result<(Loaded<PlanTerminatedEvent>, usize), String> {
    let files = find_files(folder, "htm")?;
    let mut items = Vec::new();
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
        items.extend(oe_events::detect_plan_terminations(&doc, &read_meta(path)));
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
