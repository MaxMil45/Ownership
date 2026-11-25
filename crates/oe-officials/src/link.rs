//! Which member of Congress filed a House report: the index gives a name and
//! a state-district (`MI04`), the member data a Bioguide id with terms. A
//! report is linked only when exactly one member fits; otherwise it is held
//! back with the reason, never guessed (ADR 0010, 3.4).

use crate::legislators::{Chamber, Legislator};
use crate::HouseIndexRow;

/// Members file a last report after leaving office (the trades of their final
/// weeks): a term counts up to this many days after it ended.
pub const DAYS_AFTER_TERM: i32 = 180;

#[derive(Debug, Clone, PartialEq)]
pub enum Link<'a> {
    Linked {
        member: &'a Legislator,
        /// How: `seat and surname`, `state and surname`, ... (stored).
        basis: &'static str,
    },
    HeldBack {
        reason: String,
    },
}

/// The index writes "Gutierrez" where the member data has "Gutiérrez".
use oe_core::text::fold_accent as fold;

/// Titles and degrees that some filers add to their surname ("Dunn MD, FACS").
const NOT_A_NAME: &[&str] = &[
    "MD", "DDS", "DMD", "DO", "PHD", "FACS", "JR", "SR", "II", "III", "IV",
];

/// A surname's words, letters only, upper case, accents folded, with
/// everything after a comma and any trailing degrees or suffixes left out:
/// `Dunn, MD, FACS` -> `[DUNN]`, `McMorris Rodgers` -> `[MCMORRIS, RODGERS]`.
pub fn surname_words(s: &str) -> Vec<String> {
    let before_comma = s.split(',').next().unwrap_or("");
    let mut words: Vec<String> = before_comma
        .split(|c: char| c.is_whitespace() || c == '-')
        .map(|w| {
            w.chars()
                .map(fold)
                .filter(|c| c.is_alphabetic())
                .flat_map(char::to_uppercase)
                .collect::<String>()
        })
        .filter(|w| !w.is_empty())
        .collect();
    while words.len() > 1
        && words
            .last()
            .is_some_and(|w| NOT_A_NAME.contains(&w.as_str()))
    {
        words.pop();
    }
    words
}

/// Letters only, upper case, accents folded: `O'Rourke` = `OROURKE`,
/// `Gutiérrez` = `GUTIERREZ`, `Wasserman Schultz` = `WASSERMANSCHULTZ`.
pub fn name_key(s: &str) -> String {
    surname_words(s).concat()
}

fn first_word_key(s: &str) -> String {
    name_key(s.split_whitespace().next().unwrap_or(""))
}

/// `MI04` -> (`MI`, 4); `AK00` -> (`AK`, 0).
pub fn parse_seat(s: &str) -> Option<(String, u32)> {
    let s = s.trim();
    if s.len() != 4 || !s.is_ascii() {
        return None;
    }
    let (state, district) = s.split_at(2);
    Some((state.to_ascii_uppercase(), district.parse().ok()?))
}

