//! Runs `insider.plan_terminated@1` over the real report excerpts and compares
//! the number of events, rejected termination dates and replaced plans with
//! `fixtures/item408/real/MANIFEST.tsv` (columns computed independently from
//! the original full reports).

use oe_core::{Date, FilingMeta};
use oe_events::detect_plan_terminations;
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/item408/real")
}

fn meta_for(file: &str) -> FilingMeta {
    let text = std::fs::read_to_string(dir().join(file.replace(".htm", ".meta.txt"))).unwrap();
    let field = |key: &str| {
        text.lines()
            .find_map(|l| l.strip_prefix(&format!("{key}=")))
            .map(str::to_string)
    };
    FilingMeta {
        accession: field("accession").and_then(|a| a.parse().ok()),
        known_on: field("known_on").and_then(|d| d.parse().ok()),
        source: None,
    }
}

#[test]
fn events_rejected_dates_and_replacements_match_the_manifest() {
    let manifest = std::fs::read_to_string(dir().join("MANIFEST.tsv")).unwrap();
    let (mut events_total, mut rejected_total) = (0, 0);
    for line in manifest.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let c: Vec<&str> = line.split('\t').collect();
        let (file, terminated, rejected, replaced): (&str, usize, usize, usize) = (
            c[0],
            c[7].parse().unwrap(),
            c[8].parse().unwrap(),
            c[9].parse().unwrap(),
        );
        let doc = oe_item408::extract(&std::fs::read_to_string(dir().join(file)).unwrap());
        let events = detect_plan_terminations(&doc, &meta_for(file));

        assert_eq!(events.len(), terminated, "{file}: events");
        assert_eq!(
            events
                .iter()
                .filter(|e| e.rejected_termination_date.is_some())
                .count(),
            rejected,
            "{file}: rejected dates"
        );
        assert_eq!(
            events.iter().filter(|e| e.replaced_in_filing).count(),
            replaced,
            "{file}: replaced"
        );
        // Every event carries the issuer, the report's acceptance date and an accession.
        for e in &events {
            assert!(
                e.issuer.is_some() && e.known_on.is_some() && e.accession.is_some(),
                "{file}"
            );
            // A kept termination date is never after the period end.
            if let (Some(t), Some(p)) = (e.termination_date, e.period_end) {
                assert!(t <= p, "{file}: {t} after {p}");
            }
        }
        events_total += events.len();
        rejected_total += rejected;
    }
    assert!(events_total >= 10 && rejected_total >= 1);
}

#[test]
fn spot_checks_against_what_the_reports_say() {
    let run = |file: &str| {
        let doc = oe_item408::extract(&std::fs::read_to_string(dir().join(file)).unwrap());
        detect_plan_terminations(&doc, &meta_for(file))
    };

    // Palantir: Sankar and Glazer each ended a plan and adopted a new one.
    let p = run("palantir_0001321655-26-000048.htm");
    let sankar = p
        .iter()
        .find(|e| e.person.as_deref() == Some("Shyam Sankar"))
        .unwrap();
    assert_eq!(sankar.termination_date, Date::from_ymd(2026, 8, 6));
    assert!(sankar.replaced_in_filing);
    assert_eq!(sankar.known_on, Date::from_ymd(2026, 11, 2)); // accepted 18:00 Eastern; EDGAR says the 3rd
    assert_eq!(
        sankar
            .title
            .as_deref()
            .map(|t| t.starts_with("Chief Technology Officer")),
        Some(true)
    );

    // Cryoport: the tagged date is the plan's scheduled end; the event does not report it as the termination.
    let c = run("cryoport_0001104659-26-123439.htm");
    assert_eq!(c.len(), 1);
    assert_eq!(c[0].termination_date, None);
    assert_eq!(c[0].rejected_termination_date, Date::from_ymd(2027, 3, 12));

    // ARS: four people, six plans.
    let a = run("ars_0001193125-26-449778.htm");
    assert_eq!(a.len(), 6);
    let people: std::collections::BTreeSet<_> = a.iter().filter_map(|e| e.person.clone()).collect();
    assert_eq!(people.len(), 4);

    // No event where the company says nothing was terminated, or tags a company-level flag.
    for none in [
        "meta_0001628280-26-066773.htm",
        "twinvee_0001731122-26-001404.htm",
        "apple_0000320193-26-000026.htm",
        "coinbase_0001679788-26-000122.htm",
    ] {
        assert!(run(none).is_empty(), "{none}");
    }
}
