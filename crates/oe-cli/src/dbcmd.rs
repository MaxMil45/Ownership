//! `oe db-migrate`, `oe db-load` and `oe db-status`: the PostgreSQL ledger.
//!
//! The server is named by the `DATABASE_URL` environment variable
//! (`postgres://user:password@localhost:5432/ownership`). The URL is never
//! printed, because it can contain a password.

use super::find_files;
use oe_store::{LoadReport, OtherDoc, OtherFiling, OtherReport, PrepareError, Prepared, Sidecar};
use std::path::{Path, PathBuf};

const DEFAULT_BATCH: usize = 500;

fn database_url() -> Result<String, String> {
    std::env::var("DATABASE_URL").map_err(|_| {
        "set DATABASE_URL to your PostgreSQL connection string, for example \
         postgres://user:password@localhost:5432/ownership"
            .to_string()
    })
}

fn connect() -> Result<oe_store::Client, String> {
    oe_store::connect(&database_url()?).map_err(|e| e.to_string())
}

pub fn migrate(_args: &[String]) -> Result<(), String> {
    let mut client = connect()?;
    let applied = oe_store::migrate(&mut client).map_err(|e| e.to_string())?;
    if applied.is_empty() {
        println!(
            "schema is up to date ({} migration(s) known)",
            oe_store::MIGRATIONS.len()
        );
    } else {
        println!("applied: {}", applied.join(", "));
    }
    Ok(())
}

pub fn maintain(args: &[String]) -> Result<(), String> {
    if let Some(a) = args.first() {
        return Err(format!("unexpected argument {a:?}"));
    }
    let mut client = connect()?;
    let started = std::time::Instant::now();
    let r = oe_store::maintain(&mut client).map_err(|e| e.to_string())?;
    println!(
        "vacuumed and analyzed every table in {:.0} s; database size {}",
        started.elapsed().as_secs_f64(),
        r.database_size
    );
    Ok(())
}

pub fn status(_args: &[String]) -> Result<(), String> {
    let mut client = connect()?;
    let s = oe_store::status(&mut client).map_err(|e| e.to_string())?;
    println!("migrations applied: {}", s.migrations.join(", "));
    println!(
        "filings: {} ({} from the quarterly bulk data sets, {} joint filings with more than one owner)",
        s.filings, s.bulk_filings, s.joint_filings
    );
    println!(
        "amendments: {} ({} linked and loaded, {} linked but held back, {} ambiguous, {} waiting for the report they amend)",
        s.amendment_filings,
        s.amendments_loaded,
        s.amendments_held_back,
        s.amendments_ambiguous,
        s.amendments_waiting
    );
    println!(
        "transactions: {} ({} visible as of now)",
        s.transactions, s.transactions_known_now
    );
    println!(
        "other forms: {} Form 144 notices, {} Schedule 13D/G filings, {} contest filings, {} 10-Q/10-K trading arrangements",
        s.form144_notices, s.schedule13_filings, s.contest_filings, s.trading_arrangements
    );
    println!(
        "accepted between {} and {}",
        s.earliest_accepted.as_deref().unwrap_or("-"),
        s.latest_accepted.as_deref().unwrap_or("-")
    );
    Ok(())
}

#[derive(Debug, PartialEq)]
pub struct LoadArgs {
    pub folder: PathBuf,
    pub batch: usize,
    pub dry_run: bool,
    /// Only files written to disk in the last this many days.
    pub changed_within_days: Option<u64>,
}

pub fn parse_load_args(args: &[String]) -> Result<LoadArgs, String> {
    let mut folder = None;
    let mut parsed = LoadArgs {
        folder: PathBuf::new(),
        batch: DEFAULT_BATCH,
        dry_run: false,
        changed_within_days: None,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--batch" => parsed.batch = super::args::positive(&mut it, "--batch")?,
            "--dry-run" => parsed.dry_run = true,
            "--changed-within" => {
                parsed.changed_within_days =
                    Some(super::args::positive::<usize>(&mut it, "--changed-within")? as u64)
            }
            other if folder.is_none() => folder = Some(PathBuf::from(other)),
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    parsed.folder = folder.ok_or("db-load needs a folder, for example data/form4")?;
    Ok(parsed)
}

/// Why a file was not prepared, for the summary.
#[derive(Debug)]
enum Skip {
    NoSidecar,
    Unreadable(String),
    Rejected(PrepareError),
}

impl std::fmt::Display for Skip {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Skip::NoSidecar => f.write_str("no .meta.txt sidecar next to it"),
            Skip::Unreadable(e) => write!(f, "could not read: {e}"),
            Skip::Rejected(e) => write!(f, "{e}"),
        }
    }
}

