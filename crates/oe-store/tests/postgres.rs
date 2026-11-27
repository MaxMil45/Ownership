//! Tests against a real PostgreSQL server. They are `#[ignore]`d so the normal
//! test run needs no database. To run them, point `OE_TEST_DATABASE_URL` at a
//! scratch database (each test makes its own schema and drops it afterwards, so
//! nothing else in the database is touched):
//!
//! ```powershell
//! $env:OE_TEST_DATABASE_URL = "postgres://user:password@localhost:5432/ownership_test"
//! cargo test -p oe-store --test postgres -- --ignored --test-threads=1
//! ```

use oe_store::{load_batch, migrate, prepare, status, Client, LinkReport, Prepared, Sidecar};
use std::path::PathBuf;

/// A connection whose search_path is a private schema, dropped on `finish`.
struct Scratch {
    client: Client,
    schema: String,
}

impl Scratch {
    fn new(tag: &str) -> Scratch {
        let url = std::env::var("OE_TEST_DATABASE_URL")
            .expect("set OE_TEST_DATABASE_URL to run the PostgreSQL tests");
        let mut client = oe_store::connect(&url).expect("connect");
        let schema = format!("oe_test_{}_{tag}", std::process::id());
        client
            .batch_execute(&format!(
                "DROP SCHEMA IF EXISTS {schema} CASCADE; CREATE SCHEMA {schema}; SET search_path TO {schema}"
            ))
            .expect("make scratch schema");
        Scratch { client, schema }
    }

    fn finish(mut self) {
        self.client
            .batch_execute(&format!("DROP SCHEMA {} CASCADE", self.schema))
            .expect("drop scratch schema");
    }
}

