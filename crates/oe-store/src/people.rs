//! The name index and the lookup behind `oe lookup` (ADR 0010, 2): who is
//! this name in the ledger? Candidates, each with its identifier and its
//! public roles; never merged, never guessed.
//!
//! The index (`people_candidate`, `people_token`, `people_name`) is derived
//! from the ledger by [`build_people_index`] and is not part of it: each
//! build empties and refills it in one transaction.
//!
//! Who is indexed (principle 4, public roles only):
//! * Forms 3/4/5: every reporting owner (`cik:`);
//! * Schedule 13D/G: reporting persons with a CIK (`cik:`), and entities
//!   without one (`holder:` + their name tokens); individuals without a CIK
//!   (members of a group filing) are not;
//! * contested proxies: every filer (`cik:`);
//! * members of Congress (`bioguide:`).
//!
//! Form 144 sellers and Item 408 names are not indexed yet.

use crate::db::StoreError;
use oe_core::text::{name_alternatives, name_tokens, tokens_match_nick};
use postgres::Client;
use std::collections::{BTreeSet, HashMap};
use std::io::Write;

/// A one-word query matching more candidates than this gets a count only.
pub const ONE_WORD_LIMIT: usize = 10;

#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PeopleIndexReport {
    pub candidates: u64,
    pub tokens: u64,
    pub names: u64,
    pub insiders: u64,
    pub holders: u64,
    pub contest_filers: u64,
    pub officials: u64,
}

#[derive(Default)]
struct Building {
    kinds: BTreeSet<&'static str>,
    /// name -> (filings under it, last date)
    names: HashMap<String, (i64, String)>,
    tokens: Vec<String>,
    filings: i64,
    first: Option<String>,
    last: Option<String>,
    /// Officials: the name to show regardless of filings.
    fixed_name: Option<String>,
}

impl Building {
    fn add(
        &mut self,
        kind: &'static str,
        name: &str,
        filings: i64,
        first: Option<String>,
        last: Option<String>,
    ) {
        self.kinds.insert(kind);
        let name = name.trim();
        if !name.is_empty() {
            let e = self
                .names
                .entry(name.to_string())
                .or_insert((0, String::new()));
            e.0 += filings;
            if let Some(l) = &last {
                if *l > e.1 {
                    e.1 = l.clone();
                }
            }
            for t in name_tokens(name) {
                if !self.tokens.contains(&t) {
                    self.tokens.push(t);
                }
            }
        }
        self.filings += filings;
        if let Some(f) = first {
            if self.first.as_ref().is_none_or(|x| f < *x) {
                self.first = Some(f);
            }
        }
        if let Some(l) = last {
            if self.last.as_ref().is_none_or(|x| l > *x) {
                self.last = Some(l);
            }
        }
    }

    /// The name under which the latest filing was made (then the most used).
    fn display_name(&self) -> Option<String> {
        if let Some(n) = &self.fixed_name {
            return Some(n.clone());
        }
        self.names
            .iter()
            .max_by(|a, b| {
                (&a.1 .1, a.1 .0, b.0)
                    .partial_cmp(&(&b.1 .1, b.1 .0, a.0))
                    .unwrap()
            })
            .map(|(n, _)| n.clone())
    }
}

const EASTERN_DAY: &str = "(f.accepted_at AT TIME ZONE 'America/New_York')::date::text";