fn prepare_file(xml_path: &Path) -> Result<Prepared, Skip> {
    let xml = std::fs::read_to_string(xml_path).map_err(|e| Skip::Unreadable(e.to_string()))?;
    let meta = std::fs::read_to_string(super::meta_path(xml_path)).map_err(|_| Skip::NoSidecar)?;
    let mut sidecar = Sidecar::parse(&meta);
    // Old sidecars may lack the accession line; the file name is the accession.
    if sidecar.accession.is_none() {
        sidecar.accession = xml_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned());
    }
    oe_store::prepare(&xml, &sidecar).map_err(Skip::Rejected)
}

/// A downloaded file, ready for the ledger.
enum Loadable {
    /// A Form 3/4/5 (the insider transaction tables).
    Ownership(Prepared),
    /// A Form 144, Schedule 13D/G, contest header or 10-Q/10-K (ADR 0009).
    Other(OtherFiling),
}

/// The name of an XML document's root element, without a namespace prefix.
fn root_element(xml: &str) -> Option<&str> {
    let mut rest = xml.trim_start_matches('\u{feff}');
    loop {
        let start = rest.find('<')?;
        rest = &rest[start + 1..];
        if rest.starts_with('?') || rest.starts_with('!') {
            continue; // declaration, comment, doctype
        }
        let end = rest.find(|c: char| c.is_whitespace() || c == '>' || c == '/')?;
        let name = &rest[..end];
        return Some(name.rsplit(':').next().unwrap_or(name));
    }
}

fn sidecar_for(path: &Path) -> Result<Sidecar, Skip> {
    let meta = std::fs::read_to_string(super::meta_path(path)).map_err(|_| Skip::NoSidecar)?;
    let mut sidecar = Sidecar::parse(&meta);
    if sidecar.accession.is_none() {
        sidecar.accession = accession_of(path);
    }
    Ok(sidecar)
}

/// `0001-26-000001.xml` or `0001-26-000001.hdr.sgml` -> `0001-26-000001`.
fn accession_of(path: &Path) -> Option<String> {
    let name = path.file_name()?.to_string_lossy();
    Some(name.split('.').next()?.to_string())
}

/// Reads one downloaded file of any kind `oe` saves.
fn prepare_any(path: &Path) -> Result<Loadable, Skip> {
    let name = path
        .file_name()
        .map(|n| n.to_string_lossy().to_ascii_lowercase())
        .unwrap_or_default();
    let bytes = std::fs::read(path).map_err(|e| Skip::Unreadable(e.to_string()))?;
    let text = String::from_utf8_lossy(&bytes);
    let other = |doc: OtherDoc, side: &Sidecar, default_url: String| -> Result<Loadable, Skip> {
        let accession = side
            .accession
            .clone()
            .ok_or(Skip::Rejected(PrepareError::NoAccession))?;
        let accepted = side
            .accepted_at()
            .ok_or(Skip::Rejected(PrepareError::NoAcceptanceTime))?;
        let url = side.url.clone().unwrap_or(default_url);
        let mut prepared = oe_store::prepare_other(doc, &accession, accepted, url, &bytes)
            .map_err(Skip::Rejected)?;
        // An excerpt: the receipt keeps the full original's fingerprint.
        if let Some(original) = &side.original_sha256 {
            prepared.filing.raw_sha256 = original.clone();
        }
        Ok(Loadable::Other(prepared))
    };
    if name.ends_with(".hdr.sgml") {
        // Contest headers carry their own acceptance time; no sidecar.
        let h = oe_edgar::parse_header_sgml(&text)
            .map_err(|e| Skip::Rejected(PrepareError::Parse(e.to_string())))?;
        let subject = h
            .subject
            .clone()
            .ok_or_else(|| Skip::Rejected(PrepareError::Parse("no subject company".into())))?;
        let side = Sidecar {
            accession: Some(h.accession.clone()),
            accepted_eastern: Some(h.accepted_eastern.clone()),
            ..Sidecar::default()
        };
        let url = oe_edgar::filing_folder_url(subject.cik.0, &h.accession.replace('-', ""));
        let doc = OtherDoc::Contest(oe_store::ContestHeader {
            form: h.form,
            subject,
            filed_by: h.filed_by,
        });
        return other(doc, &side, url);
    }
    if name.ends_with(".htm") || name.ends_with(".html") {
        let side = sidecar_for(path)?;
        let doc = OtherDoc::Item408 {
            doc: oe_item408::extract(&text),
            report_type: side.form.clone(),
        };
        return other(doc, &side, String::new());
    }
    match root_element(&text) {
        Some("ownershipDocument") => prepare_file(path).map(Loadable::Ownership),
        Some("edgarSubmission") => {
            let side = sidecar_for(path)?;
            let doc = if let Ok(f) = oe_form144::parse(&text) {
                OtherDoc::Form144(f)
            } else {
                let s = oe_sched13::parse(&text)
                    .map_err(|e| Skip::Rejected(PrepareError::Parse(e.to_string())))?;
                OtherDoc::Schedule13(s)
            };
            other(doc, &side, String::new())
        }
        root => Err(Skip::Rejected(PrepareError::Parse(format!(
            "not a filing oe loads (root element {root:?})"
        )))),
    }
}