pub fn link_report<'a>(row: &HouseIndexRow, members: &'a [Legislator]) -> Link<'a> {
    let held = |reason: String| Link::HeldBack { reason };
    let Some(filed) = row.filing_date else {
        return held("no filing date in the index".into());
    };
    let Some((state, district)) = parse_seat(&row.state_district) else {
        return held(format!(
            "unreadable state-district {:?}",
            row.state_district
        ));
    };
    let surname = name_key(&row.last);
    if surname.is_empty() {
        return held("no surname in the index".into());
    }
    // House terms in that state covering the filing date.
    let serving = |m: &&'a Legislator, seat: bool| {
        m.terms.iter().any(|t| {
            t.chamber == Chamber::House
                && t.state == state
                && (!seat || t.district == Some(district))
                && t.start <= filed
                && filed <= t.end.add_days(DAYS_AFTER_TERM)
        })
    };
    let same_name = |m: &&'a Legislator| name_key(&m.last) == surname;

    let at_seat: Vec<&Legislator> = members.iter().filter(|m| serving(m, true)).collect();
    let by_seat: Vec<&Legislator> = at_seat.iter().copied().filter(same_name).collect();
    if let [one] = by_seat[..] {
        return Link::Linked {
            member: one,
            basis: "seat and surname",
        };
    }
    // Redistricting or a district written differently: the state is enough
    // when only one member of that surname served it.
    let by_state: Vec<&Legislator> = members
        .iter()
        .filter(|m| serving(m, false))
        .filter(same_name)
        .collect();
    let first = first_word_key(&row.first);
    let pick = |list: &[&'a Legislator]| -> Option<&'a Legislator> {
        let by_first: Vec<&Legislator> = list
            .iter()
            .copied()
            .filter(|m| {
                !first.is_empty()
                    && (first_word_key(&m.first) == first
                        || m.nickname.as_deref().map(first_word_key) == Some(first.clone()))
            })
            .collect();
        match by_first[..] {
            [one] => Some(one),
            _ => None,
        }
    };
    match (by_seat.len(), by_state.len()) {
        (0, 1) => Link::Linked {
            member: by_state[0],
            basis: "state and surname",
        },
        (n, _) if n > 1 => match pick(&by_seat) {
            Some(m) => Link::Linked {
                member: m,
                basis: "seat, surname and first name",
            },
            None => held(format!(
                "{n} members named {} at {}",
                row.last, row.state_district
            )),
        },
        (0, n) if n > 1 => match pick(&by_state) {
            Some(m) => Link::Linked {
                member: m,
                basis: "state, surname and first name",
            },
            None => held(format!("{n} members named {} in {state}", row.last)),
        },
        // Part of a double surname: "Rodgers" for McMorris Rodgers, "Hinson
        // Arenholz" for Hinson. Only at the seat, and only when exactly one
        // member serving it shares a surname word.
        (0, 0) if !shared_word(&row.last, &at_seat).is_empty() => {
            match shared_word(&row.last, &at_seat)[..] {
                [one] => Link::Linked {
                    member: one,
                    basis: "seat and part of the surname",
                },
                _ => held(format!(
                    "several members at {} share a surname word with {}",
                    row.state_district, row.last
                )),
            }
        }
        _ => match at_seat[..] {
            [one] => held(format!(
                "the member at {} then was {}, not {}",
                row.state_district,
                one.display_name(),
                row.last
            )),
            _ => held(format!(
                "no House member named {} served {} on {filed}",
                row.last, row.state_district
            )),
        },
    }
}

