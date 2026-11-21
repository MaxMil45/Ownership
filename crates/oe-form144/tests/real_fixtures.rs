//! Every real filing in `fixtures/form144/real/` must parse, and what the
//! parser reports must match `MANIFEST.tsv`. The manifest counts and sums come
//! from the raw XML (tag counts and text sums), not from the parser, so a parser
//! bug shows up as a mismatch.

use oe_form144::parse;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn real_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/form144/real")
}

struct Row {
    file: String,
    submission_type: String,
    prefix: String,
    securities: usize,
    acquisitions: usize,
    past_sales: usize,
    plan_dates: usize,
    relationships: Vec<String>,
    units: f64,
    value: f64,
}

fn manifest() -> Vec<Row> {
    let path = real_dir().join("MANIFEST.tsv");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert!(c.len() >= 12, "manifest row needs 12 columns: {l:?}");
            Row {
                file: c[0].to_string(),
                submission_type: c[1].to_string(),
                prefix: c[2].to_string(),
                securities: c[3].parse().unwrap(),
                acquisitions: c[4].parse().unwrap(),
                past_sales: c[5].parse().unwrap(),
                plan_dates: c[6].parse().unwrap(),
                relationships: c[7].split('/').map(str::to_string).collect(),
                units: c[8].parse().unwrap(),
                value: c[9].parse().unwrap(),
            }
        })
        .collect()
}

#[test]
fn manifest_lists_every_real_fixture() {
    let on_disk: BTreeSet<String> = std::fs::read_dir(real_dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".xml"))
        .collect();
    let listed: BTreeSet<String> = manifest().into_iter().map(|r| r.file).collect();
    assert_eq!(on_disk, listed, "fixtures and MANIFEST.tsv disagree");
    assert!(on_disk.len() >= 15);
}

#[test]
fn every_real_fixture_parses_and_matches_the_manifest() {
    for row in manifest() {
        let xml = std::fs::read_to_string(real_dir().join(&row.file)).unwrap();
        let f = parse(&xml).unwrap_or_else(|e| panic!("{}: {e}", row.file));
        let name = &row.file;

        assert_eq!(f.submission_type, row.submission_type, "{name}: type");
        assert_eq!(
            f.securities.len(),
            row.securities,
            "{name}: securities blocks"
        );
        assert_eq!(
            f.acquisitions.len(),
            row.acquisitions,
            "{name}: acquisition blocks"
        );
        assert_eq!(
            f.sold_past_3_months.len(),
            row.past_sales,
            "{name}: past sales"
        );
        assert_eq!(
            f.plan_adoption_dates.len(),
            row.plan_dates,
            "{name}: plan dates"
        );
        assert_eq!(f.relationships, row.relationships, "{name}: relationships");
        assert!(
            (f.total_units().unwrap_or(0.0) - row.units).abs() < 0.005,
            "{name}: units {:?} vs {}",
            f.total_units(),
            row.units
        );
        assert!(
            (f.total_market_value().unwrap_or(0.0) - row.value).abs() < 0.005,
            "{name}: value {:?} vs {}",
            f.total_market_value(),
            row.value
        );
        // The namespace style is part of what these fixtures cover.
        let root_prefixed = xml.contains(&format!("<{}:edgarSubmission", row.prefix));
        assert_eq!(root_prefixed, row.prefix != "-", "{name}: prefix");
    }
}

#[test]
fn every_sidecar_has_a_known_on_and_an_accession() {
    for row in manifest() {
        let meta = real_dir().join(row.file.replace(".xml", ".meta.txt"));
        let text = std::fs::read_to_string(&meta).unwrap();
        let field = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("{key}=")))
                .unwrap_or("")
        };
        assert_eq!(field("known_on").len(), 10, "{}: known_on", row.file);
        assert!(
            field("accepted_eastern").starts_with(field("known_on")),
            "{}",
            row.file
        );
        assert!(
            row.file.contains(field("accession")),
            "{}: accession",
            row.file
        );
    }
}

#[test]
fn every_real_fixture_names_its_filer() {
    for row in manifest() {
        let xml = std::fs::read_to_string(real_dir().join(&row.file)).unwrap();
        let f = parse(&xml).unwrap();
        assert!(
            f.filer_cik.is_some(),
            "{}: no filerCredentials/cik",
            row.file
        );
    }
    // The first fixture's filer, read by hand from its XML.
    let first = manifest().into_iter().next().unwrap();
    let xml = std::fs::read_to_string(real_dir().join(&first.file)).unwrap();
    assert!(xml.contains(&format!(
        "<cik>{:010}</cik>",
        parse(&xml).unwrap().filer_cik.unwrap().0
    )));
}