/// Empties and rebuilds the name index from the ledger, in one transaction.
pub fn build_people_index(client: &mut Client) -> Result<PeopleIndexReport, StoreError> {
    crate::db::check_schema(client)?;
    let mut report = PeopleIndexReport::default();
    let mut all: HashMap<String, Building> = HashMap::new();

    for row in client.query(
        &format!(
            "SELECT r.owner_cik, r.owner_name, count(DISTINCT r.accession), min({EASTERN_DAY}), max({EASTERN_DAY})
             FROM filing_owner_role r JOIN filing f USING (accession)
             GROUP BY r.owner_cik, r.owner_name"
        ),
        &[],
    )? {
        let cik: i64 = row.get(0);
        report.insiders += 1;
        all.entry(format!("cik:{cik}"))
            .or_default()
            .add("insider", row.get(1), row.get(2), row.get(3), row.get(4));
    }

    for row in client.query(
        &format!(
            "SELECT p.cik, p.name, coalesce(p.person_type, ''), count(DISTINCT p.accession), min({EASTERN_DAY}), max({EASTERN_DAY})
             FROM schedule13_person p JOIN filing f USING (accession)
             GROUP BY p.cik, p.name, p.person_type"
        ),
        &[],
    )? {
        let cik: Option<i64> = row.get(0);
        let name: &str = row.get(1);
        let person_type: &str = row.get(2);
        let key = match cik {
            Some(c) => format!("cik:{c}"),
            // Individuals inside a group filing, and persons of no stated
            // type, are not searchable on their own.
            None if !person_type.is_empty() && person_type != "IN" => {
                let mut t = name_tokens(name);
                if t.is_empty() {
                    continue;
                }
                t.sort();
                format!("holder:{}", t.join(" "))
            }
            None => continue,
        };
        report.holders += 1;
        all.entry(key)
            .or_default()
            .add("holder", name, row.get(3), row.get(4), row.get(5));
    }

    for row in client.query(
        &format!(
            "SELECT c.cik, c.name, count(DISTINCT c.accession), min({EASTERN_DAY}), max({EASTERN_DAY})
             FROM contest_filer c JOIN filing f USING (accession)
             GROUP BY c.cik, c.name"
        ),
        &[],
    )? {
        let cik: i64 = row.get(0);
        report.contest_filers += 1;
        all.entry(format!("cik:{cik}"))
            .or_default()
            .add("contest", row.get(1), row.get(2), row.get(3), row.get(4));
    }

    for row in client.query(
        "SELECT o.bioguide_id,
                coalesce(o.official_full, o.first_name || ' ' || o.last_name),
                concat_ws(' ', o.first_name, o.middle_name, o.last_name, o.nickname),
                (SELECT count(*) FROM house_report_link l WHERE l.bioguide_id = o.bioguide_id),
                (SELECT min(start_on)::text FROM official_term t WHERE t.bioguide_id = o.bioguide_id),
                (SELECT max(end_on)::text FROM official_term t WHERE t.bioguide_id = o.bioguide_id)
         FROM official o",
        &[],
    )? {
        let id: String = row.get(0);
        let shown: String = row.get(1);
        report.officials += 1;
        let b = all.entry(format!("bioguide:{id}")).or_default();
        b.fixed_name = Some(shown.clone());
        b.add("official", &shown, row.get(3), row.get(4), row.get(5));
        let full: String = row.get(2);
        b.add("official", &full, 0, None, None);
    }

    let mut tx = client.transaction()?;
    tx.batch_execute("TRUNCATE people_candidate CASCADE")?;
    {
        let mut w = tx.copy_in(
            "COPY people_candidate (candidate_key, display_name, kinds, tokens, display_tokens, filings, first_on, last_on) FROM STDIN",
        )?;
        let f = crate::rows::copy_field;
        let opt = |v: &Option<String>| v.as_deref().map_or_else(|| "\\N".to_string(), f);
        for (key, b) in &all {
            let Some(display) = b.display_name() else {
                continue;
            };
            if b.tokens.is_empty() {
                continue;
            }
            let kinds: Vec<&str> = b.kinds.iter().copied().collect();
            w.write_all(
                format!(
                    "{}\t{}\t{}\t{}\t{}\t{}\t{}\t{}\n",
                    f(key),
                    f(&display),
                    kinds.join(","),
                    f(&b.tokens.join(" ")),
                    f(&name_tokens(&display).join(" ")),
                    b.filings,
                    opt(&b.first),
                    opt(&b.last),
                )
                .as_bytes(),
            )?;
            report.candidates += 1;
        }
        w.finish()?;
    }
    {
        let mut w = tx.copy_in("COPY people_token (token, candidate_key) FROM STDIN")?;
        for (key, b) in &all {
            if b.display_name().is_none() {
                continue;
            }
            for t in &b.tokens {
                w.write_all(
                    format!(
                        "{}\t{}\n",
                        crate::rows::copy_field(t),
                        crate::rows::copy_field(key)
                    )
                    .as_bytes(),
                )?;
                report.tokens += 1;
            }
        }
        w.finish()?;
    }
    {
        let mut w = tx.copy_in("COPY people_name (candidate_key, name) FROM STDIN")?;
        for (key, b) in &all {
            if b.display_name().is_none() || b.tokens.is_empty() {
                continue;
            }
            for n in b.names.keys() {
                w.write_all(
                    format!(
                        "{}\t{}\n",
                        crate::rows::copy_field(key),
                        crate::rows::copy_field(n)
                    )
                    .as_bytes(),
                )?;
                report.names += 1;
            }
        }
        w.finish()?;
    }
    tx.commit()?;
    Ok(report)
}