fn real_fixtures() -> Vec<Prepared> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/form4/real");
    let mut names: Vec<PathBuf> = std::fs::read_dir(&dir)
        .unwrap()
        .map(|e| e.unwrap().path())
        .filter(|p| p.extension().is_some_and(|x| x == "xml"))
        .collect();
    names.sort();
    names
        .iter()
        .map(|p| {
            let xml = std::fs::read_to_string(p).unwrap();
            let meta = std::fs::read_to_string(p.with_extension("meta.txt")).unwrap();
            prepare(&xml, &Sidecar::parse(&meta)).unwrap()
        })
        .collect()
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn migrate_is_idempotent() {
    let mut s = Scratch::new("migrate");
    assert_eq!(
        migrate(&mut s.client).unwrap(),
        [
            "0001_init",
            "0002_amendments",
            "0003_bulk",
            "0004_link_index",
            "0005_other_forms",
            "0006_officials",
            "0007_people",
            "0008_event_feed",
            "0009_lookup_indexes",
            "0010_prices",
            "0011_house_transactions"
        ]
    );
    assert!(
        migrate(&mut s.client).unwrap().is_empty(),
        "second run applies nothing"
    );
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn loads_the_real_fixtures_and_a_repeat_changes_nothing() {
    let mut s = Scratch::new("load");
    migrate(&mut s.client).unwrap();
    let batch = real_fixtures();
    let expected_rows: u64 = batch.iter().map(|p| p.ledger_rows().len() as u64).sum();
    assert_eq!(expected_rows, 68);

    let first = load_batch(&mut s.client, &batch).unwrap();
    assert_eq!((first.filings_new, first.filings_existing), (25, 0));
    assert_eq!((first.rows_new, first.rows_existing), (expected_rows, 0));
    assert_eq!(first.amendment_rows, 17);
    assert!(first.changed_source.is_empty());
    // Of the five amendments only the NVIDIA Form 5/A has its original among
    // the fixtures: it restates both rows. The other four wait.
    assert_eq!(
        first.links,
        LinkReport {
            loaded: 1,
            rows_loaded: 2,
            held_back: 0,
            ambiguous: 0,
            waiting: 4
        }
    );

    let again = load_batch(&mut s.client, &batch).unwrap();
    assert_eq!((again.filings_new, again.filings_existing), (0, 25));
    assert_eq!((again.rows_new, again.rows_existing), (0, expected_rows));
    assert_eq!(
        again.links,
        LinkReport {
            waiting: 4,
            ..LinkReport::default()
        }
    );

    let st = status(&mut s.client).unwrap();
    assert_eq!(st.filings, 25);
    assert_eq!(st.amendment_filings, 5);
    assert_eq!(st.joint_filings, 2);
    assert_eq!(
        (
            st.amendments_loaded,
            st.amendments_held_back,
            st.amendments_ambiguous,
            st.amendments_waiting
        ),
        (1, 0, 0, 4)
    );
    // Two amendment rows joined the ledger and hide the two rows they replace.
    assert_eq!(st.transactions as u64, expected_rows + 2);
    assert_eq!(st.transactions_known_now as u64, expected_rows);
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn stored_values_match_the_filing() {
    let mut s = Scratch::new("values");
    migrate(&mut s.client).unwrap();
    load_batch(&mut s.client, &real_fixtures()).unwrap();

    // Tesla 2026-04-21: disposition to the issuer, 96,000,000 shares at price 0,
    // from a filing accepted at 2026-04-23T... (UTC) per its sidecar.
    let row = s
        .client
        .query_one(
            "SELECT t.code, t.shares::text, t.price_per_share::text, t.issuer_cik,
                    t.known_at = f.accepted_at, f.form_type, length(f.raw_sha256)
             FROM insider_transaction t JOIN filing f USING (accession)
             WHERE t.accession = '0001104659-26-047678'",
            &[],
        )
        .unwrap();
    assert_eq!(row.get::<_, String>(0), "D");
    assert_eq!(row.get::<_, String>(1), "96000000");
    assert_eq!(row.get::<_, String>(2), "0");
    assert_eq!(row.get::<_, i64>(3), 1_318_605);
    assert!(
        row.get::<_, bool>(4),
        "known_at is the filing's acceptance time"
    );
    assert_eq!(row.get::<_, String>(5), "4");
    assert_eq!(row.get::<_, i32>(6), 64);

    // Joint filing: filer is the first owner.
    let filer: i64 = s
        .client
        .query_one(
            "SELECT filer_cik FROM filing WHERE accession = '0001193125-26-445930'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(filer, 1_067_983);
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn eastern_acceptance_times_are_converted_by_the_server_with_daylight_saving() {
    let mut s = Scratch::new("tz");
    migrate(&mut s.client).unwrap();
    let base = &real_fixtures()[0];
    let xml_path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/form4/real/aapl_0001140361-26-038674.xml");
    let xml = std::fs::read_to_string(xml_path).unwrap();
    assert_eq!(base.filing.accession.len(), 20);

    // Summer (EDT, UTC-4) and winter (EST, UTC-5), same wall-clock time.
    for (accession, eastern, expected_utc) in [
        (
            "0000000001-26-000001",
            "2026-10-05T18:42:45",
            "2026-10-05 22:42:45",
        ),
        (
            "0000000001-26-000002",
            "2026-01-15T18:42:45",
            "2026-01-15 23:42:45",
        ),
    ] {
        let side = Sidecar {
            accession: Some(accession.into()),
            accepted_eastern: Some(eastern.into()),
            ..Sidecar::default()
        };
        let p = prepare(&xml, &side).unwrap();
        load_batch(&mut s.client, &[p]).unwrap();
        let got: String = s
            .client
            .query_one(
                "SELECT to_char(accepted_at AT TIME ZONE 'UTC', 'YYYY-MM-DD HH24:MI:SS') FROM filing WHERE accession = $1",
                &[&accession],
            )
            .unwrap()
            .get(0);
        assert_eq!(got, expected_utc, "{eastern}");
    }
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn the_ledger_is_append_only() {
    let mut s = Scratch::new("append");
    migrate(&mut s.client).unwrap();
    load_batch(&mut s.client, &real_fixtures()).unwrap();
    let update = s.client.batch_execute("UPDATE filing SET form_type = 'X'");
    assert!(update.is_err(), "UPDATE must be rejected by the trigger");
    let delete = s.client.batch_execute("DELETE FROM insider_transaction");
    assert!(delete.is_err(), "DELETE must be rejected by the trigger");
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn a_changed_source_is_reported_and_not_overwritten() {
    let mut s = Scratch::new("changed");
    migrate(&mut s.client).unwrap();
    let mut batch = real_fixtures();
    load_batch(&mut s.client, &batch).unwrap();

    let original = batch[0].filing.raw_sha256.clone();
    batch[0].filing.raw_sha256 = "0".repeat(64);
    let report = load_batch(&mut s.client, &batch).unwrap();
    assert_eq!(report.changed_source, [batch[0].filing.accession.clone()]);
    let stored: String = s
        .client
        .query_one(
            "SELECT raw_sha256 FROM filing WHERE accession = $1",
            &[&batch[0].filing.accession],
        )
        .unwrap()
        .get(0);
    assert_eq!(stored, original, "the stored hash is never rewritten");
    s.finish();
}

const FORM5: &str = "0001045810-25-000054";
const FORM5A: &str = "0001045810-25-000070";

fn fixture(name: &str) -> Prepared {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/form4/real")
        .join(name);
    let xml = std::fs::read_to_string(p.with_extension("xml")).unwrap();
    let meta = std::fs::read_to_string(p.with_extension("meta.txt")).unwrap();
    prepare(&xml, &Sidecar::parse(&meta)).unwrap()
}

/// A copy of a prepared filing under another accession number and time.
fn copy_as(p: &Prepared, accession: &str, accepted_at: &str) -> Prepared {
    let mut c = p.clone();
    c.filing.accession = accession.into();
    c.filing.accepted_at = accepted_at.into();
    c.filing.raw_sha256 = format!("{:0>64}", accession.replace('-', ""));
    for t in &mut c.transactions {
        t.accession = accession.into();
    }
    c
}

type Link = (String, Option<String>, Option<String>);

fn link(client: &mut Client, amendment: &str) -> Link {
    let r = client
        .query_one(
            "SELECT outcome, original_accession, replaces_accession
             FROM amendment_link WHERE amendment_accession = $1",
            &[&amendment],
        )
        .unwrap();
    (r.get(0), r.get(1), r.get(2))
}

/// The NVIDIA owner's rows visible at a moment, as `accession:row`.
fn visible(client: &mut Client, as_of: &str) -> Vec<String> {
    client
        .query(
            "SELECT accession || ':' || row_in_filing
             FROM insider_transaction_as_of($1::text::timestamptz)
             WHERE issuer_cik = 1045810 AND owner_cik = 1636665 ORDER BY 1",
            &[&as_of],
        )
        .unwrap()
        .iter()
        .map(|r| r.get(0))
        .collect()
}

fn rows_of(accession: &str) -> Vec<String> {
    vec![format!("{accession}:0"), format!("{accession}:1")]
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn a_linked_amendment_replaces_its_original_only_from_its_acceptance_on() {
    let mut s = Scratch::new("pit");
    migrate(&mut s.client).unwrap();
    load_batch(
        &mut s.client,
        &[
            fixture("nvda_0001045810-25-000054"),
            fixture("nvda_0001045810-25-000070"),
        ],
    )
    .unwrap();
    assert_eq!(
        link(&mut s.client, FORM5A),
        ("loaded".into(), Some(FORM5.into()), Some(FORM5.into()))
    );
    // Form 5 accepted 2025-03-12 22:58 UTC, the 5/A 2025-03-22 00:44 UTC.
    assert!(visible(&mut s.client, "2025-03-12 12:00:00+00").is_empty());
    assert_eq!(
        visible(&mut s.client, "2025-03-21 12:00:00+00"),
        rows_of(FORM5)
    );
    assert_eq!(
        visible(&mut s.client, "2026-01-01 00:00:00+00"),
        rows_of(FORM5A)
    );
    // Each amendment row points at the row in the same position.
    let pairs: Vec<(i32, i32)> = s
        .client
        .query(
            "SELECT n.row_in_filing, o.row_in_filing FROM insider_transaction n
             JOIN insider_transaction o ON o.id = n.supersedes_id
             WHERE n.accession = $1 AND o.accession = $2 ORDER BY 1",
            &[&FORM5A, &FORM5],
        )
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect();
    assert_eq!(pairs, [(0, 0), (1, 1)]);
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn an_amendment_loaded_before_its_original_waits_and_links_later() {
    let mut s = Scratch::new("wait");
    migrate(&mut s.client).unwrap();
    let first = load_batch(&mut s.client, &[fixture("nvda_0001045810-25-000070")]).unwrap();
    assert_eq!(first.links.waiting, 1);
    assert!(visible(&mut s.client, "2026-01-01 00:00:00+00").is_empty());
    let second = load_batch(&mut s.client, &[fixture("nvda_0001045810-25-000054")]).unwrap();
    assert_eq!((second.links.loaded, second.links.waiting), (1, 0));
    assert_eq!(
        visible(&mut s.client, "2026-01-01 00:00:00+00"),
        rows_of(FORM5A)
    );
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn a_second_amendment_replaces_the_first_and_a_late_older_one_is_held_back() {
    let mut s = Scratch::new("chain");
    migrate(&mut s.client).unwrap();
    let form5 = fixture("nvda_0001045810-25-000054");
    let a1 = fixture("nvda_0001045810-25-000070");
    let a2 = copy_as(&a1, "0001045810-25-000099", "2025-04-01 12:00:00+00");
    // Accepted between a1 and a2, but loaded only after a2.
    let late = copy_as(&a1, "0001045810-25-000080", "2025-03-25 12:00:00+00");
    load_batch(&mut s.client, &[form5, a1, a2.clone()]).unwrap();
    assert_eq!(
        link(&mut s.client, &a2.filing.accession),
        ("loaded".into(), Some(FORM5.into()), Some(FORM5A.into()))
    );
    assert_eq!(
        visible(&mut s.client, "2025-03-30 00:00:00+00"),
        rows_of(FORM5A)
    );
    assert_eq!(
        visible(&mut s.client, "2026-01-01 00:00:00+00"),
        rows_of("0001045810-25-000099")
    );

    load_batch(&mut s.client, std::slice::from_ref(&late)).unwrap();
    assert_eq!(
        link(&mut s.client, &late.filing.accession),
        ("held_back".into(), Some(FORM5.into()), None)
    );
    assert_eq!(
        visible(&mut s.client, "2026-01-01 00:00:00+00"),
        rows_of("0001045810-25-000099"),
        "still exactly one version"
    );
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn a_supplement_is_held_back_and_two_candidate_originals_are_ambiguous() {
    let form5 = fixture("nvda_0001045810-25-000054");

    let mut s = Scratch::new("held");
    migrate(&mut s.client).unwrap();
    let mut supplement = fixture("nvda_0001045810-25-000070");
    supplement.transactions.truncate(1); // reports only one of the two rows
    load_batch(&mut s.client, &[form5.clone(), supplement]).unwrap();
    assert_eq!(
        link(&mut s.client, FORM5A),
        ("held_back".into(), Some(FORM5.into()), None)
    );
    assert_eq!(
        visible(&mut s.client, "2026-01-01 00:00:00+00"),
        rows_of(FORM5),
        "the original stays visible"
    );
    s.finish();

    let mut s = Scratch::new("ambig");
    migrate(&mut s.client).unwrap();
    // A second Form 5 by the same owner for the same issuer on the same day.
    let twin = copy_as(&form5, "0001045810-25-000055", "2025-03-12 23:30:00+00");
    load_batch(
        &mut s.client,
        &[form5, twin, fixture("nvda_0001045810-25-000070")],
    )
    .unwrap();
    assert_eq!(
        link(&mut s.client, FORM5A),
        ("ambiguous".into(), None, None)
    );
    assert_eq!(visible(&mut s.client, "2026-01-01 00:00:00+00").len(), 4);
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn joint_filings_store_every_owner() {
    let mut s = Scratch::new("owners");
    migrate(&mut s.client).unwrap();
    load_batch(&mut s.client, &real_fixtures()).unwrap();
    let owners = |client: &mut Client, accession: &str| -> Vec<i64> {
        client
            .query(
                "SELECT owner_cik FROM filing_owner WHERE accession = $1 ORDER BY position",
                &[&accession],
            )
            .unwrap()
            .iter()
            .map(|r| r.get(0))
            .collect()
    };
    assert_eq!(
        owners(&mut s.client, "0001193125-26-445930"),
        [1_067_983, 315_090]
    );
    assert_eq!(owners(&mut s.client, "0001104659-26-047678").len(), 1);
    s.finish();
}

/// The NVIDIA Form 5 as the quarterly data set gives it (filing date only).
fn bulk_form5(accession: &str, filing_date: &str) -> Prepared {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/form4/real/nvda_0001045810-25-000054.xml");
    let doc = oe_form4::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    let f = oe_bulk::BulkFiling {
        accession: accession.into(),
        filing_date: filing_date.parse().unwrap(),
        document: doc,
    };
    oe_store::prepare_bulk(&f, "https://www.sec.gov/files/test/2025q1_form345.zip").unwrap()
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn bulk_filings_load_link_and_never_count_as_changed() {
    let mut s = Scratch::new("bulk");
    migrate(&mut s.client).unwrap();
    // A bulk original and the real XML amendment link across sources.
    let first = load_batch(
        &mut s.client,
        &[
            bulk_form5(FORM5, "2025-03-12"),
            fixture("nvda_0001045810-25-000070"),
        ],
    )
    .unwrap();
    assert_eq!((first.links.loaded, first.links.waiting), (1, 0));
    let (source, accepted): (String, String) = {
        let r = s
            .client
            .query_one(
                "SELECT source, to_char(accepted_at AT TIME ZONE 'America/New_York', 'YYYY-MM-DD HH24:MI:SS')
                 FROM filing WHERE accession = $1",
                &[&FORM5],
            )
            .unwrap();
        (r.get(0), r.get(1))
    };
    assert_eq!(
        (source.as_str(), accepted.as_str()),
        ("bulk", "2025-03-12 23:59:59")
    );
    // Bulk rows are not visible before the end of their filing day.
    assert!(visible(&mut s.client, "2025-03-12 20:00:00+00").is_empty());
    assert_eq!(
        visible(&mut s.client, "2025-03-13 12:00:00+00"),
        rows_of(FORM5)
    );

    // The same filing offered again from its XML: kept as it is, not a change.
    let again = load_batch(&mut s.client, &[fixture("nvda_0001045810-25-000054")]).unwrap();
    assert_eq!(again.filings_new, 0);
    assert!(again.changed_source.is_empty());
    s.finish();

    // An original and its amendment filed the same day, both from bulk, share
    // one time (23:59:59): they still link.
    let mut s = Scratch::new("bulkday");
    migrate(&mut s.client).unwrap();
    let orig = bulk_form5("0001045810-25-000060", "2025-03-21");
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../fixtures/form4/real/nvda_0001045810-25-000070.xml");
    let mut amend_doc = oe_form4::parse(&std::fs::read_to_string(path).unwrap()).unwrap();
    amend_doc.date_of_original_submission = Some("2025-03-21".parse().unwrap());
    let amend = oe_store::prepare_bulk(
        &oe_bulk::BulkFiling {
            accession: "0001045810-25-000061".into(),
            filing_date: "2025-03-21".parse().unwrap(),
            document: amend_doc,
        },
        "https://www.sec.gov/files/test/2025q1_form345.zip",
    )
    .unwrap();
    let r = oe_store::load_batch_with(&mut s.client, &[orig, amend], false).unwrap();
    assert_eq!(r.links, LinkReport::default(), "no pass when not asked");
    let links = oe_store::link_pass(&mut s.client).unwrap();
    assert_eq!((links.loaded, links.waiting), (1, 0));
    assert_eq!(
        visible(&mut s.client, "2026-01-01 00:00:00+00"),
        rows_of("0001045810-25-000061")
    );
    let st = status(&mut s.client).unwrap();
    assert_eq!(st.bulk_filings, 2);
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn daily_flow_and_trade_history_read_the_ledger() {
    let mut s = Scratch::new("flow");
    migrate(&mut s.client).unwrap();
    load_batch(&mut s.client, &real_fixtures()).unwrap();

    // Berkshire's three Sirius XM purchases (joint filing): every owner counts
    // as having traded, so two owners times three days.
    let h = oe_store::trade_history(&mut s.client, &[908_937], "2020-01-01").unwrap();
    let owners: std::collections::BTreeSet<u64> = h.iter().map(|t| t.owner_cik).collect();
    assert_eq!(owners.into_iter().collect::<Vec<_>>(), [315_090, 1_067_983]);
    assert!(h.iter().all(|t| t.code == "P" && t.issuer_cik == 908_937));
    assert!(
        oe_store::trade_history(&mut s.client, &[908_937], "2030-01-01")
            .unwrap()
            .is_empty()
    );

    let (days, left_out) = oe_store::flow_by_day(&mut s.client).unwrap();
    assert_eq!(left_out, 0, "no implausible amounts in the fixtures");
    let buys: i64 = days.iter().map(|d| d.buys).sum();
    let sells: i64 = days.iter().map(|d| d.sells).sum();
    // Counted from the manifest's P and S rows (amendment rows count once).
    let direct: i64 = s
        .client
        .query_one(
            "SELECT count(*) FROM insider_transaction_as_of(now())
             WHERE NOT is_derivative AND code IN ('P', 'S') AND acquired_disposed IN ('A', 'D')",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(buys + sells, direct);
    assert!(buys > 0 && sells > 0);
    assert!(
        days.windows(2).all(|w| w[0].day < w[1].day),
        "sorted by day"
    );
    s.finish();
}

/// Every real fixture of the four other kinds, prepared as `oe db-load` does.
fn other_fixtures() -> Vec<oe_store::OtherFiling> {
    use oe_store::OtherDoc;
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
    let list = |kind: &str, ext: &str| -> Vec<PathBuf> {
        let mut v: Vec<PathBuf> = std::fs::read_dir(root.join(kind).join("real"))
            .unwrap()
            .map(|e| e.unwrap().path())
            .filter(|p| p.to_string_lossy().ends_with(ext))
            .collect();
        v.sort();
        v
    };
    let side = |p: &PathBuf| {
        let meta = p
            .to_string_lossy()
            .replace(".xml", ".meta.txt")
            .replace(".htm", ".meta.txt");
        Sidecar::parse(&std::fs::read_to_string(meta).unwrap())
    };
    let mut out = Vec::new();
    let mut add = |doc: OtherDoc, s: &Sidecar, raw: &[u8]| {
        out.push(
            oe_store::prepare_other(
                doc,
                s.accession.as_deref().unwrap(),
                s.accepted_at().unwrap(),
                s.url.clone().unwrap_or_default(),
                raw,
            )
            .unwrap(),
        );
    };
    for p in list("form144", ".xml") {
        let raw = std::fs::read(&p).unwrap();
        let f = oe_form144::parse(&String::from_utf8_lossy(&raw)).unwrap();
        add(OtherDoc::Form144(f), &side(&p), &raw);
    }
    for p in list("sched13", ".xml") {
        let raw = std::fs::read(&p).unwrap();
        let s = oe_sched13::parse(&String::from_utf8_lossy(&raw)).unwrap();
        add(OtherDoc::Schedule13(s), &side(&p), &raw);
    }
    for p in list("item408", ".htm") {
        let raw = std::fs::read(&p).unwrap();
        let s = side(&p);
        let doc = OtherDoc::Item408 {
            doc: oe_item408::extract(&String::from_utf8_lossy(&raw)),
            report_type: s.form.clone(),
        };
        add(doc, &s, &raw);
    }
    for p in list("proxy", ".hdr.sgml") {
        let raw = std::fs::read(&p).unwrap();
        let h = oe_edgar::parse_header_sgml(&String::from_utf8_lossy(&raw)).unwrap();
        let s = Sidecar {
            accession: Some(h.accession.clone()),
            accepted_eastern: Some(h.accepted_eastern.clone()),
            ..Sidecar::default()
        };
        let doc = OtherDoc::Contest(oe_store::ContestHeader {
            form: h.form,
            subject: h.subject.unwrap(),
            filed_by: h.filed_by,
        });
        add(doc, &s, &raw);
    }
    out
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn other_forms_load_once_and_keep_no_addresses() {
    let mut s = Scratch::new("other");
    migrate(&mut s.client).unwrap();
    let batch = other_fixtures();
    assert_eq!(batch.len(), 17 + 14 + 10 + 12, "fixtures of the four kinds");

    let first = oe_store::load_other(&mut s.client, &batch).unwrap();
    assert_eq!((first.filings_new, first.filings_existing), (53, 0));
    assert!(first.rows_new > 53);
    assert!(first.changed_source.is_empty());

    let count = |c: &mut Client, sql: &str| -> i64 { c.query_one(sql, &[]).unwrap().get(0) };
    assert_eq!(
        count(&mut s.client, "SELECT count(*) FROM form144_notice"),
        17
    );
    assert_eq!(
        count(&mut s.client, "SELECT count(*) FROM schedule13_filing"),
        14
    );
    assert_eq!(
        count(&mut s.client, "SELECT count(*) FROM contest_filing"),
        12
    );
    assert!(count(&mut s.client, "SELECT count(*) FROM trading_arrangement") >= 10);
    // Every 13D/G holder and every security block came along.
    assert!(count(&mut s.client, "SELECT count(*) FROM schedule13_person") >= 14);
    assert!(count(&mut s.client, "SELECT count(*) FROM form144_security") >= 17);
    // Receipts carry the exact acceptance time from the sidecar.
    assert_eq!(
        count(
            &mut s.client,
            "SELECT count(*) FROM filing f JOIN form144_notice n USING (accession)
             WHERE f.source = 'xml' AND f.accepted_at IS NOT NULL AND length(f.raw_sha256) = 64"
        ),
        17
    );

    // The 13G history passive_to_active@2 reads: only 13Gs, for the issuers
    // asked about, each with its holders and its Eastern acceptance date.
    let g_issuers: Vec<u64> = batch
        .iter()
        .filter_map(|f| match &f.doc {
            oe_store::OtherDoc::Schedule13(s) if s.schedule == oe_sched13::Schedule::G => {
                Some(s.issuer_cik.0)
            }
            _ => None,
        })
        .collect();
    assert!(!g_issuers.is_empty());
    let history = oe_store::schedule13g_history(&mut s.client, &g_issuers).unwrap();
    let stored_g = count(
        &mut s.client,
        "SELECT count(*) FROM schedule13_filing WHERE schedule = 'G'",
    );
    assert_eq!(history.len() as i64, stored_g);
    for g in &history {
        assert!(g_issuers.contains(&g.issuer_cik));
        assert!(!g.holders.is_empty(), "{}", g.accession);
        assert!(
            g.known_on.parse::<oe_core::Date>().is_ok(),
            "{}",
            g.known_on
        );
    }
    assert!(oe_store::schedule13g_history(&mut s.client, &[1])
        .unwrap()
        .is_empty());

    // The Form 144 history planned_sale@2 reads: originals only, from `since`.
    let issuers144: Vec<u64> = batch
        .iter()
        .filter_map(|f| match &f.doc {
            oe_store::OtherDoc::Form144(n) => Some(n.issuer_cik.0),
            _ => None,
        })
        .collect();
    let all = oe_store::form144_history(&mut s.client, &issuers144, "2000-01-01").unwrap();
    let originals = count(
        &mut s.client,
        "SELECT count(*) FROM form144_notice JOIN filing USING (accession) WHERE form_type = '144'",
    );
    assert_eq!(all.len() as i64, originals);
    assert!(originals < 17, "the fixtures include amendments, left out");
    let latest = all.iter().map(|n| n.known_on.clone()).max().unwrap();
    let from_latest = oe_store::form144_history(&mut s.client, &issuers144, &latest).unwrap();
    assert!(!from_latest.is_empty() && from_latest.iter().all(|n| n.known_on >= latest));
    assert!(from_latest.len() < all.len());

    // The adoptions plan_terminated@2 reads: adopted arrangements with a person.
    let registrants: Vec<u64> = batch
        .iter()
        .filter_map(|f| match &f.doc {
            oe_store::OtherDoc::Item408 { doc, .. } => doc.cik.map(|c| c.0),
            _ => None,
        })
        .collect();
    let adoptions = oe_store::adoption_history(&mut s.client, &registrants).unwrap();
    assert_eq!(
        adoptions.len() as i64,
        count(
            &mut s.client,
            "SELECT count(*) FROM trading_arrangement WHERE rule_10b5_1_adopted AND person IS NOT NULL"
        )
    );
    assert!(!adoptions.is_empty(), "the fixtures include adoptions");
    for a in &adoptions {
        assert!(registrants.contains(&a.registrant_cik));
        assert!(a.known_on.parse::<oe_core::Date>().is_ok());
        if let Some(d) = &a.adoption_date {
            assert!(d.parse::<oe_core::Date>().is_ok(), "{d}");
        }
    }

    // The contest history proxy_contest@2 reads, with every filer in order.
    let subjects: Vec<u64> = batch
        .iter()
        .filter_map(|f| match &f.doc {
            oe_store::OtherDoc::Contest(c) => Some(c.subject.cik.0),
            _ => None,
        })
        .collect();
    let contests = oe_store::contest_history(&mut s.client, &subjects).unwrap();
    assert_eq!(contests.len(), 12);
    for c in &contests {
        let original = batch
            .iter()
            .find(|f| f.filing.accession == c.accession)
            .unwrap();
        let oe_store::OtherDoc::Contest(h) = &original.doc else {
            panic!("{} is not a contest filing", c.accession)
        };
        assert_eq!(
            (&c.form, &c.subject, &c.filed_by),
            (&h.form, &h.subject, &h.filed_by)
        );
        assert!(c.known_on.parse::<oe_core::Date>().is_ok());
    }

    // A repeat writes nothing.
    let again = oe_store::load_other(&mut s.client, &batch).unwrap();
    assert_eq!(
        (again.filings_new, again.filings_existing, again.rows_new),
        (0, 53, 0)
    );

    // A changed source is reported and not overwritten.
    let mut changed = batch[0].clone();
    changed.filing.raw_sha256 = "0".repeat(64);
    let r = oe_store::load_other(&mut s.client, std::slice::from_ref(&changed)).unwrap();
    assert_eq!(r.changed_source, [changed.filing.accession.clone()]);

    // Append-only, like the rest of the ledger.
    assert!(s
        .client
        .batch_execute("UPDATE schedule13_person SET name = 'x'")
        .is_err());

    // No column for street addresses or CUSIPs in any new table.
    let bad = count(
        &mut s.client,
        "SELECT count(*) FROM information_schema.columns
         WHERE table_schema = current_schema()
           AND table_name IN ('form144_notice', 'form144_security', 'schedule13_filing',
                              'schedule13_person', 'contest_filing', 'contest_filer',
                              'trading_arrangement')
           AND (column_name ILIKE '%street%' OR column_name ILIKE '%address%'
                OR column_name ILIKE '%zip%' OR column_name ILIKE '%cusip%')",
    );
    assert_eq!(bad, 0);
    let st = status(&mut s.client).unwrap();
    assert_eq!(
        (
            st.form144_notices,
            st.schedule13_filings,
            st.contest_filings
        ),
        (17, 14, 12)
    );
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn officials_and_house_reports_load_once_link_and_are_found_by_name() {
    let mut s = Scratch::new("officials");
    migrate(&mut s.client).unwrap();
    // Invented people (no real members or reports in the repository).
    let (people, skipped) = oe_officials::parse_legislators(
        r#"[{"id": {"bioguide": "Z000001", "fec": ["H0ZZ"]},
             "name": {"first": "Alex", "last": "Gutiérrez", "official_full": "Alex Gutiérrez"},
             "bio": {"birthday": "1960-01-01"},
             "terms": [{"type": "rep", "start": "2019-01-03", "end": "2025-01-03", "state": "ZZ", "district": 4, "party": "Independent",
                        "phone": "000"}]},
            {"id": {"bioguide": "Z000002"}, "name": {"first": "Sam", "last": "Tester"},
             "terms": [{"type": "sen", "start": "2021-01-03", "end": "2027-01-03", "state": "ZZ"}]}]"#,
    )
    .unwrap();
    assert_eq!(skipped, 0);
    let committees = oe_officials::parse_committees(
        r#"[{"type": "house", "name": "Committee on Examples", "thomas_id": "HSZZ", "subcommittees": []}]"#,
    )
    .unwrap();
    let memberships = oe_officials::parse_memberships(
        r#"{"HSZZ": [{"bioguide": "Z000001", "rank": 1, "title": "Chair", "party": "majority"},
                      {"bioguide": "Z999999", "rank": 2, "party": "minority"}]}"#,
    )
    .unwrap();
    let m = oe_store::load_members(
        &mut s.client,
        &people,
        &committees,
        &memberships,
        "2026-11-24",
    )
    .unwrap();
    assert_eq!((m.officials_new, m.terms_new, m.committees_new), (2, 2, 1));
    assert_eq!((m.memberships_new, m.memberships_unknown), (1, 1));
    let again = oe_store::load_members(
        &mut s.client,
        &people,
        &committees,
        &memberships,
        "2026-11-24",
    )
    .unwrap();
    assert_eq!(
        (again.officials_new, again.terms_new, again.memberships_new),
        (0, 0, 0)
    );

    let report =
        |doc: &str, filed: &str, link: Result<(String, String), String>| oe_store::HouseReportRow {
            doc_id: doc.into(),
            year: 2024,
            filing_type: "P".into(),
            prefix: "Hon.".into(),
            last: "Gutierrez".into(),
            first: "Alex".into(),
            suffix: String::new(),
            state_district: "ZZ04".into(),
            filed_on: Some(filed.into()),
            scanned: doc.len() < 8,
            source_url: format!("https://example.invalid/{doc}.pdf"),
            first_seen_at: Some("2026-11-24T10:30:00Z".into()),
            pdf_sha256: Some("ab".repeat(32)),
            link,
        };
    let rows = [
        report(
            "20000001",
            "2024-03-01",
            Ok(("Z000001".into(), "seat and surname".into())),
        ),
        report(
            "8000002",
            "2024-06-01",
            Ok(("Z000001".into(), "seat and surname".into())),
        ),
        report(
            "20000003",
            "2024-07-01",
            Err("no House member named Nobody served ZZ09".into()),
        ),
    ];
    let w = oe_store::load_house_reports(&mut s.client, &rows).unwrap();
    assert_eq!((w.reports_new, w.links_new, w.held_back_new), (3, 2, 1));
    let w = oe_store::load_house_reports(&mut s.client, &rows).unwrap();
    assert_eq!((w.reports_new, w.links_new, w.held_back_new), (0, 0, 0));

    // Found without the accent, newest report first, with the committee.
    let found = oe_store::find_officials(&mut s.client, "gutierrez", 10).unwrap();
    assert_eq!(found.len(), 1);
    let o = &found[0];
    assert_eq!(
        (o.bioguide.as_str(), o.reports, o.scanned_reports),
        ("Z000001", 2, 1)
    );
    assert_eq!(
        (o.first_filed.as_deref(), o.last_filed.as_deref()),
        (Some("2024-03-01"), Some("2024-06-01"))
    );
    assert_eq!(o.recent[0].1, "8000002");
    assert_eq!(o.terms, ["rep ZZ-04 2019-01-03..2025-01-03 Independent"]);
    assert_eq!(
        o.committees,
        [(
            "Committee on Examples".to_string(),
            Some("Chair".to_string()),
            "2026-11-24".to_string()
        )]
    );
    assert!(oe_store::find_officials(&mut s.client, "nobody", 10)
        .unwrap()
        .is_empty());
    assert_eq!(
        oe_store::find_officials(&mut s.client, "sam", 10).unwrap()[0].reports,
        0
    );

    // Append-only, and nothing personal beyond names and terms.
    assert!(s
        .client
        .batch_execute("UPDATE official SET last_name = 'x'")
        .is_err());
    assert!(s
        .client
        .batch_execute("DELETE FROM house_report_link")
        .is_err());
    let bad: i64 = s
        .client
        .query_one(
            "SELECT count(*) FROM information_schema.columns
             WHERE table_schema = current_schema()
               AND table_name IN ('official', 'official_term', 'committee', 'committee_membership', 'house_report', 'house_report_link')
               AND (column_name ~ '(birth|gender|address|phone|fec|opensecrets|cusip)')",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(bad, 0);
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn names_and_roles_are_stored_and_the_name_index_finds_them() {
    let mut s = Scratch::new("people");
    migrate(&mut s.client).unwrap();
    let batch = real_fixtures();
    load_batch(&mut s.client, &batch).unwrap();
    let count = |c: &mut Client, sql: &str| -> i64 { c.query_one(sql, &[]).unwrap().get(0) };
    let owners = count(&mut s.client, "SELECT count(*) FROM filing_owner");
    assert_eq!(
        count(&mut s.client, "SELECT count(*) FROM filing_owner_role"),
        owners,
        "one role row per owner row"
    );
    assert_eq!(
        count(&mut s.client, "SELECT count(*) FROM ownership_issuer"),
        25
    );
    assert_eq!(
        count(
            &mut s.client,
            "SELECT count(*) FROM filing_owner_role r JOIN filing_owner o USING (accession, position)
             WHERE r.owner_cik <> o.owner_cik"
        ),
        0
    );
    // The joint filing keeps both owners' names and relationships.
    let joint = s
        .client
        .query(
            "SELECT owner_cik, owner_name, is_ten_percent_owner FROM filing_owner_role
             WHERE accession = '0001193125-26-445930' ORDER BY position",
            &[],
        )
        .unwrap();
    assert_eq!(joint.len(), 2);
    assert!(joint.iter().all(|r| !r.get::<_, &str>(1).is_empty()));

    // A repeat load writes nothing; the tables are append-only.
    load_batch(&mut s.client, &batch).unwrap();
    assert_eq!(
        count(&mut s.client, "SELECT count(*) FROM filing_owner_role"),
        owners
    );
    assert!(s
        .client
        .batch_execute("UPDATE filing_owner_role SET owner_name = 'x'")
        .is_err());
    assert!(s
        .client
        .batch_execute("DELETE FROM ownership_issuer")
        .is_err());
    // No address in the new tables (ADR 0009, 0010).
    assert_eq!(
        count(
            &mut s.client,
            "SELECT count(*) FROM information_schema.columns
             WHERE table_schema = current_schema()
               AND table_name IN ('filing_owner_role', 'ownership_issuer', 'people_candidate', 'people_name', 'people_token')
               AND column_name ~ '(street|city|zip|address|state)'"
        ),
        0
    );

    // An invented member of Congress, to be found next to the insiders.
    let (people, _) = oe_officials::parse_legislators(
        r#"[{"id": {"bioguide": "Z000001"},
             "name": {"first": "Alex", "last": "Gutiérrez", "official_full": "Alex Gutiérrez"},
             "terms": [{"type": "rep", "start": "2019-01-03", "end": "2025-01-03", "state": "ZZ", "district": 4}]}]"#,
    )
    .unwrap();
    oe_store::load_members(&mut s.client, &people, &[], &[], "2026-11-24").unwrap();

    let first = oe_store::build_people_index(&mut s.client).unwrap();
    let again = oe_store::build_people_index(&mut s.client).unwrap();
    assert_eq!(first, again, "a rebuild gives the same index");
    assert_eq!(first.officials, 1);
    let distinct_owners = count(
        &mut s.client,
        "SELECT count(DISTINCT owner_cik) FROM filing_owner_role",
    );
    assert!(first.candidates as i64 > distinct_owners);

    // Each joint owner is found by the name it filed under, in page order.
    for row in &joint {
        let cik: i64 = row.get(0);
        let name: &str = row.get(1);
        let mut words = oe_core::text::name_tokens(name);
        words.reverse();
        let query = words.join(" ");
        let oe_store::Lookup::Found(cards) = oe_store::lookup(&mut s.client, &query, 10).unwrap()
        else {
            panic!("{query:?} should find candidates");
        };
        let card = cards
            .iter()
            .find(|c| c.key == format!("cik:{cik}"))
            .unwrap_or_else(|| panic!("{query:?} should find cik:{cik}"));
        assert!(card.exact);
        assert!(card.kinds.contains(&"insider".to_string()));
        assert!(!card.roles.is_empty());
        assert!(card.roles.iter().all(|r| !r.issuer_name.is_empty()));
        for t in &card.trades {
            assert!(t.code == "P" || t.code == "S");
            assert!(t
                .receipt
                .url
                .starts_with("https://www.sec.gov/Archives/edgar/data/"));
        }
    }

    // The member, without the accent; initials alone are not a name.
    let oe_store::Lookup::Found(cards) =
        oe_store::lookup(&mut s.client, "alex gutierrez", 10).unwrap()
    else {
        panic!("the member should be found");
    };
    assert_eq!(cards.len(), 1);
    assert_eq!(cards[0].key, "bioguide:Z000001");
    assert_eq!(
        cards[0].official.as_ref().map(|o| o.name.as_str()),
        Some("Alex Gutiérrez")
    );
    assert_eq!(
        oe_store::lookup(&mut s.client, "A. G.", 10).unwrap(),
        oe_store::Lookup::NeedsAName
    );
    assert_eq!(
        oe_store::lookup(&mut s.client, "nobody zzzz", 10).unwrap(),
        oe_store::Lookup::Found(Vec::new())
    );

    // A long form of the first name finds the member filed as "Alex".
    let oe_store::Lookup::Found(cards) =
        oe_store::lookup(&mut s.client, "Alexander Gutierrez", 10).unwrap()
    else {
        panic!("a nickname's long form should find the member");
    };
    assert_eq!(cards[0].key, "bioguide:Z000001");
    assert!(!cards[0].exact, "the name typed is not the one filed");

    // A first name nobody filed under: people with the surname are offered.
    match oe_store::lookup(&mut s.client, "Zzzz Gutierrez", 10).unwrap() {
        oe_store::Lookup::Related { word, cards } => {
            assert_eq!(word, "GUTIERREZ");
            assert!(cards.iter().any(|c| c.key == "bioguide:Z000001"));
            assert!(cards.iter().all(|c| !c.exact));
        }
        other => panic!("expected related names, got {other:?}"),
    }
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn the_event_feed_is_replaced_whole_and_filtered() {
    let mut s = Scratch::new("events");
    migrate(&mut s.client).unwrap();
    // Invented events.
    let ev = |key: &str,
              rule: &str,
              day: &str,
              ticker: Option<&str>,
              amount: Option<f64>,
              headline: &str| {
        oe_store::EventRow {
            key: key.into(),
            rule_id: rule.into(),
            known_on: Some(day.into()),
            issuer_cik: Some(9_999_901),
            issuer_name: Some("Example 100% Holdings".into()),
            ticker: ticker.map(str::to_string),
            headline: headline.into(),
            amount_usd: amount,
            percent: None,
            body: format!("{{\"key\":\"{key}\"}}"),
        }
    };
    let rows = vec![
        ev(
            "a",
            "insider.cluster_buy@1",
            "2026-11-02",
            Some("EXMP"),
            Some(2e6),
            "3 insiders bought EXMP",
        ),
        ev(
            "b",
            "insider.planned_sale@2",
            "2026-11-03",
            Some("exmp"),
            Some(5e5),
            "planned sale of EXMP",
        ),
        ev(
            "c",
            "activist.new_13d@1",
            "2026-10-30",
            None,
            None,
            "New 13D by Zeta_Fund",
        ),
        ev(
            "a",
            "insider.cluster_buy@1",
            "2026-11-02",
            None,
            None,
            "duplicate key, dropped",
        ),
    ];
    assert_eq!(
        oe_store::replace_event_feed(&mut s.client, &rows).unwrap(),
        3
    );
    assert_eq!(
        oe_store::replace_event_feed(&mut s.client, &rows[..2]).unwrap(),
        2,
        "a rebuild replaces the feed"
    );
    oe_store::replace_event_feed(&mut s.client, &rows).unwrap();

    let keys = |c: &mut Client, f: oe_store::EventFilter| -> (i64, Vec<String>) {
        let page = oe_store::query_events(c, &f).unwrap();
        assert!(page.built_at.is_some());
        (page.total, page.events)
    };
    let all = oe_store::EventFilter {
        limit: 50,
        ..Default::default()
    };
    assert_eq!(
        keys(&mut s.client, all.clone()),
        (
            3,
            vec![
                r#"{"key":"b"}"#.to_string(),
                r#"{"key":"a"}"#.into(),
                r#"{"key":"c"}"#.into()
            ]
        ),
        "newest first"
    );
    let f = |g: fn(&mut oe_store::EventFilter)| {
        let mut x = all.clone();
        g(&mut x);
        x
    };
    assert_eq!(
        keys(
            &mut s.client,
            f(|x| x.rule = Some("insider.cluster_buy".into()))
        )
        .0,
        1
    );
    assert_eq!(
        keys(
            &mut s.client,
            f(|x| x.rule = Some("insider.cluster_buy@2".into()))
        )
        .0,
        0
    );
    assert_eq!(
        keys(&mut s.client, f(|x| x.ticker = Some("Exmp".into()))).0,
        2
    );
    assert_eq!(keys(&mut s.client, f(|x| x.min_amount = Some(1e6))).0, 1);
    assert_eq!(
        keys(&mut s.client, f(|x| x.since = Some("2026-11-01".into()))).0,
        2
    );
    assert_eq!(
        keys(&mut s.client, f(|x| x.until = Some("2026-10-31".into()))).0,
        1
    );
    assert_eq!(keys(&mut s.client, f(|x| x.issuer_cik = Some(1))).0, 0);
    // Words in the headline or the company name; % and _ are plain letters.
    assert_eq!(
        keys(&mut s.client, f(|x| x.text = Some("zeta_fund 13d".into()))).0,
        1
    );
    assert_eq!(
        keys(&mut s.client, f(|x| x.text = Some("zeta%fund".into()))).0,
        0
    );
    assert_eq!(
        keys(&mut s.client, f(|x| x.text = Some("100% planned".into()))).0,
        1
    );
    let page = f(|x| {
        x.limit = 1;
        x.offset = 1
    });
    assert_eq!(
        keys(&mut s.client, page),
        (3, vec![r#"{"key":"a"}"#.to_string()])
    );

    let counts = oe_store::event_counts(&mut s.client).unwrap();
    assert_eq!(counts.len(), 3);
    assert_eq!(
        counts[0],
        (
            "2026-10-30".to_string(),
            "activist.new_13d@1".to_string(),
            1
        )
    );

    // Tickers for events whose filings name none: the issuer's latest Form 3/4/5.
    load_batch(&mut s.client, &real_fixtures()).unwrap();
    let tickers = oe_store::latest_tickers(&mut s.client).unwrap();
    assert!(!tickers.is_empty());
    assert!(tickers
        .values()
        .all(|t| !t.is_empty() && t.to_uppercase() == *t));
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn ledger_purchases_come_with_names_and_leave_out_impossible_dates() {
    let mut s = Scratch::new("purchases");
    migrate(&mut s.client).unwrap();
    load_batch(&mut s.client, &real_fixtures()).unwrap();
    let all_p: i64 = s
        .client
        .query_one(
            "SELECT count(*) FROM insider_transaction_as_of(now())
             WHERE NOT is_derivative AND code = 'P' AND acquired_disposed IS DISTINCT FROM 'D'",
            &[],
        )
        .unwrap()
        .get(0);
    let (items, left_out) = oe_store::ledger_purchases(&mut s.client).unwrap();
    assert_eq!(items.len() as i64 + left_out.implausible_dates, all_p);
    assert!(
        !items.is_empty(),
        "the real fixtures hold open-market purchases"
    );
    for p in &items {
        assert!(
            p.date.as_str() >= "2015-01-01" && p.date <= p.known_on,
            "{p:?}"
        );
        assert!(!p.accession.is_empty());
        assert!(!p.owner_name.is_empty(), "names are stored on load: {p:?}");
    }
    // A purchase dated years after its filing is a typo as filed: left out.
    s.client
        .batch_execute(
            "ALTER TABLE insider_transaction DISABLE TRIGGER USER;
             UPDATE insider_transaction SET transaction_date = '2033-01-01'
              WHERE id = (SELECT id FROM insider_transaction
                          WHERE NOT is_derivative AND code = 'P' LIMIT 1);
             ALTER TABLE insider_transaction ENABLE TRIGGER USER;",
        )
        .unwrap();
    let (after, left_after) = oe_store::ledger_purchases(&mut s.client).unwrap();
    assert_eq!(after.len(), items.len() - 1);
    assert_eq!(left_after.implausible_dates, left_out.implausible_dates + 1);
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn a_company_shows_its_insiders_trades_and_flow() {
    let mut s = Scratch::new("company");
    migrate(&mut s.client).unwrap();
    load_batch(&mut s.client, &real_fixtures()).unwrap();
    // The company with the most fixture filings.
    let cik: i64 = s
        .client
        .query_one(
            "SELECT issuer_cik FROM ownership_issuer GROUP BY 1 ORDER BY count(*) DESC, 1 LIMIT 1",
            &[],
        )
        .unwrap()
        .get(0);
    let c = oe_store::company(&mut s.client, cik)
        .unwrap()
        .expect("in the fixtures");
    assert_eq!(c.cik, cik);
    assert!(!c.name.is_empty());
    assert!(c.filings >= 1 && c.first_on <= c.last_on);
    assert!(!c.insiders.is_empty());
    assert!(c
        .insiders
        .iter()
        .all(|i| !i.name.is_empty() && i.first_on <= i.last_on));
    for t in &c.trades {
        assert!(t.code == "P" || t.code == "S");
        assert!(t
            .receipt
            .url
            .starts_with("https://www.sec.gov/Archives/edgar/data/"));
    }
    assert!(
        c.flow.windows(2).all(|w| w[0].0 < w[1].0),
        "one row per day, in order"
    );
    assert!(c.flow.iter().all(|d| d.1 >= 0.0 && d.3 >= 0.0));
    assert_eq!(oe_store::company(&mut s.client, 1).unwrap(), None);

    // The search box reads the event feed.
    oe_store::replace_event_feed(
        &mut s.client,
        &[oe_store::EventRow {
            key: "k".into(),
            rule_id: "activist.new_13d@1".into(),
            known_on: Some("2026-11-02".into()),
            issuer_cik: Some(9_999_901),
            issuer_name: Some("Example_Widgets Corp".into()),
            ticker: Some("EXW".into()),
            headline: "New 13D".into(),
            amount_usd: None,
            percent: None,
            body: "{}".into(),
        }],
    )
    .unwrap();
    let found = oe_store::find_companies(&mut s.client, "example_w", 10).unwrap();
    assert_eq!(
        found,
        vec![(
            9_999_901,
            "Example_Widgets Corp".to_string(),
            Some("EXW".to_string())
        )]
    );
    assert_eq!(
        oe_store::find_companies(&mut s.client, "exw", 10)
            .unwrap()
            .len(),
        1,
        "by ticker"
    );
    assert!(
        oe_store::find_companies(&mut s.client, "example%", 10)
            .unwrap()
            .is_empty(),
        "% is a letter"
    );
    assert!(oe_store::find_companies(&mut s.client, " ", 10)
        .unwrap()
        .is_empty());
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn trade_marks_group_by_day_owner_and_code_for_a_company_or_a_person() {
    let mut s = Scratch::new("marks");
    migrate(&mut s.client).unwrap();
    load_batch(&mut s.client, &real_fixtures()).unwrap();
    let (issuer, owner): (i64, i64) = {
        let r = s
            .client
            .query_one(
                "SELECT t.issuer_cik, t.owner_cik FROM insider_transaction t
                 WHERE NOT t.is_derivative AND t.code IN ('P', 'S')
                 GROUP BY 1, 2 ORDER BY count(*) DESC, 1, 2 LIMIT 1",
                &[],
            )
            .unwrap();
        (r.get(0), r.get(1))
    };
    let by_company =
        oe_store::trade_marks(&mut s.client, oe_store::TradesOf::Issuer(issuer)).unwrap();
    let by_person = oe_store::trade_marks(&mut s.client, oe_store::TradesOf::Owner(owner)).unwrap();
    assert!(!by_company.is_empty() && !by_person.is_empty());
    for m in by_company.iter().chain(&by_person) {
        assert!(m.code == "P" || m.code == "S");
        assert!(m.trades >= 1 && m.shares > 0.0);
        assert!(m.value >= 0.0 && m.priced_shares <= m.shares + 1e-6);
        assert!(m.plan_trades <= m.plan_known && m.plan_known <= m.trades);
        assert!(m.day <= m.receipt.known_on, "never dated after its filing");
        assert!(m
            .receipt
            .url
            .starts_with("https://www.sec.gov/Archives/edgar/data/"));
    }
    assert!(by_company.iter().all(|m| m.issuer_cik == issuer));
    assert!(by_person.iter().all(|m| m.owner_cik == owner));
    assert!(
        by_company.windows(2).all(|w| w[0].day <= w[1].day),
        "oldest first"
    );
    // One mark per day, owner and code.
    let mut keys: Vec<_> = by_company
        .iter()
        .map(|m| (&m.day, m.owner_cik, &m.code))
        .collect();
    let n = keys.len();
    keys.dedup();
    assert_eq!(keys.len(), n);
    // The person's marks at this company are the company's marks for them.
    let theirs: i64 = by_company
        .iter()
        .filter(|m| m.owner_cik == owner)
        .map(|m| m.trades)
        .sum();
    let here: i64 = by_person
        .iter()
        .filter(|m| m.issuer_cik == issuer)
        .map(|m| m.trades)
        .sum();
    assert_eq!(theirs, here);
    assert!(
        oe_store::trade_marks(&mut s.client, oe_store::TradesOf::Issuer(1))
            .unwrap()
            .is_empty()
    );
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn prices_are_checked_against_insider_prices_and_outcomes_measured_from_the_public_day() {
    let mut s = Scratch::new("prices");
    migrate(&mut s.client).unwrap();
    load_batch(&mut s.client, &real_fixtures()).unwrap();
    let wanted = oe_store::wanted_symbols(&mut s.client).unwrap();
    // Two companies whose insiders reported prices: one gets a series that
    // agrees with them, the other one ten times too high.
    let priced: Vec<(i64, String)> = wanted
        .iter()
        .filter(|(cik, _)| {
            s.client
                .query_one(
                    "SELECT count(*) FROM insider_transaction WHERE issuer_cik = $1 AND NOT is_derivative
                     AND code IN ('P','S') AND price_per_share > 0",
                    &[cik],
                )
                .unwrap()
                .get::<_, i64>(0)
                >= 1
        })
        .cloned()
        .collect();
    assert!(
        priced.len() >= 2,
        "the fixtures have two companies with priced trades"
    );
    let (good, bad) = (priced[0].clone(), priced[1].clone());
    let reported = |c: &mut Client, cik: i64| -> std::collections::BTreeMap<String, f64> {
        c.query(
            "SELECT transaction_date::text, avg(price_per_share)::float8 FROM insider_transaction
             WHERE issuer_cik = $1 AND NOT is_derivative AND code IN ('P','S') AND price_per_share > 0
             GROUP BY 1",
            &[&cik],
        )
        .unwrap()
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect()
    };
    let good_prices = reported(&mut s.client, good.0);
    let bad_prices = reported(&mut s.client, bad.0);
    // Every weekday 2014-2027: the reported price on its days, otherwise the
    // last one; after the last trade, up 0.1% a day. The market stays flat.
    let mut days = Vec::new();
    let mut d: oe_core::Date = "2014-01-01".parse().unwrap();
    let end: oe_core::Date = "2027-12-31".parse().unwrap();
    let monday: oe_core::Date = "2014-01-06".parse().unwrap();
    while d <= end {
        if monday.days_until(d).rem_euclid(7) < 5 {
            days.push(d);
        }
        d = d.add_days(1);
    }
    let series = |reported: &std::collections::BTreeMap<String, f64>,
                  scale: f64|
     -> Vec<(oe_core::Date, f64)> {
        let last_trade = reported.keys().last().unwrap().clone();
        let mut price = *reported.values().next().unwrap();
        days.iter()
            .map(|d| {
                let k = d.to_string();
                if let Some(p) = reported.get(&k) {
                    price = *p;
                } else if k > last_trade {
                    price *= 1.001;
                }
                (*d, price * scale)
            })
            .collect()
    };
    let good_series = series(&good_prices, 1.0);
    let bad_series = series(&bad_prices, 10.0);
    let flat: Vec<_> = days.iter().map(|d| (*d, 400.0)).collect();
    let r = oe_store::import_prices(&mut s.client, "invented", |sink| {
        sink(&good.1, &good_series)?;
        sink(&bad.1, &bad_series)?;
        sink(oe_store::BENCHMARK, &flat)?;
        sink("nobody.us", &[])?;
        Ok(())
    })
    .unwrap()
    .unwrap();
    assert_eq!(r.symbols, 3, "an empty series is not stored");
    assert_eq!(r.days as usize, days.len() * 3);
    // A failed import keeps nothing.
    let failed = oe_store::import_prices(&mut s.client, "broken", |sink| {
        sink("x.us", &flat)?;
        Err("the file stopped".into())
    })
    .unwrap();
    assert!(failed.is_err());
    let kept: i64 = s
        .client
        .query_one("SELECT count(DISTINCT symbol) FROM price_daily", &[])
        .unwrap()
        .get(0);
    assert_eq!(kept, 3, "the earlier import is still there");

    let check = oe_store::check_series(&mut s.client).unwrap();
    assert_eq!((check.ok, check.mismatch), (1, 1));
    let status = |c: &mut Client, cik: i64| -> String {
        c.query_one(
            "SELECT status FROM issuer_price_series WHERE issuer_cik = $1",
            &[&cik],
        )
        .unwrap()
        .get(0)
    };
    assert_eq!(status(&mut s.client, good.0), "ok");
    assert_eq!(status(&mut s.client, bad.0), "mismatch");

    // An event at the good company, public on its last trade day.
    let last_trade = good_prices.keys().last().unwrap().clone();
    oe_store::replace_event_feed(
        &mut s.client,
        &[oe_store::EventRow {
            key: "evt".into(),
            rule_id: "insider.cluster_buy@1".into(),
            known_on: Some(last_trade.clone()),
            issuer_cik: Some(good.0 as u64),
            issuer_name: None,
            ticker: None,
            headline: "invented".into(),
            amount_usd: None,
            percent: None,
            body: "{}".into(),
        }],
    )
    .unwrap();
    let built = oe_store::build_outcomes(&mut s.client).unwrap();
    assert_eq!(built.events, 1);
    assert!(
        built.buys + built.sales >= 1,
        "the good company's trades are measured"
    );
    let ev = oe_store::outcomes(&mut s.client, &oe_store::OutcomesOf::Issuer(good.0)).unwrap();
    assert_eq!(
        ev.len(),
        1,
        "the company's events, not the yardstick's months"
    );
    let months: i64 = s
        .client
        .query_one(
            "SELECT count(*) FROM outcome WHERE family = 'baseline.any_month'",
            &[],
        )
        .unwrap()
        .get(0);
    assert!(
        months > 100,
        "the yardstick: one start a month since 2015 ({months})"
    );
    let e = &ev[0];
    assert!(
        e.entry_day > e.known_on,
        "entered after the public day, never on it"
    );
    // 0.1% a day for 21 trading days, and a flat market: return = excess.
    let r21 = e.returns[0].unwrap();
    assert!((r21 - (1.001f64.powi(21) - 1.0)).abs() < 1e-9, "{r21}");
    assert!((e.returns[4].unwrap() - r21).abs() < 1e-12);
    assert!(e.returns[3].unwrap() > e.returns[2].unwrap());
    // Nothing at the company whose prices did not agree.
    let none: i64 = s
        .client
        .query_one(
            "SELECT count(*) FROM outcome WHERE issuer_cik = $1",
            &[&bad.0],
        )
        .unwrap()
        .get(0);
    assert_eq!(none, 0);

    let summary = oe_store::outcome_summary(&mut s.client).unwrap();
    let cluster = summary
        .iter()
        .find(|g| g.family == "insider.cluster_buy")
        .unwrap();
    assert_eq!((cluster.kind.as_str(), cluster.count), ("event", 1));
    assert_eq!(cluster.horizons.len(), 4);
    assert_eq!(cluster.horizons[0].share_beat, Some(1.0));
    let best = oe_store::outcomes(
        &mut s.client,
        &oe_store::OutcomesOf::Extremes {
            family: "insider.cluster_buy".into(),
            best: true,
            limit: 5,
        },
    )
    .unwrap();
    assert_eq!(best.len(), 1);
    assert_eq!(
        best[0].ticker.as_deref(),
        Some(good.1.trim_end_matches(".us").to_ascii_uppercase().as_str())
    );
    let _ = oe_store::insider_records(&mut s.client, 1, 10, true).unwrap();

    let (sym, closes) = oe_store::prices_for(&mut s.client, good.0)
        .unwrap()
        .unwrap();
    assert_eq!((sym, closes.len()), (good.1.clone(), days.len()));
    assert!(
        oe_store::prices_for(&mut s.client, bad.0)
            .unwrap()
            .is_none(),
        "never a mismatched series"
    );
    let st = oe_store::price_status(&mut s.client).unwrap();
    assert_eq!(
        (st.series_ok, st.series_mismatch, st.benchmark),
        (1, 1, true)
    );
    assert_eq!(st.source.as_deref(), Some("invented"));
    s.finish();
}

#[test]
#[ignore = "needs OE_TEST_DATABASE_URL"]
fn house_transactions_are_replaced_whole_filtered_and_ranked() {
    let mut s = Scratch::new("housetx");
    migrate(&mut s.client).unwrap();
    // An invented member and two invented reports.
    let (people, _) = oe_officials::parse_legislators(
        r#"[{"id": {"bioguide": "Z000001"}, "name": {"first": "Alex", "last": "Example", "official_full": "Alex Example"},
             "terms": [{"type": "rep", "start": "2019-01-03", "end": "2027-01-03", "state": "ZZ", "district": 4}]}]"#,
    )
    .unwrap();
    oe_store::load_members(&mut s.client, &people, &[], &[], "2026-11-24").unwrap();
    let report = |doc: &str, filed: &str| oe_store::HouseReportRow {
        doc_id: doc.into(),
        year: 2026,
        filing_type: "P".into(),
        prefix: "Hon.".into(),
        last: "Example".into(),
        first: "Alex".into(),
        suffix: String::new(),
        state_district: "ZZ04".into(),
        filed_on: Some(filed.into()),
        scanned: false,
        source_url: format!(
            "https://disclosures-clerk.house.gov/public_disc/ptr-pdfs/2026/{doc}.pdf"
        ),
        first_seen_at: None,
        pdf_sha256: None,
        link: Ok(("Z000001".into(), "seat and surname".into())),
    };
    oe_store::load_house_reports(
        &mut s.client,
        &[
            report("20090001", "2026-06-23"),
            report("20090002", "2026-08-21"),
        ],
    )
    .unwrap();
    let tx = |n: i32, t: &str, ticker: &str, day: &str, owner: Option<&str>| oe_store::HouseTxRow {
        line_no: n,
        owner: owner.map(str::to_string),
        asset: format!("Example {ticker} Common Stock ({ticker}) [ST]"),
        ticker: Some(ticker.into()),
        asset_type: Some("ST".into()),
        tx_type: t.into(),
        tx_date: Some(day.into()),
        notified_on: Some(day.into()),
        amount: "$1,001 - $15,000".into(),
        amount_min: Some(1001),
        amount_max: Some(15000),
        issuer_cik: Some(9_999_901),
        ..Default::default()
    };
    let reads = vec![
        oe_store::HouseRead {
            doc_id: "20090001".into(),
            rows: vec![
                tx(1, "P", "EXA", "2026-05-29", Some("SP")),
                tx(2, "S (partial)", "EXB", "2026-06-01", None),
            ],
            problems: vec![],
        },
        oe_store::HouseRead {
            doc_id: "20090002".into(),
            rows: vec![tx(1, "P", "EXA", "2026-07-24", Some("SP"))],
            problems: vec!["one".into()],
        },
        oe_store::HouseRead {
            doc_id: "99999999".into(),
            rows: vec![tx(1, "P", "EXZ", "2026-07-24", None)],
            problems: vec![],
        },
    ];
    let r = oe_store::replace_house_transactions(&mut s.client, &reads, "test").unwrap();
    assert_eq!(
        (r.reports, r.transactions, r.with_issuer),
        (2, 3, 3),
        "an unknown report is skipped"
    );
    let again = oe_store::replace_house_transactions(&mut s.client, &reads[..1], "test").unwrap();
    assert_eq!(again.transactions, 2, "a load replaces the earlier one");
    oe_store::replace_house_transactions(&mut s.client, &reads, "test").unwrap();

    let all = oe_store::OfficialTradeFilter {
        limit: 50,
        ..Default::default()
    };
    let (total, rows) = oe_store::official_trades(&mut s.client, &all).unwrap();
    assert_eq!(total, 3);
    assert_eq!(
        rows[0].tx_date.as_deref(),
        Some("2026-07-24"),
        "newest first"
    );
    assert_eq!(
        (rows[0].member.as_str(), rows[0].bioguide.as_deref()),
        ("Alex Example", Some("Z000001"))
    );
    assert!(rows[0].report_url.ends_with("20090002.pdf"));
    let f = |g: fn(&mut oe_store::OfficialTradeFilter)| {
        let mut x = all.clone();
        g(&mut x);
        x
    };
    assert_eq!(
        oe_store::official_trades(&mut s.client, &f(|x| x.ticker = Some("exa".into())))
            .unwrap()
            .0,
        2
    );
    assert_eq!(
        oe_store::official_trades(&mut s.client, &f(|x| x.side = Some("sale".into())))
            .unwrap()
            .0,
        1
    );
    assert_eq!(
        oe_store::official_trades(&mut s.client, &f(|x| x.since = Some("2026-07-01".into())))
            .unwrap()
            .0,
        1
    );
    assert_eq!(
        oe_store::official_trades(&mut s.client, &f(|x| x.bioguide = Some("Z000002".into())))
            .unwrap()
            .0,
        0
    );
    assert_eq!(
        oe_store::official_trades(&mut s.client, &f(|x| x.issuer_cik = Some(9_999_901)))
            .unwrap()
            .0,
        3
    );

    let active = oe_store::member_activity(&mut s.client, None, 10).unwrap();
    assert_eq!(active.len(), 1);
    let a = &active[0];
    assert_eq!(
        (a.trades, a.buys, a.sales, a.low_total, a.high_total),
        (3, 2, 1, 3003, 45000)
    );
    assert_eq!(a.top_tickers, vec!["EXA".to_string(), "EXB".to_string()]);
    assert_eq!(
        oe_store::member_activity(&mut s.client, Some("2026-07-01"), 10).unwrap()[0].trades,
        1
    );
    // No address or name of a family member is ever stored.
    let cols: i64 = s
        .client
        .query_one(
            "SELECT count(*) FROM information_schema.columns WHERE table_schema = current_schema()
             AND table_name = 'house_transaction' AND column_name ~ '(location|address|spouse_name|child_name)'",
            &[],
        )
        .unwrap()
        .get(0);
    assert_eq!(cols, 0);
    s.finish();
}
