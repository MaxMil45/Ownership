//! Members of Congress and their committees, from the `congress-legislators`
//! data set (the @unitedstates project, public domain, CC0):
//! `legislators-current.json`, `legislators-historical.json`,
//! `committees-current.json`, `committee-membership-current.json`.
//!
//! Only what the project needs is read (an allowlist): the Bioguide id,
//! names, and each term's chamber, dates, state, district and party; the
//! committees' ids, names and chamber; who sits on which committee. Not read:
//! birthdays, gender, office addresses and phone numbers, and the FEC and
//! OpenSecrets ids (no donor data, `CLAUDE.md`).

use oe_core::Date;
use oe_edgar::json::{self, Json};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Chamber {
    House,
    Senate,
}

impl Chamber {
    pub fn code(self) -> &'static str {
        match self {
            Chamber::House => "rep",
            Chamber::Senate => "sen",
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Term {
    pub chamber: Chamber,
    pub start: Date,
    pub end: Date,
    /// Two letters, e.g. `MI`.
    pub state: String,
    /// House only; 0 for an at-large seat.
    pub district: Option<u32>,
    pub party: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Legislator {
    /// The Congressional Biographical Directory id, e.g. `C000127`.
    pub bioguide: String,
    pub first: String,
    pub last: String,
    pub middle: Option<String>,
    pub suffix: Option<String>,
    pub nickname: Option<String>,
    pub official_full: Option<String>,
    pub terms: Vec<Term>,
}

impl Legislator {
    /// `Maria Cantwell`: the official full name, else first and last.
    pub fn display_name(&self) -> String {
        self.official_full
            .clone()
            .unwrap_or_else(|| format!("{} {}", self.first, self.last))
    }
}

fn text(v: Option<&Json>) -> Option<String> {
    v.and_then(Json::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn date(v: Option<&Json>) -> Option<Date> {
    v.and_then(Json::as_str).and_then(|s| s.parse().ok())
}

/// Reads one `legislators-*.json` file. A person without a Bioguide id or a
/// usable name, or a term without dates, chamber or state, is counted in the
/// second value and left out.
pub fn parse_legislators(text_json: &str) -> Result<(Vec<Legislator>, usize), String> {
    let root = json::parse(text_json).map_err(|e| e.to_string())?;
    let people = root.as_array().ok_or("expected a JSON array of people")?;
    let (mut out, mut skipped) = (Vec::new(), 0usize);
    for p in people {
        let name = p.get("name");
        let (Some(bioguide), Some(first), Some(last)) = (
            text(p.get("id").and_then(|i| i.get("bioguide"))),
            text(name.and_then(|n| n.get("first"))),
            text(name.and_then(|n| n.get("last"))),
        ) else {
            skipped += 1;
            continue;
        };
        let mut terms = Vec::new();
        for t in p.get("terms").and_then(Json::as_array).unwrap_or(&[]) {
            let chamber = match t.get("type").and_then(Json::as_str) {
                Some("rep") => Chamber::House,
                Some("sen") => Chamber::Senate,
                _ => {
                    skipped += 1;
                    continue;
                }
            };
            let (Some(start), Some(end), Some(state)) = (
                date(t.get("start")),
                date(t.get("end")),
                text(t.get("state")),
            ) else {
                skipped += 1;
                continue;
            };
            terms.push(Term {
                chamber,
                start,
                end,
                state,
                district: t
                    .get("district")
                    .and_then(Json::as_f64)
                    .filter(|d| *d >= 0.0 && d.fract() == 0.0)
                    .map(|d| d as u32),
                party: text(t.get("party")),
            });
        }
        out.push(Legislator {
            bioguide,
            first,
            last,
            middle: text(name.and_then(|n| n.get("middle"))),
            suffix: text(name.and_then(|n| n.get("suffix"))),
            nickname: text(name.and_then(|n| n.get("nickname"))),
            official_full: text(name.and_then(|n| n.get("official_full"))),
            terms,
        });
    }
    Ok((out, skipped))
}

#[derive(Debug, Clone, PartialEq)]
pub struct Committee {
    /// `HSAG`, or for a subcommittee the parent's id and its own (`HSAG15`).
    pub id: String,
    pub parent: Option<String>,
    /// `house`, `senate` or `joint`.
    pub chamber: String,
    pub name: String,
}

/// Reads `committees-current.json`: committees and their subcommittees.
pub fn parse_committees(text_json: &str) -> Result<Vec<Committee>, String> {
    let root = json::parse(text_json).map_err(|e| e.to_string())?;
    let list = root
        .as_array()
        .ok_or("expected a JSON array of committees")?;
    let mut out = Vec::new();
    for c in list {
        let (Some(id), Some(name)) = (text(c.get("thomas_id")), text(c.get("name"))) else {
            continue;
        };
        let chamber = text(c.get("type")).unwrap_or_default();
        for s in c
            .get("subcommittees")
            .and_then(Json::as_array)
            .unwrap_or(&[])
        {
            if let (Some(sid), Some(sname)) = (text(s.get("thomas_id")), text(s.get("name"))) {
                out.push(Committee {
                    id: format!("{id}{sid}"),
                    parent: Some(id.clone()),
                    chamber: chamber.clone(),
                    name: sname,
                });
            }
        }
        out.push(Committee {
            id,
            parent: None,
            chamber,
            name,
        });
    }
    Ok(out)
}

#[derive(Debug, Clone, PartialEq)]
pub struct Membership {
    pub committee: String,
    pub bioguide: String,
    pub rank: Option<u32>,
    /// `Chairman`, `Ranking Member`, ... when given.
    pub title: Option<String>,
    /// `majority` or `minority`.
    pub side: Option<String>,
}

/// Reads `committee-membership-current.json`: an object from committee id to
/// its members.
pub fn parse_memberships(text_json: &str) -> Result<Vec<Membership>, String> {
    let root = json::parse(text_json).map_err(|e| e.to_string())?;
    let map = root
        .as_object()
        .ok_or("expected a JSON object of committees")?;
    let mut out = Vec::new();
    for (committee, members) in map {
        for m in members.as_array().unwrap_or(&[]) {
            let Some(bioguide) = text(m.get("bioguide")) else {
                continue;
            };
            out.push(Membership {
                committee: committee.clone(),
                bioguide,
                rank: m
                    .get("rank")
                    .and_then(Json::as_f64)
                    .filter(|r| *r >= 0.0)
                    .map(|r| r as u32),
                title: text(m.get("title")),
                side: text(m.get("party")),
            });
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    // Invented people in the data set's layout.
    const PEOPLE: &str = r#"[
      {"id": {"bioguide": "Z000001", "fec": ["H0ZZ00001"], "opensecrets": "N0"},
       "name": {"first": "Alex", "middle": "B.", "last": "Example", "official_full": "Alex B. Example"},
       "bio": {"birthday": "1960-01-01", "gender": "F"},
       "terms": [
         {"type": "rep", "start": "2013-01-03", "end": "2015-01-03", "state": "ZZ", "district": 0, "party": "Independent",
          "address": "1 Office Building", "phone": "000"},
         {"type": "sen", "start": "2015-01-06", "end": "2021-01-03", "state": "ZZ", "party": "Independent"}
       ]},
      {"id": {"bioguide": "Z000002"}, "name": {"first": "Sam", "last": "Tester", "suffix": "Jr."},
       "terms": [{"type": "rep", "start": "bad", "end": "2015-01-03", "state": "ZY", "district": 4}]},
      {"id": {}, "name": {"first": "No", "last": "Id"}, "terms": []}
    ]"#;

    #[test]
    fn reads_members_and_their_terms_and_nothing_else() {
        let (people, skipped) = parse_legislators(PEOPLE).unwrap();
        assert_eq!(people.len(), 2);
        assert_eq!(
            skipped, 2,
            "one person without an id, one term without a date"
        );
        let a = &people[0];
        assert_eq!(a.bioguide, "Z000001");
        assert_eq!(a.display_name(), "Alex B. Example");
        assert_eq!(a.middle.as_deref(), Some("B."));
        assert_eq!(a.terms.len(), 2);
        assert_eq!(a.terms[0].chamber, Chamber::House);
        assert_eq!(a.terms[0].district, Some(0), "at large");
        assert_eq!(a.terms[1].chamber.code(), "sen");
        assert_eq!(a.terms[1].district, None);
        assert_eq!(people[1].display_name(), "Sam Tester");
        assert!(people[1].terms.is_empty());
        // The struct has no field for birthdays, addresses or FEC ids.
        let debug = format!("{a:?}");
        assert!(!debug.contains("1960") && !debug.contains("H0ZZ") && !debug.contains("Office"));
    }

    #[test]
    fn reads_committees_subcommittees_and_members() {
        let committees = parse_committees(
            r#"[{"type": "house", "name": "Committee on Examples", "thomas_id": "HSZZ",
                 "jurisdiction": "Examples.", "subcommittees": [{"name": "Small Examples", "thomas_id": "15"}]}]"#,
        )
        .unwrap();
        let ids: Vec<&str> = committees.iter().map(|c| c.id.as_str()).collect();
        assert_eq!(ids, ["HSZZ15", "HSZZ"]);
        assert_eq!(committees[0].parent.as_deref(), Some("HSZZ"));
        assert_eq!(committees[1].chamber, "house");

        let members = parse_memberships(
            r#"{"HSZZ": [{"name": "Alex B. Example", "party": "majority", "rank": 1, "title": "Chair", "bioguide": "Z000001"},
                         {"name": "No Id", "party": "minority", "rank": 1}]}"#,
        )
        .unwrap();
        assert_eq!(members.len(), 1);
        assert_eq!(members[0].title.as_deref(), Some("Chair"));
        assert_eq!(members[0].side.as_deref(), Some("majority"));
        assert!(parse_legislators("{}").is_err());
    }
}