/// The members in `list` whose surname shares a word with `surname`.
fn shared_word<'a>(surname: &str, list: &[&'a Legislator]) -> Vec<&'a Legislator> {
    let words = surname_words(surname);
    list.iter()
        .copied()
        .filter(|m| surname_words(&m.last).iter().any(|w| words.contains(w)))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::legislators::Term;
    use oe_core::Date;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    fn member(id: &str, first: &str, last: &str, terms: &[(&str, u32, (i32, i32))]) -> Legislator {
        Legislator {
            bioguide: id.into(),
            first: first.into(),
            last: last.into(),
            middle: None,
            suffix: None,
            nickname: None,
            official_full: None,
            terms: terms
                .iter()
                .map(|(state, district, (from, to))| Term {
                    chamber: Chamber::House,
                    start: d(*from, 1, 3),
                    end: d(*to, 1, 3),
                    state: state.to_string(),
                    district: Some(*district),
                    party: None,
                })
                .collect(),
        }
    }

    fn row(first: &str, last: &str, seat: &str, filed: Date) -> HouseIndexRow {
        HouseIndexRow {
            prefix: "Hon.".into(),
            last: last.into(),
            first: first.into(),
            suffix: String::new(),
            filing_type: "P".into(),
            state_district: seat.into(),
            year: filed.year(),
            filing_date: Some(filed),
            doc_id: "20000001".into(),
        }
    }

    fn linked(l: Link) -> (String, &'static str) {
        match l {
            Link::Linked { member, basis } => (member.bioguide.clone(), basis),
            Link::HeldBack { reason } => panic!("held back: {reason}"),
        }
    }

    #[test]
    fn links_by_seat_and_surname_and_after_the_term_for_a_while() {
        let people = [
            member("Z1", "Alex", "O'Example", &[("ZZ", 4, (2015, 2019))]),
            member("Z2", "Sam", "Tester", &[("ZZ", 4, (2019, 2023))]),
        ];
        // Punctuation and case do not matter.
        assert_eq!(
            linked(link_report(
                &row("Alex", "OEXAMPLE", "ZZ04", d(2017, 5, 1)),
                &people
            )),
            ("Z1".into(), "seat and surname")
        );
        // A final report four months after leaving office.
        assert_eq!(
            linked(link_report(
                &row("Alex", "O'Example", "ZZ04", d(2019, 5, 1)),
                &people
            ))
            .0,
            "Z1"
        );
        // A year after: no longer.
        assert!(matches!(
            link_report(&row("Alex", "O'Example", "ZZ04", d(2020, 2, 1)), &people),
            Link::HeldBack { .. }
        ));
    }

    /// Real data, outside the repository: `OE_OFFICIALS_DIR=<data/officials>
    /// cargo test -p oe-officials --release -- --ignored --nocapture real_reports`
    #[test]
    #[ignore = "needs data/officials, run by hand"]
    fn real_reports_link_to_members() {
        let root = std::path::PathBuf::from(
            std::env::var("OE_OFFICIALS_DIR").expect("set OE_OFFICIALS_DIR"),
        );
        let mut people = Vec::new();
        for f in ["legislators-current.json", "legislators-historical.json"] {
            let text = std::fs::read_to_string(root.join("legislators").join(f)).unwrap();
            people.extend(crate::parse_legislators(&text).unwrap().0);
        }
        let (mut linked, mut held) = (0usize, Vec::new());
        let mut bases: std::collections::BTreeMap<&str, usize> = Default::default();
        let mut filers = std::collections::BTreeSet::new();
        for year in 2013..=2026 {
            let p = root
                .join("house")
                .join(year.to_string())
                .join(format!("{year}FD.txt"));
            let Ok(text) = std::fs::read_to_string(&p) else {
                continue;
            };
            for r in crate::parse_house_index(&text)
                .unwrap()
                .0
                .iter()
                .filter(|r| r.is_ptr())
            {
                match link_report(r, &people) {
                    Link::Linked { member, basis } => {
                        if basis != "seat and surname" {
                            println!(
                                "  {basis}: {} {} -> {} {}",
                                r.first,
                                r.last,
                                member.bioguide,
                                member.display_name()
                            );
                        }
                        linked += 1;
                        *bases.entry(basis).or_default() += 1;
                        filers.insert(member.bioguide.clone());
                    }
                    Link::HeldBack { reason } => {
                        held.push(format!("{year} {} {}: {reason}", r.last, r.state_district))
                    }
                }
            }
        }
        println!(
            "{linked} linked ({} members), {} held back; {bases:?}",
            filers.len(),
            held.len()
        );
        for h in held.iter().take(40) {
            println!("  {h}");
        }
    }

    #[test]
    fn falls_back_to_the_state_and_holds_back_what_is_not_clear() {
        let people = [
            member("Z1", "Alex", "Example", &[("ZZ", 7, (2015, 2025))]),
            member("Z2", "Sam", "Smith", &[("ZZ", 1, (2015, 2025))]),
            member("Z3", "Jo", "Smith", &[("ZZ", 2, (2015, 2025))]),
            member("Z4", "Pat", "Other", &[("ZZ", 3, (2015, 2025))]),
        ];
        // Redistricted: written as ZZ05, served as ZZ07.
        assert_eq!(
            linked(link_report(
                &row("Alex", "Example", "ZZ05", d(2020, 1, 1)),
                &people
            )),
            ("Z1".into(), "state and surname")
        );
        // Two Smiths in the state: the first name decides.
        assert_eq!(
            linked(link_report(
                &row("Jo A.", "Smith", "ZZ09", d(2020, 1, 1)),
                &people
            )),
            ("Z3".into(), "state, surname and first name")
        );
        let held = |r: HouseIndexRow| match link_report(&r, &people) {
            Link::HeldBack { reason } => reason,
            Link::Linked { member, .. } => panic!("linked {}", member.bioguide),
        };
        assert_eq!(
            held(row("Chris", "Smith", "ZZ09", d(2020, 1, 1))),
            "2 members named Smith in ZZ"
        );
        // A seat whose member has another name (a name change, or a typo).
        assert!(held(row("Pat", "Changed", "ZZ03", d(2020, 1, 1)))
            .contains("the member at ZZ03 then was Pat Other"));
        assert!(held(row("A", "Nobody", "ZZ08", d(2020, 1, 1)))
            .starts_with("no House member named Nobody"));
        let mut undated = row("Alex", "Example", "ZZ07", d(2020, 1, 1));
        undated.filing_date = None;
        assert_eq!(held(undated), "no filing date in the index");
        assert_eq!(parse_seat("AK00"), Some(("AK".into(), 0)));
    }

    #[test]
    fn accents_degrees_and_double_surnames_as_real_filers_write_them() {
        let people = [
            member("Z1", "Luis", "Gutiérrez", &[("ZZ", 4, (2013, 2019))]),
            member("Z2", "Neal", "Dunn", &[("ZZ", 2, (2017, 2027))]),
            member(
                "Z3",
                "Cathy",
                "McMorris Rodgers",
                &[("ZZ", 5, (2013, 2025))],
            ),
            member("Z4", "Ashley", "Hinson", &[("ZZ", 1, (2021, 2027))]),
            // Left office, still within the 180 days: two members at ZZ01.
            member("Z5", "Abby", "Previous", &[("ZZ", 1, (2019, 2021))]),
        ];
        let id = |first: &str, last: &str, seat: &str| match link_report(
            &row(first, last, seat, d(2021, 4, 5)),
            &people,
        ) {
            Link::Linked { member, basis } => format!("{} {basis}", member.bioguide),
            Link::HeldBack { reason } => format!("held: {reason}"),
        };
        assert_eq!(
            id("Luis V.", "Gutierrez", "ZZ04"),
            "held: no House member named Gutierrez served ZZ04 on 2021-04-05",
            "term over by then"
        );
        assert_eq!(
            match link_report(&row("Luis", "Gutierrez", "ZZ04", d(2016, 1, 1)), &people) {
                Link::Linked { member, .. } => member.bioguide.clone(),
                _ => "held".into(),
            },
            "Z1"
        );
        assert_eq!(
            id("Neal P.", "Dunn MD, FACS", "ZZ02"),
            "Z2 seat and surname"
        );
        assert_eq!(
            id("Neal P.", "Dunn, MD, FACS", "ZZ02"),
            "Z2 seat and surname"
        );
        assert_eq!(
            id("Cathy", "Rodgers", "ZZ05"),
            "Z3 seat and part of the surname"
        );
        assert_eq!(
            id("Ashley", "Hinson Arenholz", "ZZ01"),
            "Z4 seat and part of the surname"
        );
        assert_eq!(surname_words("Dunn, MD, FACS"), ["DUNN"]);
        assert_eq!(surname_words("Barragán"), ["BARRAGAN"]);
        assert_eq!(name_key("O'Rourke"), "OROURKE");
        assert_eq!(parse_seat("A1"), None);
    }
}
