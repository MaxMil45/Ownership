//! Members of Congress, their committees and the House reports they filed
//! (migration `0006_officials`, ADR 0010). Plain inserts; a repeat load
//! writes nothing. The reports' PDFs are not stored here, only where they are
//! and their fingerprints.

use crate::db::{check_schema, StoreError};
use oe_officials::{Committee, Legislator, Membership};
use postgres::Client;

/// What a member load wrote.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct MembersReport {
    pub officials_new: u64,
    pub terms_new: u64,
    pub committees_new: u64,
    pub memberships_new: u64,
    /// Committee members the data set names but whose Bioguide id is not
    /// among the people loaded.
    pub memberships_unknown: u64,
}

/// Loads people and their terms, committees, and the committee membership as
/// of `taken_on` (`YYYY-MM-DD`), in one transaction.
pub fn load_members(
    client: &mut Client,
    people: &[Legislator],
    committees: &[Committee],
    memberships: &[Membership],
    taken_on: &str,
) -> Result<MembersReport, StoreError> {
    check_schema(client)?;
    let mut tx = client.transaction()?;
    let mut r = MembersReport::default();
    let mut known = std::collections::HashSet::new();
    for p in people {
        known.insert(p.bioguide.as_str());
        r.officials_new += tx.execute(
            "INSERT INTO official (bioguide_id, first_name, last_name, middle_name, suffix, nickname, official_full)
             VALUES ($1, $2, $3, $4, $5, $6, $7) ON CONFLICT DO NOTHING",
            &[&p.bioguide, &p.first, &p.last, &p.middle, &p.suffix, &p.nickname, &p.official_full],
        )?;
        for t in &p.terms {
            r.terms_new += tx.execute(
                "INSERT INTO official_term (bioguide_id, chamber, start_on, end_on, state, district, party)
                 VALUES ($1, $2, $3::text::date, $4::text::date, $5, $6, $7) ON CONFLICT DO NOTHING",
                &[
                    &p.bioguide,
                    &t.chamber.code(),
                    &t.start.to_string(),
                    &t.end.to_string(),
                    &t.state,
                    &t.district.map(|d| d as i32),
                    &t.party,
                ],
            )?;
        }
    }
    for c in committees {
        r.committees_new += tx.execute(
            "INSERT INTO committee (committee_id, parent_id, chamber, name) VALUES ($1, $2, $3, $4)
             ON CONFLICT DO NOTHING",
            &[&c.id, &c.parent, &c.chamber, &c.name],
        )?;
    }
    for m in memberships {
        if !known.contains(m.bioguide.as_str()) {
            r.memberships_unknown += 1;
            continue;
        }
        r.memberships_new += tx.execute(
            "INSERT INTO committee_membership (taken_on, committee_id, bioguide_id, rank, title, side)
             VALUES ($1::text::date, $2, $3, $4, $5, $6) ON CONFLICT DO NOTHING",
            &[
                &taken_on,
                &m.committee,
                &m.bioguide,
                &m.rank.map(|x| x as i32),
                &m.title,
                &m.side,
            ],
        )?;
    }
    tx.commit()?;
    Ok(r)
}

/// One House report ready for the ledger.
#[derive(Debug, Clone, PartialEq)]
pub struct HouseReportRow {
    pub doc_id: String,
    pub year: i32,
    pub filing_type: String,
    pub prefix: String,
    pub last: String,
    pub first: String,
    pub suffix: String,
    pub state_district: String,
    /// `YYYY-MM-DD`.
    pub filed_on: Option<String>,
    pub scanned: bool,
    pub source_url: String,
    /// When the PDF was first saved, from its `.meta.txt` (`...Z`).
    pub first_seen_at: Option<String>,
    pub pdf_sha256: Option<String>,
    /// The member (Bioguide id, basis) or why the report is held back.
    pub link: Result<(String, String), String>,
}

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct HouseReportsReport {
    pub reports_new: u64,
    pub links_new: u64,
    pub held_back_new: u64,
}

/// Loads reports and their links in one transaction. Load the members first:
/// a link names an `official`.
pub fn load_house_reports(
    client: &mut Client,
    rows: &[HouseReportRow],
) -> Result<HouseReportsReport, StoreError> {
    check_schema(client)?;
    let mut tx = client.transaction()?;
    let mut r = HouseReportsReport::default();
    for x in rows {
        r.reports_new += tx.execute(
            "INSERT INTO house_report (doc_id, year, filing_type, prefix, last_name, first_name, suffix,
                 state_district, filed_on, scanned, source_url, first_seen_at, pdf_sha256)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9::text::date, $10, $11, $12::text::timestamptz, $13)
             ON CONFLICT DO NOTHING",
            &[
                &x.doc_id,
                &x.year,
                &x.filing_type,
                &x.prefix,
                &x.last,
                &x.first,
                &x.suffix,
                &x.state_district,
                &x.filed_on,
                &x.scanned,
                &x.source_url,
                &x.first_seen_at,
                &x.pdf_sha256,
            ],
        )?;
        let (bioguide, basis, held): (Option<&str>, Option<&str>, Option<&str>) = match &x.link {
            Ok((b, why)) => (Some(b), Some(why), None),
            Err(reason) => (None, None, Some(reason)),
        };
        let n = tx.execute(
            "INSERT INTO house_report_link (doc_id, bioguide_id, basis, held_back) VALUES ($1, $2, $3, $4)
             ON CONFLICT DO NOTHING",
            &[&x.doc_id, &bioguide, &basis, &held],
        )?;
        if held.is_some() {
            r.held_back_new += n;
        } else {
            r.links_new += n;
        }
    }
    tx.commit()?;
    Ok(r)
}

