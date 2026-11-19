//! Tests against a real PostgreSQL server. They are `#[ignore]`d so the normal
//! test run needs no database. To run them, point `OE_TEST_DATABASE_URL` at a
//! scratch database (each test makes its own schema and drops it afterwards, so
//! nothing else in the database is touched):
//!
//! ```powershell
//! $env:OE_TEST_DATABASE_URL = "postgres://user:password@localhost:5432/ownership_test"
//! cargo test -p oe-store --test postgres -- --ignored --test-threads=1
//! ```

use oe_store::{load_batch, migrate, prepare, status, Client, Prepared, Sidecar};
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
    assert_eq!(migrate(&mut s.client).unwrap(), ["0001_init"]);
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
    let expected_rows: u64 = batch.iter().map(|p| p.transactions.len() as u64).sum();
    assert_eq!(expected_rows, 67);

    let first = load_batch(&mut s.client, &batch).unwrap();
    assert_eq!((first.filings_new, first.filings_existing), (24, 0));
    assert_eq!((first.rows_new, first.rows_existing), (expected_rows, 0));
    assert_eq!(first.held_back_amendment_rows, 17);
    assert!(first.changed_source.is_empty());

    let again = load_batch(&mut s.client, &batch).unwrap();
    assert_eq!((again.filings_new, again.filings_existing), (0, 24));
    assert_eq!((again.rows_new, again.rows_existing), (0, expected_rows));

    let st = status(&mut s.client).unwrap();
    assert_eq!(st.filings, 24);
    assert_eq!(st.amendment_filings, 5);
    assert_eq!(st.transactions as u64, expected_rows);
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
