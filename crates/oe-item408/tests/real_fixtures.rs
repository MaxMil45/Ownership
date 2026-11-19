//! Every excerpt in `fixtures/item408/real/` (real 10-Q and 10-K reports,
//! reduced to their XBRL contexts and `dei`/`ecd` facts) must read the way
//! `MANIFEST.tsv` says. The manifest numbers come from regexes over the
//! **original full reports**, not from this crate, so a disagreement is a bug
//! in one of them.

use oe_item408::extract;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/item408/real")
}

struct Row {
    file: String,
    form: String,
    registrant: String,
    cik: u64,
    arrangements: usize,
    adopted: usize,
    terminated: usize,
}

fn manifest() -> Vec<Row> {
    let text = std::fs::read_to_string(dir().join("MANIFEST.tsv")).unwrap();
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert!(c.len() >= 10, "manifest row needs 10+ columns: {l:?}");
            Row {
                file: c[0].to_string(),
                form: c[1].to_string(),
                registrant: c[2].to_string(),
                cik: c[3].parse().unwrap(),
                arrangements: c[5].parse().unwrap(),
                adopted: c[6].parse().unwrap(),
                terminated: c[7].parse().unwrap(),
            }
        })
        .collect()
}

#[test]
fn manifest_lists_every_fixture() {
    let on_disk: BTreeSet<String> = std::fs::read_dir(dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".htm"))
        .collect();
    let listed: BTreeSet<String> = manifest().into_iter().map(|r| r.file).collect();
    assert_eq!(on_disk, listed, "fixtures and MANIFEST.tsv disagree");
    assert!(on_disk.len() >= 10);
}

#[test]
fn every_fixture_reads_the_way_the_manifest_says() {
    for row in manifest() {
        let html = std::fs::read_to_string(dir().join(&row.file)).unwrap();
        let doc = extract(&html);
        let name = &row.file;
        assert_eq!(
            doc.document_type.as_deref(),
            Some(row.form.as_str()),
            "{name}: form"
        );
        assert_eq!(
            doc.registrant.as_deref(),
            Some(row.registrant.as_str()),
            "{name}: registrant"
        );
        assert_eq!(doc.cik.map(|c| c.0), Some(row.cik), "{name}: cik");
        assert!(doc.uses_ecd_tags, "{name}: ecd tags");
        assert_eq!(
            doc.arrangements.len(),
            row.arrangements,
            "{name}: arrangements"
        );
        let adopted = doc
            .arrangements
            .iter()
            .filter(|a| a.rule_10b5_1_adopted)
            .count();
        let terminated = doc
            .arrangements
            .iter()
            .filter(|a| a.rule_10b5_1_terminated)
            .count();
        assert_eq!(adopted, row.adopted, "{name}: adopted");
        assert_eq!(terminated, row.terminated, "{name}: terminated");
    }
}

#[test]
fn the_set_covers_both_tagging_styles_and_both_forms() {
    let docs: Vec<_> = manifest()
        .iter()
        .map(|r| extract(&std::fs::read_to_string(dir().join(&r.file)).unwrap()))
        .collect();
    let plan_named = docs
        .iter()
        .flat_map(|d| &d.arrangements)
        .any(|a| a.arrangement.is_some());
    let person_only = docs
        .iter()
        .flat_map(|d| &d.arrangements)
        .any(|a| a.arrangement.is_none());
    assert!(plan_named && person_only);
    let forms: BTreeSet<&str> = docs
        .iter()
        .filter_map(|d| d.document_type.as_deref())
        .collect();
    assert!(forms.contains("10-Q") && forms.contains("10-K"));
}

#[test]
fn sidecars_carry_the_acceptance_date_not_the_filing_date() {
    for row in manifest() {
        let meta =
            std::fs::read_to_string(dir().join(row.file.replace(".htm", ".meta.txt"))).unwrap();
        let field = |key: &str| {
            meta.lines()
                .find_map(|l| l.strip_prefix(&format!("{key}=")))
                .unwrap_or("")
                .to_string()
        };
        assert!(row.file.contains(&field("accession")), "{}", row.file);
        assert!(
            field("accepted_eastern").starts_with(&field("known_on")),
            "{}",
            row.file
        );
    }
    // ACADIA: accepted 18:16 Eastern on 4 November, EDGAR filing date 5 November.
    let meta = std::fs::read_to_string(dir().join("acadia_0001193125-26-447794.meta.txt")).unwrap();
    assert!(meta.contains("known_on=2026-11-04") && meta.contains("filing_date=2026-11-05"));
}
