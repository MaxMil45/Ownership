//! `oe periodic-excerpt [<folder>] [--dry-run]`: replaces each saved full
//! 10-Q/10-K report with its Item 408 excerpt (`oe_item408::excerpt`), offline.
//!
//! For each `.htm`: the full report's SHA-256 goes into its `.meta.txt` as
//! `original_sha256` (so the ledger's fingerprint stays the SEC original's),
//! the excerpt is checked to read exactly as the report does, and only then is
//! the report replaced. A report that does not read the same stops the run and
//! is left untouched. Files that are already excerpts are skipped.

use super::{find_files, meta_path};
use std::path::{Path, PathBuf};

/// The first line every excerpt starts with.
const EXCERPT_MARK: &str = "<!-- Excerpt of an SEC 10-Q/10-K";

pub fn periodic_excerpt(args: &[String]) -> Result<(), String> {
    let dry_run = args.iter().any(|a| a == "--dry-run");
    let rest: Vec<&String> = args.iter().filter(|a| *a != "--dry-run").collect();
    let folder = match rest.as_slice() {
        [] => PathBuf::from("data/periodic"),
        [one] => PathBuf::from(one),
        _ => return Err("periodic-excerpt takes one folder, for example data/periodic".into()),
    };
    let files = find_files(&folder, "htm")?;
    let (mut done, mut already, mut before, mut after) = (0usize, 0usize, 0u64, 0u64);
    for path in &files {
        match shrink(path, dry_run)? {
            None => already += 1,
            Some((full, small)) => {
                done += 1;
                before += full as u64;
                after += small as u64;
            }
        }
    }
    println!(
        "{} report(s): {done} {} to excerpts ({} MB to {} MB), {already} already excerpts{}",
        files.len(),
        if dry_run { "would be cut" } else { "cut" },
        before >> 20,
        after >> 20,
        if dry_run {
            "; dry run, nothing written"
        } else {
            ""
        }
    );
    Ok(())
}

/// Replaces one report with its excerpt. `None` if it already is one.
fn shrink(path: &Path, dry_run: bool) -> Result<Option<(usize, usize)>, String> {
    let shown = || path.display().to_string();
    let html = std::fs::read_to_string(path).map_err(|e| format!("{}: {e}", shown()))?;
    if html.starts_with(EXCERPT_MARK) {
        return Ok(None);
    }
    let excerpt = oe_item408::excerpt(&html);
    if oe_item408::extract(&excerpt) != oe_item408::extract(&html) {
        return Err(format!(
            "{}: its excerpt does not read the same; left as it is, run stopped",
            shown()
        ));
    }
    if dry_run {
        return Ok(Some((html.len(), excerpt.len())));
    }
    // The sidecar first: an interrupted run leaves a full report with its
    // fingerprint noted, which is still correct.
    let meta = meta_path(path);
    let side = std::fs::read_to_string(&meta).map_err(|e| format!("{}: {e}", meta.display()))?;
    if !side.lines().any(|l| l.starts_with("original_sha256=")) {
        let mut side = side;
        if !side.ends_with('\n') {
            side.push('\n');
        }
        side.push_str(&format!(
            "original_sha256={}\n",
            oe_core::sha256_hex(html.as_bytes())
        ));
        std::fs::write(&meta, side).map_err(|e| format!("{}: {e}", meta.display()))?;
    }
    let tmp = path.with_extension("htm.tmp");
    std::fs::write(&tmp, &excerpt).map_err(|e| format!("{}: {e}", tmp.display()))?;
    std::fs::rename(&tmp, path).map_err(|e| format!("{}: {e}", shown()))?;
    Ok(Some((html.len(), excerpt.len())))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_report_becomes_its_excerpt_once_with_its_fingerprint_noted() {
        let src = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../../fixtures/item408/real/acadia_0001193125-26-447794.htm");
        let dir = std::env::temp_dir().join(format!("oe-shrink-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Make it look like a full report: the excerpt mark must not be first.
        let full = format!(
            "<html><!-- full -->{}</html>",
            std::fs::read_to_string(&src).unwrap()
        );
        let path = dir.join("0001193125-26-447794.htm");
        std::fs::write(&path, &full).unwrap();
        std::fs::write(meta_path(&path), "accession=0001193125-26-447794").unwrap();

        assert!(shrink(&path, true).unwrap().is_some());
        assert_eq!(std::fs::read_to_string(&path).unwrap(), full, "dry run");

        let (before, after) = shrink(&path, false).unwrap().unwrap();
        assert!(after < before);
        let now = std::fs::read_to_string(&path).unwrap();
        assert!(now.starts_with(EXCERPT_MARK));
        assert_eq!(oe_item408::extract(&now), oe_item408::extract(&full));
        let side = std::fs::read_to_string(meta_path(&path)).unwrap();
        assert_eq!(
            side,
            format!(
                "accession=0001193125-26-447794\noriginal_sha256={}\n",
                oe_core::sha256_hex(full.as_bytes())
            )
        );
        // Again: nothing to do, the sidecar is not touched twice.
        assert_eq!(shrink(&path, false).unwrap(), None);
        assert_eq!(std::fs::read_to_string(meta_path(&path)).unwrap(), side);
        std::fs::remove_dir_all(&dir).unwrap();
    }
}
