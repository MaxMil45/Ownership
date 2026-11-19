//! `oe db-migrate`, `oe db-load` and `oe db-status`: the PostgreSQL ledger.
//!
//! The server is named by the `DATABASE_URL` environment variable
//! (`postgres://user:password@localhost:5432/ownership`). The URL is never
//! printed, because it can contain a password.

use super::find_xml_files;
use oe_store::{LoadReport, PrepareError, Prepared, Sidecar};
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

pub fn status(_args: &[String]) -> Result<(), String> {
    let mut client = connect()?;
    let s = oe_store::status(&mut client).map_err(|e| e.to_string())?;
    println!("migrations applied: {}", s.migrations.join(", "));
    println!(
        "filings: {} ({} amendments, whose rows are not loaded)",
        s.filings, s.amendment_filings
    );
    println!(
        "transactions: {} ({} visible as of now)",
        s.transactions, s.transactions_known_now
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
}

pub fn parse_load_args(args: &[String]) -> Result<LoadArgs, String> {
    let mut folder = None;
    let mut parsed = LoadArgs {
        folder: PathBuf::new(),
        batch: DEFAULT_BATCH,
        dry_run: false,
    };
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--batch" => {
                parsed.batch = it
                    .next()
                    .and_then(|v| v.parse().ok())
                    .filter(|n| *n > 0)
                    .ok_or("--batch needs a positive number")?
            }
            "--dry-run" => parsed.dry_run = true,
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
    let meta = std::fs::read_to_string(xml_path.with_extension("meta.txt"))
        .map_err(|_| Skip::NoSidecar)?;
    let mut sidecar = Sidecar::parse(&meta);
    // Old sidecars may lack the accession line; the file name is the accession.
    if sidecar.accession.is_none() {
        sidecar.accession = xml_path
            .file_stem()
            .map(|s| s.to_string_lossy().into_owned());
    }
    oe_store::prepare(&xml, &sidecar).map_err(Skip::Rejected)
}

pub fn load(args: &[String]) -> Result<(), String> {
    let args = parse_load_args(args)?;
    let files = find_xml_files(&args.folder)?;
    println!(
        "{} XML file(s) under {}",
        files.len(),
        args.folder.display()
    );

    let mut prepared = Vec::with_capacity(files.len());
    let mut skipped: Vec<(PathBuf, Skip)> = Vec::new();
    for path in &files {
        match prepare_file(path) {
            Ok(p) => prepared.push(p),
            Err(why) => skipped.push((path.clone(), why)),
        }
    }
    let rows: usize = prepared.iter().map(|p| p.transactions.len()).sum();
    let held: usize = prepared.iter().map(|p| p.held_back_amendment_rows).sum();
    println!(
        "prepared {} filing(s) with {rows} transaction row(s); {held} amendment row(s) held back; {} skipped",
        prepared.len(),
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
    let mut total = LoadReport::default();
    for (n, batch) in prepared.chunks(args.batch).enumerate() {
        let report = oe_store::load_batch(&mut client, batch).map_err(|e| e.to_string())?;
        println!(
            "batch {}: {} new filing(s), {} new row(s)",
            n + 1,
            report.filings_new,
            report.rows_new
        );
        total.add(report);
    }
    println!(
        "done: {} filing(s) new, {} already present; {} row(s) new, {} already present",
        total.filings_new, total.filings_existing, total.rows_new, total.rows_existing
    );
    if !total.changed_source.is_empty() {
        eprintln!(
            "warning: {} stored filing(s) now hash differently from the file on disk (the source changed, or the file was edited). Nothing was overwritten: {}",
            total.changed_source.len(),
            total.changed_source.iter().take(5).cloned().collect::<Vec<_>>().join(", ")
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::UNPARSED_DIR;

    #[test]
    fn load_args() {
        let a = parse_load_args(&[
            "data/form4".into(),
            "--dry-run".into(),
            "--batch".into(),
            "50".into(),
        ])
        .unwrap();
        assert_eq!(
            a,
            LoadArgs {
                folder: PathBuf::from("data/form4"),
                batch: 50,
                dry_run: true
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
        assert_eq!(files.len(), 24);
        for f in &files {
            prepare_file(f).unwrap_or_else(|e| panic!("{}: {e}", f.display()));
        }
    }
}
