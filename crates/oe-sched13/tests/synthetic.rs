use oe_core::{Cik, Date};
use oe_sched13::{parse, ParseError, Schedule};
use std::path::PathBuf;

fn fixture(name: &str) -> String {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/sched13/synthetic")
        .join(name);
    std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()))
}

fn d(y: i32, m: u32, day: u32) -> Date {
    Date::from_ymd(y, m, day).unwrap()
}

#[test]
fn a_new_13d_from_a_joint_filing() {
    let s = parse(&fixture("01_new_13d_group.xml")).unwrap();
    assert_eq!(s.submission_type, "SCHEDULE 13D");
    assert_eq!(s.schedule, Schedule::D);
    assert!(!s.is_amendment());
    assert_eq!(s.amendment_no, None);
    assert_eq!(s.previous_accession, None);
    assert_eq!(s.issuer_cik, Cik(9_999_901));
    assert_eq!(s.issuer_name, "SYNTHETIC WIDGETS INC");
    assert_eq!(s.cusips, ["000000AA1"]);
    assert_eq!(s.class_title, "Common Stock, $0.01 par value");
    assert_eq!(s.event_date, Some(d(2026, 3, 4)));
    assert_eq!(s.previously_filed_13g, Some(false));
    assert_eq!(s.filer_ciks, [Cik(9_999_921)]);
    assert_eq!(s.signature_date, Some(d(2026, 3, 11)));

    assert_eq!(s.persons.len(), 2);
    let fund = &s.persons[0];
    assert_eq!(fund.name, "Example Opportunity Fund LP");
    assert_eq!(fund.cik, Some(Cik(9_999_921)));
    assert_eq!(fund.member_of_group.as_deref(), Some("a"));
    assert_eq!(fund.fund_type.as_deref(), Some("WC"));
    assert_eq!(
        fund.shared_voting,
        Some(1_250_000.0),
        "thousands separators are read"
    );
    assert_eq!(fund.sole_voting, Some(0.0));
    assert_eq!(fund.aggregate_shares, Some(1_250_000.0));
    assert_eq!(fund.percent_of_class, Some(8.4));
    assert_eq!(fund.person_type.as_deref(), Some("PN"));
    // Joint filers state the same shares: the largest is used, not the sum.
    assert_eq!(s.largest_percent(), Some(8.4));
    assert_eq!(s.largest_aggregate_shares(), Some(1_250_000.0));

    let purpose = s.purpose_of_transaction.unwrap();
    assert!(purpose.contains("undervalued") && purpose.contains("board composition"));
}

#[test]
fn a_13d_that_says_it_followed_a_13g() {
    let s = parse(&fixture("02_13d_after_13g.xml")).unwrap();
    assert_eq!(s.previously_filed_13g, Some(true));
    assert_eq!(s.persons.len(), 1);
    assert_eq!(s.persons[0].member_of_group.as_deref(), Some("b"));
    assert_eq!(s.largest_percent(), Some(9.9));
    assert!(s.purpose_of_transaction.unwrap().contains("13G"));
}

#[test]
fn an_amendment_names_what_it_amends() {
    let s = parse(&fixture("03_13d_amendment.xml")).unwrap();
    assert!(s.is_amendment());
    assert_eq!(s.submission_type, "SCHEDULE 13D/A");
    assert_eq!(s.amendment_no, Some(3));
    assert_eq!(
        s.previous_accession.as_deref(),
        Some("0009999921-26-000001")
    );
}

#[test]
fn a_13g_uses_other_element_names() {
    let s = parse(&fixture("04_13g_passive.xml")).unwrap();
    assert_eq!(s.schedule, Schedule::G);
    assert_eq!(s.submission_type, "SCHEDULE 13G");
    assert_eq!(s.issuer_cik, Cik(9_999_902));
    assert_eq!(s.issuer_name, "Gadget Holdings Corp.");
    assert_eq!(s.event_date, Some(d(2026, 2, 27)));
    assert_eq!(s.rules, ["Rule 13d-1(c)"]);
    assert_eq!(s.previously_filed_13g, None);
    assert_eq!(s.purpose_of_transaction, None);
    assert_eq!(s.filer_ciks, [Cik(9_999_931)]);
    assert_eq!(s.persons.len(), 2);
    let adviser = &s.persons[0];
    assert_eq!(adviser.name, "Example Index Advisers LLC");
    assert_eq!(adviser.cik, None, "a 13G does not list the person's CIK");
    assert_eq!(adviser.shared_voting, Some(612_000.0));
    assert_eq!(adviser.shared_dispositive, Some(612_000.0));
    assert_eq!(adviser.aggregate_shares, Some(612_000.0));
    assert_eq!(adviser.percent_of_class, Some(5.4));
    assert_eq!(adviser.person_type.as_deref(), Some("IA"));
    assert_eq!(s.largest_percent(), Some(5.4));
    assert_eq!(s.signature_date, Some(d(2026, 3, 2)));
}

#[test]
fn a_single_issuer_cusip_tag_is_read_too() {
    // Real filing (Sharps Technology): <issuerCUSIP> instead of issuerCusips/issuerCusipNumber.
    let xml = fixture("01_new_13d_group.xml").replace(
        "<issuerCusips>\n          <issuerCusipNumber>000000AA1</issuerCusipNumber>\n        </issuerCusips>",
        "<issuerCUSIP>000000AA1</issuerCUSIP>",
    );
    assert!(xml.contains("<issuerCUSIP>"), "the replacement must apply");
    assert_eq!(parse(&xml).unwrap().cusips, ["000000AA1"]);
}

#[test]
fn rejects_other_documents_and_bad_values() {
    assert_eq!(
        parse("<ownershipDocument/>"),
        Err(ParseError::NotSchedule13)
    );
    assert!(matches!(parse("<html>"), Err(ParseError::Xml(_))));
    let no_type = "<edgarSubmission><formData/></edgarSubmission>";
    assert_eq!(parse(no_type), Err(ParseError::Missing("submissionType")));
    let form4 = "<edgarSubmission><headerData><submissionType>144</submissionType></headerData></edgarSubmission>";
    assert_eq!(parse(form4), Err(ParseError::NotSchedule13));

    let no_issuer = "<edgarSubmission><headerData><submissionType>SCHEDULE 13D</submissionType></headerData></edgarSubmission>";
    assert_eq!(parse(no_issuer), Err(ParseError::Missing("issuer CIK")));

    let bad_pct = fixture("01_new_13d_group.xml").replace(">8.4<", ">lots<");
    assert!(matches!(
        parse(&bad_pct),
        Err(ParseError::Invalid {
            field: "percent of class",
            ..
        })
    ));
    let bad_date = fixture("01_new_13d_group.xml").replace("03/04/2026", "13/45/2026");
    assert!(matches!(
        parse(&bad_date),
        Err(ParseError::Invalid {
            field: "event date",
            ..
        })
    ));
}

#[test]
fn missing_numbers_stay_empty_and_the_filing_still_parses() {
    let xml = fixture("01_new_13d_group.xml").replace(
        "<percentOfClass>8.4</percentOfClass>",
        "<percentOfClass></percentOfClass>",
    );
    let s = parse(&xml).unwrap();
    assert_eq!(s.largest_percent(), None);
    let na = fixture("04_13g_passive.xml").replace(">5.4<", ">N/A<");
    assert_eq!(parse(&na).unwrap().largest_percent(), None);
}