/// A member found by name, with what the ledger holds about them.
#[derive(Debug, Clone, PartialEq)]
pub struct OfficialSummary {
    pub bioguide: String,
    pub name: String,
    /// `rep IL-04 2013-01-03..2019-01-03 Democrat`, newest first.
    pub terms: Vec<String>,
    pub reports: i64,
    pub scanned_reports: i64,
    pub first_filed: Option<String>,
    pub last_filed: Option<String>,
    /// The latest reports: filing date, document id, scanned, URL.
    pub recent: Vec<(String, String, bool, String)>,
    /// Committees in the latest membership snapshot: name, title, snapshot date.
    pub committees: Vec<(String, Option<String>, String)>,
}

/// Every official whose name holds all the words of `query` (accents and
/// punctuation ignored), with House report counts; members with reports
/// first. At most `limit`.
pub fn find_officials(
    client: &mut Client,
    query: &str,
    limit: usize,
) -> Result<Vec<OfficialSummary>, StoreError> {
    check_schema(client)?;
    let words = oe_officials::link::surname_words(query);
    if words.is_empty() {
        return Ok(Vec::new());
    }
    let people = client.query(
        "SELECT o.bioguide_id,
                coalesce(o.official_full, o.first_name || ' ' || o.last_name),
                concat_ws(' ', o.first_name, o.middle_name, o.last_name, o.nickname, o.official_full),
                (SELECT count(*) FROM house_report_link l WHERE l.bioguide_id = o.bioguide_id)
         FROM official o",
        &[],
    )?;
    let mut hits: Vec<(String, String, i64)> = people
        .iter()
        .filter(|row| {
            let names = oe_officials::link::surname_words(row.get::<_, &str>(2));
            words
                .iter()
                .all(|w| names.iter().any(|n| n.starts_with(w.as_str())))
        })
        .map(|row| (row.get(0), row.get(1), row.get(3)))
        .collect();
    hits.sort_by(|a, b| b.2.cmp(&a.2).then(a.1.cmp(&b.1)));
    hits.truncate(limit);

    let mut out = Vec::new();
    for (bioguide, name, reports) in hits {
        out.push(summary(client, bioguide, name, reports)?);
    }
    Ok(out)
}

/// One member by Bioguide id, or `None` if the ledger does not have them.
pub fn official_by_id(
    client: &mut Client,
    bioguide: &str,
) -> Result<Option<OfficialSummary>, StoreError> {
    let Some(row) = client.query_opt(
        "SELECT o.bioguide_id, coalesce(o.official_full, o.first_name || ' ' || o.last_name),
                (SELECT count(*) FROM house_report_link l WHERE l.bioguide_id = o.bioguide_id)
         FROM official o WHERE o.bioguide_id = $1",
        &[&bioguide],
    )?
    else {
        return Ok(None);
    };
    Ok(Some(summary(client, row.get(0), row.get(1), row.get(2))?))
}

fn summary(
    client: &mut Client,
    bioguide: String,
    name: String,
    reports: i64,
) -> Result<OfficialSummary, StoreError> {
    let terms = client
        .query(
            "SELECT chamber || ' ' || state || coalesce('-' || lpad(district::text, 2, '0'), '') || ' '
                    || start_on || '..' || end_on || coalesce(' ' || party, '')
             FROM official_term WHERE bioguide_id = $1 ORDER BY start_on DESC",
            &[&bioguide],
        )?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let stats = client.query_one(
        "SELECT count(*) FILTER (WHERE r.scanned), min(r.filed_on)::text, max(r.filed_on)::text
         FROM house_report r JOIN house_report_link l USING (doc_id) WHERE l.bioguide_id = $1",
        &[&bioguide],
    )?;
    let recent = client
        .query(
            "SELECT coalesce(r.filed_on::text, '?'), r.doc_id, r.scanned, r.source_url
             FROM house_report r JOIN house_report_link l USING (doc_id)
             WHERE l.bioguide_id = $1 ORDER BY r.filed_on DESC NULLS LAST, r.doc_id DESC LIMIT 5",
            &[&bioguide],
        )?
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2), r.get(3)))
        .collect();
    let committees = client
        .query(
            "SELECT c.name, m.title, m.taken_on::text
             FROM committee_membership m JOIN committee c USING (committee_id)
             WHERE m.bioguide_id = $1
               AND m.taken_on = (SELECT max(taken_on) FROM committee_membership)
             ORDER BY c.parent_id NULLS FIRST, c.name",
            &[&bioguide],
        )?
        .iter()
        .map(|r| (r.get(0), r.get(1), r.get(2)))
        .collect();
    Ok(OfficialSummary {
        bioguide,
        name,
        terms,
        reports,
        scanned_reports: stats.get(0),
        first_filed: stats.get(1),
        last_filed: stats.get(2),
        recent,
        committees,
    })
}
