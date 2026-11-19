use oe_core::{Cik, Date};
use oe_form144::{parse, ParseError};
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/form144/synthetic")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn d(y: i32, m: u32, day: u32) -> Date {
    Date::from_ymd(y, m, day).unwrap()
}

#[test]
fn officer_selling_under_a_plan() {
    let f = parse(&fixture("01_officer_plan_sale.xml")).unwrap();
    assert_eq!(f.submission_type, "144");
    assert!(!f.is_amendment());
    assert_eq!(f.issuer_cik, Cik(9_999_901));
    assert_eq!(f.issuer_name, "SYNTHETIC WIDGETS INC");
    assert_eq!(f.seller_name, "OFFICER BRAVO");
    assert_eq!(f.relationships, ["Officer", "Director", "10% Stockholder"]);
    assert!(f.is_insider());

    assert_eq!(f.securities.len(), 1);
    let s = &f.securities[0];
    assert_eq!(s.class_title, "Common");
    assert_eq!(s.units, Some(65_000.0));
    assert_eq!(s.aggregate_market_value, Some(2_824_614.0));
    assert_eq!(s.units_outstanding, Some(65_019_350.0));
    assert_eq!(s.approx_sale_date, Some(d(2026, 3, 11)));
    assert_eq!(s.exchange.as_deref(), Some("NASDAQ"));
    assert_eq!(s.broker.as_deref(), Some("Example Securities LLC"));

    assert_eq!(f.acquisitions.len(), 1);
    assert_eq!(f.acquisitions[0].nature, "Stock Option Exercise");
    assert_eq!(f.acquisitions[0].from_whom.as_deref(), Some("ISSUER"));
    assert_eq!(f.acquisitions[0].is_gift, Some(false));
    assert_eq!(f.acquisitions[0].amount, Some(65_000.0));

    assert_eq!(f.sold_past_3_months.len(), 3);
    assert_eq!(f.sold_past_3_months[0].seller, "OFFICER BRAVO");
    assert_eq!(f.sold_past_3_months[0].date, Some(d(2026, 3, 10)));
    assert_eq!(f.sold_past_3_months[1].gross_proceeds, Some(5_221_482.0));
    assert_eq!(f.units_sold_past_3_months(), 205_000.0);
    assert_eq!(f.nothing_sold_past_3_months, Some(false));

    assert!(f.under_plan());
    assert_eq!(f.plan_adoption_dates, [d(2025, 6, 5)]);
    assert_eq!(f.notice_date, Some(d(2026, 3, 11)));
    assert_eq!(f.remarks, None);
    assert_eq!(f.total_units(), Some(65_000.0));
    assert_eq!(f.total_market_value(), Some(2_824_614.0));
}

#[test]
fn prefixed_namespace_amendment_with_two_classes() {
    let f = parse(&fixture("02_prefixed_amendment_two_classes.xml")).unwrap();
    assert_eq!(f.submission_type, "144/A");
    assert!(f.is_amendment());
    assert_eq!(f.issuer_cik, Cik(9_999_902));
    // Lower-case "director" still counts; the free-text relationship is kept.
    assert_eq!(f.relationships, ["director", "May be deemed affiliate"]);
    assert!(f.is_insider());

    // Thousands separators are read; two classes are summed.
    assert_eq!(f.securities.len(), 2);
    assert_eq!(f.securities[0].units, Some(1_250_000.0));
    assert_eq!(f.securities[1].class_title, "Class B Common");
    assert_eq!(f.total_units(), Some(1_260_000.0));
    assert_eq!(f.total_market_value(), Some(12_590_000.0));

    assert_eq!(f.acquisitions.len(), 2);
    assert_eq!(f.acquisitions[0].is_gift, Some(true));
    assert_eq!(f.acquisitions[1].nature, "Restricted Stock Vesting");

    assert_eq!(f.nothing_sold_past_3_months, Some(true));
    assert!(f.sold_past_3_months.is_empty());
    assert!(!f.under_plan());
    assert_eq!(
        f.remarks.as_deref(),
        Some("Amended to correct the number of units.")
    );
}

#[test]
fn a_seller_with_only_a_free_text_relationship_is_not_an_insider() {
    let xml = fixture("02_prefixed_amendment_two_classes.xml").replace(
        "<own:relationshipToIssuer>director</own:relationshipToIssuer>",
        "",
    );
    let f = parse(&xml).unwrap();
    assert_eq!(f.relationships, ["May be deemed affiliate"]);
    assert!(!f.is_insider());
}

#[test]
fn the_insider_test_reads_free_text_relationships() {
    let mut f = parse(&fixture("01_officer_plan_sale.xml")).unwrap();
    let cases = [
        ("Chief Financial Officer", true),
        ("President & CEO", true),
        ("Director/Non-Executive Director", true),
        ("10% Stockholder", true),
        ("  OFFICER ", true),
        ("CFO", true),
        ("Chairman", true),
        ("Former CEO", false),
        ("Former Director", false),
        ("Former Officer", false),
        ("Affiliate", false),
        ("Shareholder", false),
        ("See Remarks", false),
        ("May be deemed affiliate", false),
        ("Member of Section 13D group", false),
        ("Self", false),
        // "ceo" inside another word is not the title.
        ("Piceous Holdings", false),
    ];
    for (text, expected) in cases {
        f.relationships = vec![text.to_string()];
        assert_eq!(f.is_insider(), expected, "{text:?}");
    }
    // One qualifying relationship among several is enough.
    f.relationships = vec!["Affiliate".into(), "Officer".into()];
    assert!(f.is_insider());
}

#[test]
fn rejects_other_documents_and_bad_values() {
    assert_eq!(parse("<ownershipDocument/>"), Err(ParseError::NotForm144));
    assert!(matches!(parse("<html>"), Err(ParseError::Xml(_))));

    let no_issuer = "<edgarSubmission><headerData><submissionType>144</submissionType></headerData></edgarSubmission>";
    assert_eq!(parse(no_issuer), Err(ParseError::Missing("issuerCik")));

    let bad_date = fixture("01_officer_plan_sale.xml")
        .replace("03/11/2026</approxSaleDate>", "13/45/2026</approxSaleDate>");
    assert!(matches!(
        parse(&bad_date),
        Err(ParseError::Invalid {
            field: "approxSaleDate",
            ..
        })
    ));
    let bad_number = fixture("01_officer_plan_sale.xml")
        .replace(">65000</noOfUnitsSold>", ">lots</noOfUnitsSold>");
    assert!(matches!(
        parse(&bad_number),
        Err(ParseError::Invalid {
            field: "noOfUnitsSold",
            ..
        })
    ));
}

#[test]
fn a_notice_without_values_still_parses() {
    let xml = fixture("01_officer_plan_sale.xml")
        .replace("<noOfUnitsSold>65000</noOfUnitsSold>", "")
        .replace(
            "<aggregateMarketValue>2824614.00</aggregateMarketValue>",
            "",
        );
    let f = parse(&xml).unwrap();
    assert_eq!(f.total_units(), None);
    assert_eq!(f.total_market_value(), None);
}
