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
            "0005_other_forms"
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