/// Every file under `folder` that `oe` downloads: `.xml`, `.hdr.sgml`, `.htm`.
fn loadable_files(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let mut files = find_files(folder, "xml")?;
    files.extend(find_files(folder, "sgml")?);
    files.extend(find_files(folder, "htm")?);
    files.sort();
    Ok(files)
}

/// What one load did, for both kinds of table.
#[derive(Debug, Default)]
pub struct BothReports {
    pub ownership: LoadReport,
    pub other: OtherReport,
}

fn load_prepared(
    client: &mut oe_store::Client,
    items: Vec<Loadable>,
    batch: usize,
    verbose: bool,
) -> Result<BothReports, String> {
    let mut ownership = Vec::new();
    let mut others = Vec::new();
    for item in items {
        match item {
            Loadable::Ownership(p) => ownership.push(p),
            Loadable::Other(o) => others.push(o),
        }
    }
    let mut out = BothReports::default();
    for (n, chunk) in ownership.chunks(batch.max(1)).enumerate() {
        let r = oe_store::load_batch(client, chunk).map_err(|e| e.to_string())?;
        if verbose {
            println!(
                "Form 3/4/5 batch {}: {} new filing(s), {} new row(s)",
                n + 1,
                r.filings_new,
                r.rows_new
            );
        }
        out.ownership.add(r);
    }
    for chunk in others.chunks(batch.max(1)) {
        let r = oe_store::load_other(client, chunk).map_err(|e| e.to_string())?;
        out.other.filings_new += r.filings_new;
        out.other.filings_existing += r.filings_existing;
        out.other.rows_new += r.rows_new;
        out.other.changed_source.extend(r.changed_source);
    }
    Ok(out)
}

/// Loads the given downloaded files (any kind) in one go: what
/// `oe follow --db` calls after each poll. Files that cannot be prepared are
/// reported and skipped.
pub fn load_paths(paths: &[PathBuf]) -> Result<BothReports, String> {
    let mut items = Vec::with_capacity(paths.len());
    for path in paths {
        match prepare_any(path) {
            Ok(p) => items.push(p),
            Err(why) => eprintln!("  not loaded {}: {why}", path.display()),
        }
    }
    let mut client = connect()?;
    oe_store::check_schema(&mut client).map_err(|e| e.to_string())?;
    load_prepared(&mut client, items, DEFAULT_BATCH, false)
}

