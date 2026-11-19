//! Runs `activist.proxy_contest@1` over the real headers in
//! `fixtures/proxy/real/` and compares each contest with `EVENTS.tsv`, which was
//! worked out separately from the raw headers.

use oe_events::{detect_proxy_contests, ContestFiling};
use std::collections::BTreeSet;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/proxy/real")
}

fn filings() -> Vec<ContestFiling> {
    let mut paths: Vec<PathBuf> = std::fs::read_dir(dir())
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.to_string_lossy().ends_with(".hdr.sgml"))
        .collect();
    paths.sort();
    paths
        .iter()
        .map(|p| {
            let h = oe_edgar::parse_header_sgml(&std::fs::read_to_string(p).unwrap()).unwrap();
            ContestFiling {
                known_on: h.known_on(),
                subject: h.subject.clone().unwrap(),
                form: h.form,
                accession: h.accession,
                filed_by: h.filed_by,
            }
        })
        .collect()
}

#[test]
fn each_contest_matches_an_independent_count() {
    let events = detect_proxy_contests(&filings());
    let table = std::fs::read_to_string(dir().join("EVENTS.tsv")).unwrap();
    let rows: Vec<Vec<&str>> = table
        .lines()
        .skip(1)
        .filter(|l| !l.trim().is_empty())
        .map(|l| l.split('\t').collect())
        .collect();
    assert_eq!(events.len(), rows.len(), "one event per subject company");

    for c in rows {
        let e = events
            .iter()
            .find(|e| e.issuer.0.to_string() == c[0])
            .unwrap_or_else(|| panic!("no event for {}", c[1]));
        assert_eq!(e.issuer_name, c[1]);
        assert_eq!(e.filings.len().to_string(), c[2], "{}: filings", c[1]);
        let got: BTreeSet<String> = e.challengers.iter().map(|p| p.cik.0.to_string()).collect();
        let want: BTreeSet<String> = c[3]
            .split('+')
            .filter(|s| !s.is_empty())
            .map(str::to_string)
            .collect();
        assert_eq!(got, want, "{}: challengers", c[1]);
        assert_eq!(
            e.challenger_proxy_statement,
            c[4] == "yes",
            "{}: challenger proxy",
            c[1]
        );
        assert_eq!(
            e.company_contested_proxy,
            c[5] == "yes",
            "{}: company proxy",
            c[1]
        );
        assert!(e.first_known_on <= e.last_known_on && e.first_known_on.is_some());
    }
}

#[test]
fn spot_checks_against_the_real_contests() {
    let events = detect_proxy_contests(&filings());
    let by_name = |n: &str| events.iter().find(|e| e.issuer_name == n).unwrap();

    // Two Harbors: two rival bidders; the DEFC14A was filed by one of them, not the company.
    let th = by_name("TWO HARBORS INVESTMENT CORP.");
    assert_eq!(th.challengers.len(), 2);
    assert!(th.challenger_proxy_statement && !th.company_contested_proxy);

    // Sturm Ruger: Beretta's letter and the company's own contested proxy.
    let sr = by_name("STURM RUGER & CO INC");
    assert_eq!(sr.challengers[0].name, "Beretta Holding S.A.");
    assert!(sr.company_contested_proxy && !sr.challenger_proxy_statement);

    // Pacira: only the company's DEFC14A in this set, so no challenger is named.
    let pa = by_name("Pacira BioSciences, Inc.");
    assert!(pa.challengers.is_empty() && pa.company_contested_proxy);
}