/// A link to the SEC filing a fact comes from, and when it became public.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Receipt {
    pub accession: String,
    pub url: String,
    /// US Eastern date the SEC accepted it.
    pub known_on: String,
    /// Bulk-loaded Form 3/4/5: the time of day is not known (ADR 0008).
    pub filing_date_only: bool,
}

/// One company an insider filed about.
#[derive(Debug, Clone, PartialEq)]
pub struct InsiderRole {
    pub issuer_cik: i64,
    pub issuer_name: String,
    pub ticker: Option<String>,
    pub director: bool,
    pub officer: bool,
    pub ten_percent_owner: bool,
    pub other: bool,
    /// Officer titles as filed, most recent first.
    pub titles: Vec<String>,
    pub first_on: String,
    pub last_on: String,
    pub filings: i64,
}

/// An open-market purchase (P) or sale (S) of non-derivative stock.
#[derive(Debug, Clone, PartialEq)]
pub struct Trade {
    pub transaction_date: String,
    pub code: String,
    pub shares: Option<f64>,
    pub price: Option<f64>,
    pub issuer_name: String,
    pub ticker: Option<String>,
    pub receipt: Receipt,
}

/// The latest Schedule 13D/G of a holder for one company.
#[derive(Debug, Clone, PartialEq)]
pub struct Holding {
    pub issuer_cik: i64,
    pub issuer_name: String,
    pub class_title: String,
    /// `SCHEDULE 13D/A`, ...
    pub submission_type: String,
    pub percent: Option<f64>,
    pub receipt: Receipt,
}

/// A contested-proxy filing the candidate made.
#[derive(Debug, Clone, PartialEq)]
pub struct ContestFiling {
    pub subject_name: String,
    pub form: String,
    pub receipt: Receipt,
}