pub fn load(args: &[String]) -> Result<(), String> {
    let args = parse_load_args(args)?;
    let mut files = loadable_files(&args.folder)?;
    if let Some(days) = args.changed_within_days {
        let cutoff = std::time::SystemTime::now()
            .checked_sub(std::time::Duration::from_secs(days * 86_400))
            .ok_or("--changed-within is too large")?;
        files.retain(|f| {
            std::fs::metadata(f)
                .and_then(|m| m.modified())
                .is_ok_and(|t| t >= cutoff)
        });
    }
    println!("{} file(s) under {}", files.len(), args.folder.display());

    let mut items = Vec::with_capacity(files.len());
    let mut skipped: Vec<(PathBuf, Skip)> = Vec::new();
    for path in &files {
        match prepare_any(path) {
            Ok(p) => items.push(p),
            Err(why) => skipped.push((path.clone(), why)),
        }
    }
    let ownership: Vec<&Prepared> = items
        .iter()
        .filter_map(|i| match i {
            Loadable::Ownership(p) => Some(p),
            Loadable::Other(_) => None,
        })
        .collect();
    let rows: usize = ownership.iter().map(|p| p.ledger_rows().len()).sum();
    let amendments = ownership.iter().filter(|p| p.is_amendment()).count();
    let held: usize = ownership.iter().map(|p| p.amendment_rows().len()).sum();
    let mut kinds: std::collections::BTreeMap<&str, usize> = std::collections::BTreeMap::new();
    for i in &items {
        if let Loadable::Other(o) = i {
            *kinds.entry(o.doc.kind()).or_default() += 1;
        }
    }
    let others = if kinds.is_empty() {
        "no other forms".to_string()
    } else {
        kinds
            .iter()
            .map(|(k, n)| format!("{n} {k}"))
            .collect::<Vec<_>>()
            .join(", ")
    };
    println!(
        "prepared {} Form 3/4/5 filing(s) with {rows} transaction row(s) ({amendments} amendment(s) with {held} row(s) that load only once linked); {others}; {} skipped",
        ownership.len(),
        skipped.len()
    );
    for (path, why) in skipped.iter().take(10) {
        eprintln!("  skipped {}: {why}", path.display());
    }
    if skipped.len() > 10 {
        eprintln!("  ... and {} more", skipped.len() - 10);
    }

    if args.dry_run {
        println!("dry run: nothing was written to a database");
        return Ok(());
    }

    let mut client = connect()?;
    oe_store::check_schema(&mut client).map_err(|e| e.to_string())?;
    let both = load_prepared(&mut client, items, args.batch, true)?;
    let total = &both.ownership;
    println!(
        "done: {} Form 3/4/5 filing(s) new, {} already present; {} row(s) new, {} already present",
        total.filings_new, total.filings_existing, total.rows_new, total.rows_existing
    );
    let l = &total.links;
    println!(
        "amendments: {} linked and loaded ({} row(s)), {} linked but held back, {} ambiguous; {} in the database still waiting for the report they amend",
        l.loaded, l.rows_loaded, l.held_back, l.ambiguous, l.waiting
    );
    let o = &both.other;
    println!(
        "other forms: {} filing(s) new, {} already present, {} content row(s) new",
        o.filings_new, o.filings_existing, o.rows_new
    );
    let changed: Vec<&String> = total
        .changed_source
        .iter()
        .chain(&o.changed_source)
        .collect();
    if !changed.is_empty() {
        eprintln!(
            "warning: {} stored filing(s) now hash differently from the file on disk (the source changed, or the file was edited). Nothing was overwritten: {}",
            changed.len(),
            changed.iter().take(5).map(|s| s.as_str()).collect::<Vec<_>>().join(", ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{find_xml_files, UNPARSED_DIR};

    #[test]
    fn load_args() {
        let a = parse_load_args(&[
            "data/form4".into(),
            "--dry-run".into(),
            "--batch".into(),
            "50".into(),
            "--changed-within".into(),
            "3".into(),
        ])
        .unwrap();
        assert_eq!(
            a,
            LoadArgs {
                folder: PathBuf::from("data/form4"),
                batch: 50,
                dry_run: true,
                changed_within_days: Some(3)
            }
        );
        assert!(parse_load_args(&[]).is_err());
        assert!(parse_load_args(&["a".into(), "b".into()]).is_err());
        assert!(parse_load_args(&["a".into(), "--batch".into(), "0".into()]).is_err());
    }

    #[test]
    fn finds_xml_recursively_and_skips_unparsed() {
        let root = std::env::temp_dir().join(format!("oe-dbcmd-test-{}", std::process::id()));
        let issuer = root.join("0000000001");
        let bad = root.join(UNPARSED_DIR);
        std::fs::create_dir_all(&issuer).unwrap();
        std::fs::create_dir_all(&bad).unwrap();
        std::fs::write(issuer.join("a.xml"), "x").unwrap();
        std::fs::write(issuer.join("a.meta.txt"), "x").unwrap();
        std::fs::write(bad.join("b.xml"), "x").unwrap();
        std::fs::write(root.join("c.XML"), "x").unwrap();
        let found = find_xml_files(&root).unwrap();
        std::fs::remove_dir_all(&root).unwrap();
        let names: Vec<String> = found
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        // Sorted by full path: the issuer folder "0000000001" sorts before "c.XML".
        assert_eq!(names, ["a.xml", "c.XML"]);
    }

    #[test]
    fn real_fixtures_prepare_through_the_cli_path() {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/form4/real");
        let files = find_xml_files(&dir).unwrap();
        assert_eq!(files.len(), 25);
        for f in &files {
            prepare_file(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        }
    }
}

#[cfg(test)]
mod loadable_tests {
    use super::*;

    fn fixtures(kind: &str) -> Vec<PathBuf> {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures")
            .join(kind)
            .join("real");
        loadable_files(&dir).unwrap()
    }

    #[test]
    fn root_elements_ignore_declarations_and_prefixes() {
        assert_eq!(
            root_element("<?xml version=\"1.0\"?><!-- x --><own:edgarSubmission xmlns:own=\"u\">"),
            Some("edgarSubmission")
        );
        assert_eq!(
            root_element("\u{feff}<ownershipDocument>"),
            Some("ownershipDocument")
        );
        assert_eq!(root_element("no xml here"), None);
    }

    #[test]
    fn an_excerpt_keeps_the_fingerprint_of_its_full_original() {
        let src = &fixtures("item408")[0];
        let dir = std::env::temp_dir().join(format!("oe-excerpt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join(src.file_name().unwrap());
        std::fs::copy(src, &file).unwrap();
        let meta = std::fs::read_to_string(super::super::meta_path(src)).unwrap();
        let prepared = |meta: &str| {
            std::fs::write(super::super::meta_path(&file), meta).unwrap();
            match prepare_any(&file) {
                Ok(Loadable::Other(o)) => o.filing.raw_sha256,
                Ok(_) => panic!("not read as a 10-Q/10-K"),
                Err(e) => panic!("{e:?}"),
            }
        };
        let own = oe_core::sha256_hex(&std::fs::read(&file).unwrap());
        assert_eq!(prepared(&meta), own, "no original named: the file's own");
        let original = "ab".repeat(32);
        assert_eq!(
            prepared(&format!("{meta}original_sha256={original}\n")),
            original
        );
        // Not a SHA-256: ignored.
        assert_eq!(prepared(&format!("{meta}original_sha256=xyz\n")), own);
        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn every_real_fixture_of_every_kind_is_recognised() {
        for (kind, expected) in [
            ("form144", "Form 144"),
            ("sched13", "Schedule 13D/G"),
            ("proxy", "contest filing"),
            ("item408", "10-Q/10-K"),
        ] {
            let files = fixtures(kind);
            assert!(!files.is_empty(), "{kind}");
            for f in &files {
                match prepare_any(f) {
                    Ok(Loadable::Other(o)) => {
                        assert_eq!(o.doc.kind(), expected, "{}", f.display());
                        assert_eq!(o.filing.accession.len(), 20, "{}", f.display());
                        assert!(!o.filing.accepted_at.is_empty(), "{}", f.display());
                        assert_eq!(o.filing.raw_sha256.len(), 64);
                    }
                    Ok(Loadable::Ownership(_)) => panic!("{} read as a Form 4", f.display()),
                    Err(e) => panic!("{}: {e}", f.display()),
                }
            }
        }
        // And Forms 3/4/5 still go to the insider tables.
        for f in fixtures("form4") {
            assert!(
                matches!(prepare_any(&f), Ok(Loadable::Ownership(_))),
                "{}",
                f.display()
            );
        }
    }
}
