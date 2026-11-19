//! Runs `insider.planned_sale@1` over the real Form 144 fixtures and compares
//! with the `event` column of `fixtures/form144/real/MANIFEST.tsv`, which was
//! worked out separately from the raw XML (type, relationship words, summed
//! `aggregateMarketValue`), not by this code.

use oe_core::FilingMeta;
use oe_events::{planned_sale_event, PlannedSaleParams};
use std::path::PathBuf;

#[test]
fn the_rule_agrees_with_an_independent_count_on_real_filings() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/form144/real");
    let manifest = std::fs::read_to_string(dir.join("MANIFEST.tsv")).unwrap();
    let (mut fired, mut total) = (0, 0);
    for line in manifest.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let c: Vec<&str> = line.split('\t').collect();
        let (file, expected) = (c[0], c[10] == "yes");
        let form = oe_form144::parse(&std::fs::read_to_string(dir.join(file)).unwrap())
            .unwrap_or_else(|e| panic!("{file}: {e}"));
        let event =
            planned_sale_event(&form, &FilingMeta::default(), &PlannedSaleParams::default());
        assert_eq!(event.is_some(), expected, "{file}");
        fired += usize::from(event.is_some());
        total += 1;
    }
    // Both sides of the rule are covered by the set.
    assert!(fired > 0 && fired < total, "fired {fired} of {total}");
}