/// Everything the ledger says about one candidate.
#[derive(Debug, Clone, PartialEq)]
pub struct PersonCard {
    /// `cik:1234`, `holder:...`, `bioguide:P000197`.
    pub key: String,
    pub display_name: String,
    /// insider, holder, contest, official.
    pub kinds: Vec<String>,
    /// Other names the candidate filed under.
    pub other_names: Vec<String>,
    pub filings: i64,
    pub first_on: Option<String>,
    pub last_on: Option<String>,
    /// Every query token equals a name token and the other way round.
    pub exact: bool,
    pub roles: Vec<InsiderRole>,
    pub trades: Vec<Trade>,
    pub holdings: Vec<Holding>,
    pub contests: Vec<ContestFiling>,
    pub official: Option<crate::officials::OfficialSummary>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Lookup {
    /// The query has no full word (only initials, or nothing).
    NeedsAName,
    /// A one-word query with many candidates: how many in all, and the
    /// first ones (exact name, then the most recent).
    Many {
        total: usize,
        cards: Vec<PersonCard>,
    },
    /// No one matches every word; these people match `word` alone (the
    /// rarest word of the query: first names are often filed differently,
    /// "Jensen Huang" is "HUANG JEN HSUN").
    Related {
        word: String,
        cards: Vec<PersonCard>,
    },
    /// Best first: exact name, then the most recent.
    Found(Vec<PersonCard>),
}

/// Candidates whose names hold every word of `q` (a nickname counts as its
/// first name, a single letter as an initial), unsorted and without details.
fn candidates(client: &mut Client, q: &[String]) -> Result<Vec<PersonCard>, StoreError> {
    let words: Vec<&String> = q.iter().filter(|t| t.chars().count() > 1).collect();
    if words.is_empty() {
        return Ok(Vec::new());
    }
    // Every alternative name of word i carries group i; a candidate needs
    // one token of each group.
    let mut tokens: Vec<String> = Vec::new();
    let mut groups: Vec<i32> = Vec::new();
    for (i, w) in words.iter().enumerate() {
        for alt in name_alternatives(w) {
            tokens.push(alt);
            groups.push(i as i32);
        }
    }
    let rows = client.query(
        "SELECT c.candidate_key, c.display_name, c.kinds, c.tokens, c.display_tokens,
                c.filings, c.first_on::text, c.last_on::text
         FROM people_candidate c
         WHERE c.candidate_key IN (
             SELECT t.candidate_key
             FROM people_token t
             JOIN unnest($1::text[], $2::int4[]) AS w(token, grp) ON t.token = w.token
             GROUP BY t.candidate_key HAVING count(DISTINCT w.grp) = $3)",
        &[&tokens, &groups, &(words.len() as i64)],
    )?;
    let mut sorted_q = q.to_vec();
    sorted_q.sort();
    Ok(rows
        .iter()
        .filter_map(|r| {
            let tokens: Vec<String> = r.get::<_, &str>(3).split(' ').map(str::to_string).collect();
            if !tokens_match_nick(q, &tokens) {
                return None;
            }
            let mut shown: Vec<String> =
                r.get::<_, &str>(4).split(' ').map(str::to_string).collect();
            shown.sort();
            Some(PersonCard {
                key: r.get(0),
                display_name: r.get(1),
                kinds: r.get::<_, &str>(2).split(',').map(str::to_string).collect(),
                other_names: Vec::new(),
                filings: r.get(5),
                first_on: r.get(6),
                last_on: r.get(7),
                exact: shown == sorted_q,
                roles: Vec::new(),
                trades: Vec::new(),
                holdings: Vec::new(),
                contests: Vec::new(),
                official: None,
            })
        })
        .collect())
}

/// Exact name first, then the most recent, then the most filings.
fn rank(found: &mut [PersonCard]) {
    found.sort_by(|a, b| {
        b.exact
            .cmp(&a.exact)
            .then_with(|| b.last_on.cmp(&a.last_on))
            .then_with(|| b.filings.cmp(&a.filings))
            .then_with(|| a.key.cmp(&b.key))
    });
}

/// The word of a query that is most likely the surname: the first word
/// before a comma ("Huang, Jensen"), else the last ("Jensen Huang").
fn surname_word(query: &str) -> Option<String> {
    let comma = query.contains(',');
    let part = if comma {
        query.split(',').next().unwrap_or("")
    } else {
        query
    };
    let words: Vec<String> = name_tokens(part)
        .into_iter()
        .filter(|t| t.chars().count() > 1)
        .collect();
    words.last().cloned()
}

/// How many of the query's other words (not `surname`) resemble a word of
/// `name`: one starts the other, at least three letters long ("JEN" and
/// "JENSEN"), or they are equal.
fn resemblance(words: &[String], surname: &str, name: &str) -> usize {
    let have = name_tokens(name);
    words
        .iter()
        .filter(|w| w.as_str() != surname)
        .filter(|w| {
            have.iter().any(|t| {
                t.as_str() != surname
                    && (t == *w
                        || (t.chars().count() >= 3
                            && w.chars().count() >= 3
                            && (t.starts_with(w.as_str()) || w.starts_with(t.as_str()))))
            })
        })
        .count()
}

/// Looks a name up in the index: every word of the query must be a word of
/// the candidate's names (a single letter matches as an initial, a nickname
/// as its first name). With no such candidate, people matching the rarest
/// single word are offered instead. At most `limit` candidates, each with
/// its details.
pub fn lookup(client: &mut Client, query: &str, limit: usize) -> Result<Lookup, StoreError> {
    crate::db::check_schema(client)?;
    let q = name_tokens(query);
    let words: Vec<String> = q
        .iter()
        .filter(|t| t.chars().count() > 1)
        .cloned()
        .collect();
    if words.is_empty() {
        return Ok(Lookup::NeedsAName);
    }
    let mut found = candidates(client, &q)?;
    if found.is_empty() && words.len() >= 2 {
        // Who is probably the surname: the last word ("Jensen Huang"), or
        // the first when there is a comma ("Huang, Jensen"); else the
        // rarest word that matches anyone.
        let surname = surname_word(query);
        let rows = client.query(
            "SELECT token, count(*) FROM people_token WHERE token = ANY($1)
             GROUP BY token ORDER BY count(*), token",
            &[&words],
        )?;
        let word: Option<String> = match surname {
            Some(w) if rows.iter().any(|r| r.get::<_, String>(0) == w) => Some(w),
            _ => rows.first().map(|r| r.get(0)),
        };
        if let Some(word) = word {
            let mut related = candidates(client, std::slice::from_ref(&word))?;
            for c in &mut related {
                c.exact = false;
            }
            rank(&mut related);
            // Those whose other words look like the ones typed come first
            // (a stable sort keeps the order above within each score).
            related.sort_by_key(|c| std::cmp::Reverse(resemblance(&words, &word, &c.display_name)));
            related.truncate(limit);
            for card in &mut related {
                fill(client, card)?;
            }
            return Ok(Lookup::Related {
                word,
                cards: related,
            });
        }
    }
    rank(&mut found);
    let total = found.len();
    found.truncate(limit);
    for card in &mut found {
        fill(client, card)?;
    }
    if q.len() == 1 && total > ONE_WORD_LIMIT {
        return Ok(Lookup::Many {
            total,
            cards: found,
        });
    }
    Ok(Lookup::Found(found))
}

fn receipt(accession: String, filer_cik: i64, known_on: String, source: &str) -> Receipt {
    Receipt {
        url: oe_edgar::filing_folder_url(filer_cik as u64, &accession.replace('-', "")),
        accession,
        known_on,
        filing_date_only: source == "bulk",
    }
}

fn fill(client: &mut Client, card: &mut PersonCard) -> Result<(), StoreError> {
    card.other_names = client
        .query(
            "SELECT name FROM people_name WHERE candidate_key = $1 AND name <> $2 ORDER BY name",
            &[&card.key, &card.display_name],
        )?
        .iter()
        .map(|r| r.get(0))
        .collect();

    if let Some(id) = card.key.strip_prefix("bioguide:") {
        card.official = crate::officials::official_by_id(client, id)?;
        return Ok(());
    }
    let has = |k: &str| card.kinds.iter().any(|x| x == k);
    let cik: Option<i64> = card.key.strip_prefix("cik:").and_then(|c| c.parse().ok());

    if let (Some(cik), true) = (cik, has("insider")) {
        card.roles = client
            .query(
                &format!(
                    "SELECT i.issuer_cik,
                            (array_agg(i.issuer_name ORDER BY f.accepted_at DESC))[1],
                            (array_agg(i.ticker ORDER BY f.accepted_at DESC) FILTER (WHERE i.ticker IS NOT NULL))[1],
                            bool_or(r.is_director), bool_or(r.is_officer),
                            bool_or(r.is_ten_percent_owner), bool_or(r.is_other),
                            array_remove(array_agg(DISTINCT r.officer_title), NULL),
                            min({EASTERN_DAY}), max({EASTERN_DAY}), count(DISTINCT r.accession)
                     FROM filing_owner_role r
                     JOIN ownership_issuer i USING (accession)
                     JOIN filing f USING (accession)
                     WHERE r.owner_cik = $1
                     GROUP BY i.issuer_cik
                     ORDER BY max(f.accepted_at) DESC
                     LIMIT 10"
                ),
                &[&cik],
            )?
            .iter()
            .map(|r| InsiderRole {
                issuer_cik: r.get(0),
                issuer_name: r.get(1),
                ticker: r.get(2),
                director: r.get(3),
                officer: r.get(4),
                ten_percent_owner: r.get(5),
                other: r.get(6),
                titles: r.get(7),
                first_on: r.get(8),
                last_on: r.get(9),
                filings: r.get(10),
            })
            .collect();
        card.trades = client
            .query(
                &format!(
                    "SELECT t.transaction_date::text, t.code, t.shares::float8, t.price_per_share::float8,
                            coalesce(i.issuer_name, ''), i.ticker,
                            f.accession, f.filer_cik, {EASTERN_DAY}, f.source
                     FROM filing_owner fo
                     JOIN insider_transaction t ON t.accession = fo.accession
                     JOIN filing f ON f.accession = t.accession
                     LEFT JOIN ownership_issuer i ON i.accession = t.accession
                     WHERE fo.owner_cik = $1 AND NOT t.is_derivative AND t.code IN ('P', 'S')
                       AND NOT EXISTS (SELECT 1 FROM insider_transaction n WHERE n.supersedes_id = t.id)
                     ORDER BY f.accepted_at DESC, t.row_in_filing
                     LIMIT 5"
                ),
                &[&cik],
            )?
            .into_iter()
            .map(|r| Trade {
                transaction_date: r.get(0),
                code: r.get(1),
                shares: r.get(2),
                price: r.get(3),
                issuer_name: r.get(4),
                ticker: r.get(5),
                receipt: receipt(r.get(6), r.get(7), r.get(8), r.get::<_, &str>(9)),
            })
            .collect();
    }

    if has("holder") {
        let names: Vec<String> = client
            .query(
                "SELECT name FROM people_name WHERE candidate_key = $1",
                &[&card.key],
            )?
            .iter()
            .map(|r| r.get(0))
            .collect();
        let sql = |filter: &str| {
            format!(
                "SELECT DISTINCT ON (s.issuer_cik)
                        s.issuer_cik, s.issuer_name, s.class_title, s.submission_type,
                        p.percent_of_class::float8, f.accession, f.filer_cik, {EASTERN_DAY}, f.source
                 FROM schedule13_person p
                 JOIN schedule13_filing s USING (accession)
                 JOIN filing f USING (accession)
                 WHERE {filter}
                 ORDER BY s.issuer_cik, f.accepted_at DESC"
            )
        };
        let rows = match cik {
            Some(c) => client.query(&sql("p.cik = $1"), &[&c])?,
            None => client.query(&sql("p.cik IS NULL AND p.name = ANY($1)"), &[&names])?,
        };
        let mut holdings: Vec<Holding> = rows
            .into_iter()
            .map(|r| Holding {
                issuer_cik: r.get(0),
                issuer_name: r.get(1),
                class_title: r.get(2),
                submission_type: r.get(3),
                percent: r.get(4),
                receipt: receipt(r.get(5), r.get(6), r.get(7), r.get::<_, &str>(8)),
            })
            .collect();
        holdings.sort_by(|a, b| b.receipt.known_on.cmp(&a.receipt.known_on));
        holdings.truncate(10);
        card.holdings = holdings;
    }

    if let (Some(cik), true) = (cik, has("contest")) {
        card.contests = client
            .query(
                &format!(
                    "SELECT cf.subject_name, cf.form, f.accession, f.filer_cik, {EASTERN_DAY}, f.source
                     FROM contest_filer c
                     JOIN contest_filing cf USING (accession)
                     JOIN filing f USING (accession)
                     WHERE c.cik = $1
                     ORDER BY f.accepted_at DESC
                     LIMIT 5"
                ),
                &[&cik],
            )?
            .into_iter()
            .map(|r| ContestFiling {
                subject_name: r.get(0),
                form: r.get(1),
                receipt: receipt(r.get(2), r.get(3), r.get(4), r.get::<_, &str>(5)),
            })
            .collect();
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn related_names_are_ranked_by_how_much_they_resemble_the_query() {
        let q = vec!["JENSEN".to_string(), "HUANG".to_string()];
        assert_eq!(super::resemblance(&q, "HUANG", "Huang Jen Hsun"), 1);
        assert_eq!(super::resemblance(&q, "HUANG", "Huang Jack Jiajia"), 0);
        assert_eq!(super::resemblance(&q, "HUANG", "Jensen Huang"), 1);
        assert_eq!(
            super::surname_word("Jensen Huang").as_deref(),
            Some("HUANG")
        );
        assert_eq!(
            super::surname_word("Huang, Jensen").as_deref(),
            Some("HUANG")
        );
    }
}
