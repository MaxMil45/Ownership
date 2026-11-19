//! Every real filing in `fixtures/sched13/real/` must parse, and what the
//! parser reports must match `MANIFEST.tsv`. The manifest values come from
//! regexes over the raw XML (tag counts, maximums, flags), not from the
//! parser, so a parser bug shows up as a mismatch.

use oe_sched13::{parse, Schedule};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sched13/real")
}

struct Row {
    file: String,
    submission_type: String,
    prefixed: bool,
    persons: usize,
    issuer_cik: u64,
    largest_percent: Option<f64>,
    previous_flag: Option<bool>,
    amendment_no: Option<u32>,
    previous_accession: Option<String>,
    rules: Vec<String>,
    group_persons: usize,
}

fn dash(s: &str) -> Option<&str> {
    (s != "-").then_some(s)
}

fn manifest() -> Vec<Row> {
    let text = std::fs::read_to_string(dir().join("MANIFEST.tsv")).unwrap();
    text.lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| {
            let c: Vec<&str> = l.split('\t').collect();
            assert!(c.len() >= 14, "manifest row needs 14 columns: {l:?}");
            Row {
                file: c[0].to_string(),
                submission_type: c[1].to_string(),
                prefixed: c[2] != "-",
                persons: c[3].parse().unwrap(),
                issuer_cik: c[4].parse().unwrap(),
                largest_percent: dash(c[5]).map(|v| v.parse().unwrap()),
                previous_flag: dash(c[6]).map(|v| v == "true"),
                amendment_no: dash(c[7]).map(|v| v.parse().unwrap()),
                previous_accession: dash(c[8]).map(str::to_string),
                rules: dash(c[9])
                    .map(|r| r.split('+').map(str::to_string).collect())
                    .unwrap_or_default(),
                group_persons: c[10].parse().unwrap(),
            }
        })
        .collect()
}

#[test]
fn manifest_lists_every_real_fixture() {
    let on_disk: BTreeSet<String> = std::fs::read_dir(dir())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".xml"))
        .collect();
    let listed: BTreeSet<String> = manifest().into_iter().map(|r| r.file).collect();
    assert_eq!(on_disk, listed, "fixtures and MANIFEST.tsv disagree");
    assert!(on_disk.len() >= 12);
}

#[test]
fn every_real_fixture_parses_and_matches_the_manifest() {
    for row in manifest() {
        let xml = std::fs::read_to_string(dir().join(&row.file)).unwrap();
        let s = parse(&xml).unwrap_or_else(|e| panic!("{}: {e}", row.file));
        let name = &row.file;

        assert_eq!(s.submission_type, row.submission_type, "{name}: type");
        assert_eq!(
            s.schedule,
            if row.submission_type.contains("13D") {
                Schedule::D
            } else {
                Schedule::G
            },
            "{name}: schedule"
        );
        assert_eq!(
            s.is_amendment(),
            row.submission_type.ends_with("/A"),
            "{name}: amendment flag"
        );
        assert_eq!(s.persons.len(), row.persons, "{name}: persons");
        assert_eq!(s.issuer_cik.0, row.issuer_cik, "{name}: issuer");
        assert_eq!(
            s.largest_percent(),
            row.largest_percent,
            "{name}: largest percent"
        );
        assert_eq!(
            s.previously_filed_13g, row.previous_flag,
            "{name}: previouslyFiledFlag"
        );
        assert_eq!(s.amendment_no, row.amendment_no, "{name}: amendment number");
        assert_eq!(
            s.previous_accession, row.previous_accession,
            "{name}: previous accession"
        );
        assert_eq!(s.rules, row.rules, "{name}: rules");
        let group = s
            .persons
            .iter()
            .filter(|p| p.member_of_group.as_deref() == Some("a"))
            .count();
        assert_eq!(group, row.group_persons, "{name}: group persons");
        assert!(
            !s.issuer_name.is_empty() && !s.cusips.is_empty(),
            "{name}: issuer name / CUSIP"
        );
        assert!(
            s.persons.iter().all(|p| !p.name.is_empty()),
            "{name}: person names"
        );
        // Item 4 (purpose): always on an original 13D, never on a 13G, and optional on a 13D/A (an
        // amendment may amend only some items; Autoliv's has none).
        match (s.schedule, s.is_amendment()) {
            (Schedule::D, false) => assert!(s.purpose_of_transaction.is_some(), "{name}: Item 4"),
            (Schedule::G, _) => assert!(s.purpose_of_transaction.is_none(), "{name}: Item 4"),
            (Schedule::D, true) => {}
        }
        // The namespace style is part of what these fixtures cover.
        let root_prefixed = xml.contains(":edgarSubmission");
        assert_eq!(root_prefixed, row.prefixed, "{name}: prefix");
    }
}

#[test]
fn every_sidecar_has_a_known_on_and_an_accession() {
    for row in manifest() {
        let text =
            std::fs::read_to_string(dir().join(row.file.replace(".xml", ".meta.txt"))).unwrap();
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
