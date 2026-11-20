//! Every real filing in `fixtures/form4/real/` must parse, and what the parser
//! reports must match `MANIFEST.tsv`. The manifest counts come from the raw XML
//! (counting tags), not from the parser, so a parser bug shows up as a mismatch.

use oe_form4::parse;
use std::collections::BTreeSet;
use std::path::PathBuf;

fn real_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/form4/real")
}

struct Row {
    file: String,
    document_type: String,
    owners: usize,
    non_derivative: usize,
    derivative: usize,
    codes: String,
    aff_10b5_1: String,
}

fn manifest() -> Vec<Row> {
    let path = real_dir().join("MANIFEST.tsv");
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert!(c.len() >= 8, "manifest row needs 8 columns: {l:?}");
            Row {
                file: c[0].to_string(),
                document_type: c[1].to_string(),
                owners: c[2].parse().expect("owners"),
                non_derivative: c[3].parse().expect("non_derivative"),
                derivative: c[4].parse().expect("derivative"),
                codes: c[5].to_string(),
                aff_10b5_1: c[6].to_string(),
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
    assert!(on_disk.len() >= 20, "expected about 20 real fixtures");
}

#[test]
fn every_real_fixture_parses_and_matches_the_manifest() {
    for row in manifest() {
        let path = real_dir().join(&row.file);
        let xml = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", row.file));
        let doc = parse(&xml).unwrap_or_else(|e| panic!("{}: {e}", row.file));

        assert_eq!(doc.document_type, row.document_type, "{}: type", row.file);
        assert_eq!(doc.owners.len(), row.owners, "{}: owners", row.file);

        let derivative = doc.transactions.iter().filter(|t| t.is_derivative).count();
        let non_derivative = doc.transactions.len() - derivative;
        assert_eq!(derivative, row.derivative, "{}: derivative rows", row.file);
        assert_eq!(
            non_derivative, row.non_derivative,
            "{}: non-derivative rows",
            row.file
        );

        let codes: BTreeSet<&str> = doc.transactions.iter().map(|t| t.code.as_str()).collect();
        let codes: String = if codes.is_empty() {
            "-".to_string()
        } else {
            codes.into_iter().collect()
        };
        assert_eq!(codes, row.codes, "{}: transaction codes", row.file);

        let plan = match doc.aff_10b5_1 {
            Some(true) => "yes",
            Some(false) => "no",
            None => "-",
        };
        assert_eq!(plan, row.aff_10b5_1, "{}: 10b5-1 box", row.file);

        assert_eq!(
            doc.is_amendment(),
            row.document_type.ends_with("/A"),
            "{}: amendment flag",
            row.file
        );
    }
}

#[test]
fn every_real_fixture_has_a_sidecar_with_known_on() {
    for row in manifest() {
        let meta = real_dir().join(row.file.replace(".xml", ".meta.txt"));
        let text =
            std::fs::read_to_string(&meta).unwrap_or_else(|e| panic!("{}: {e}", meta.display()));
        let field = |key: &str| {
            text.lines()
                .find_map(|l| l.strip_prefix(&format!("{key}=")))
                .unwrap_or("")
        };
        assert_eq!(
            field("known_on").len(),
            10,
            "{}: known_on missing",
            row.file
        );
        assert!(text.contains("accession="), "{}: accession", row.file);
        // `accepted` is the SEC's UTC timestamp; `known_on` must be its US
        // Eastern date (a UTC date would be a day late after about 8 pm ET).
        let eastern = oe_core::us_eastern_date(field("accepted"))
            .unwrap_or_else(|| panic!("{}: accepted is not a timestamp", row.file));
        assert_eq!(field("known_on"), eastern.to_string(), "{}", row.file);
    }
}

#[test]
fn amendments_name_the_date_of_the_report_they_amend() {
    for row in manifest() {
        let xml = std::fs::read_to_string(real_dir().join(&row.file)).unwrap();
        let doc = parse(&xml).unwrap();
        assert_eq!(
            doc.date_of_original_submission.is_some(),
            doc.is_amendment(),
            "{}: dateOfOriginalSubmission",
            row.file
        );
    }
    // The Form 5/A amends the Form 5 next to it, filed 2025-03-12.
    let xml = std::fs::read_to_string(real_dir().join("nvda_0001045810-25-000070.xml")).unwrap();
    assert_eq!(
        parse(&xml)
            .unwrap()
            .date_of_original_submission
            .map(|d| d.to_string()),
        Some("2025-03-12".to_string())
    );
}
