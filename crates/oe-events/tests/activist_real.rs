//! Runs the two activist rules over the real Schedule 13D/13G fixtures and
//! compares with the `new_13d_event` and `passive_to_active_event` columns of
//! `fixtures/sched13/real/MANIFEST.tsv`, which were worked out separately from
//! the raw XML (exact form type, maximum stated percentage, the cover-page flag).

use oe_core::FilingMeta;
use oe_events::{detect_activist_events, ActivistParams};
use std::path::PathBuf;

fn dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/sched13/real")
}

#[test]
fn the_rules_agree_with_an_independent_count_on_real_filings() {
    let manifest = std::fs::read_to_string(dir().join("MANIFEST.tsv")).unwrap();
    let (mut new_fired, mut pta_fired, mut total) = (0, 0, 0);
    for line in manifest.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let c: Vec<&str> = line.split('\t').collect();
        let (file, want_new, want_pta) = (c[0], c[11] == "yes", c[12] == "yes");
        let s = oe_sched13::parse(&std::fs::read_to_string(dir().join(file)).unwrap())
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        let events =
            detect_activist_events(&[(s, FilingMeta::default())], &ActivistParams::default());
        let has = |id: &str| events.iter().any(|e| e.rule_id == id);
        assert_eq!(has("activist.new_13d@1"), want_new, "{file}: new_13d");
        assert_eq!(
            has("activist.passive_to_active@1"),
            want_pta,
            "{file}: passive_to_active"
        );
        new_fired += usize::from(want_new);
        pta_fired += usize::from(want_pta);
        total += 1;
    }
    // Both sides of both rules are covered by the set.
    assert!(
        new_fired > 0 && new_fired < total,
        "new_13d fired {new_fired} of {total}"
    );
    assert!(pta_fired == 1, "passive_to_active fired {pta_fired}");
}

#[test]
fn spot_checks_against_what_the_filings_say() {
    let run = |file: &str| {
        let s = oe_sched13::parse(&std::fs::read_to_string(dir().join(file)).unwrap()).unwrap();
        detect_activist_events(&[(s, FilingMeta::default())], &ActivistParams::default())
    };
    let find = |prefix: &str| {
        std::fs::read_dir(dir())
            .unwrap()
            .filter_map(|e| e.ok())
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .find(|n| n.starts_with(prefix) && n.ends_with(".xml"))
            .unwrap()
    };

    // Perfect Moment: a 13D that says it followed a 13G fires both rules.
    let pm = run(&find("perfectmoment_"));
    let ids: Vec<&str> = pm.iter().map(|e| e.rule_id).collect();
    assert_eq!(ids, ["activist.new_13d@1", "activist.passive_to_active@1"]);
    assert_eq!(pm[1].basis, Some("filer's own previouslyFiledFlag"));
    assert!(pm[0].purpose.as_deref().unwrap().contains("13G"));

    // FutureFuel: four trusts, all in a group; the percentage is not summed.
    let ff = run(&find("futurefuel_"));
    assert_eq!(ff.len(), 1);
    assert!(ff[0].is_group);
    assert_eq!(ff[0].holders.len(), 4);
    assert_eq!(ff[0].largest_percent, Some(9.7));

    // Apollo: 0.51% is under the threshold; SPAR is an amendment even though it ticks the flag.
    assert!(run(&find("apollo_")).is_empty());
    assert!(run(&find("spar_")).is_empty());
    // A 13G, however large, is not a 13D.
    assert!(run(&find("airproducts_")).is_empty());

    // Lowering the threshold below the filing's own percentage makes Apollo fire.
    let s =
        oe_sched13::parse(&std::fs::read_to_string(dir().join(find("apollo_"))).unwrap()).unwrap();
    let low = ActivistParams { min_percent: 0.5 };
    assert_eq!(
        detect_activist_events(&[(s, FilingMeta::default())], &low).len(),
        1
    );
}
