//! Parser for SEC Schedule 13D and Schedule 13G, the reports a holder files on
//! crossing 5% of a class of a company's stock.
//!
//! * **13D** is for holders who may want to influence the company (an
//!   "activist" filing). It has a free-text Item 4, the purpose of the
//!   transaction.
//! * **13G** is the short form for passive holders: institutions (Rule
//!   13d-1(b)), passive investors under 20% (Rule 13d-1(c)) and exempt
//!   investors (Rule 13d-1(d)).
//!
//! Only the XML era (December 2024 onwards) is covered. The two forms use
//! different element names for the same things; this crate reads both into one
//! model. The `oe-form4` tokenizer strips namespace prefixes.
//!
//! ```
//! let xml = std::fs::read_to_string(concat!(
//!     env!("CARGO_MANIFEST_DIR"),
//!     "/../../fixtures/sched13/synthetic/01_new_13d_group.xml"
//! )).unwrap();
//! let s = oe_sched13::parse(&xml).unwrap();
//! assert_eq!(s.schedule, oe_sched13::Schedule::D);
//! assert!(!s.is_amendment());
//! ```

mod parse;

pub use parse::{parse, ParseError};

use oe_core::{Cik, Date};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Schedule {
    D,
    G,
}

/// One reporting person (a 13D/13G can have several, often a fund and its
/// manager, who share the same shares).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ReportingPerson {
    pub name: String,
    /// 13D only; a 13G does not list the person's CIK in the XML.
    pub cik: Option<Cik>,
    /// 13D: `a` (member of a group) or `b` (not).
    pub member_of_group: Option<String>,
    pub citizenship_or_organization: Option<String>,
    pub sole_voting: Option<f64>,
    pub shared_voting: Option<f64>,
    pub sole_dispositive: Option<f64>,
    pub shared_dispositive: Option<f64>,
    pub aggregate_shares: Option<f64>,
    /// Percent of the class, as the filer states it (0 to 100).
    pub percent_of_class: Option<f64>,
    /// `IN` (individual), `CO`, `IA` (investment adviser), `HC`, `PN`, `OO`, ...
    pub person_type: Option<String>,
    /// 13D source of funds (`OO`, `PF`, `WC`, ...).
    pub fund_type: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Schedule13 {
    /// `SCHEDULE 13D`, `SCHEDULE 13D/A`, `SCHEDULE 13G` or `SCHEDULE 13G/A`.
    pub submission_type: String,
    pub schedule: Schedule,
    pub amendment_no: Option<u32>,
    /// The accession number of the filing this amends, when it says.
    pub previous_accession: Option<String>,
    /// The CIKs of the submitting filers (`headerData/filerInfo`).
    pub filer_ciks: Vec<Cik>,
    pub issuer_cik: Cik,
    pub issuer_name: String,
    pub cusips: Vec<String>,
    pub class_title: String,
    /// 13D `dateOfEvent`, 13G `eventDateRequiresFilingThisStatement`.
    pub event_date: Option<Date>,
    /// 13D `previouslyFiledFlag`: the filer says they earlier reported this
    /// holding on a Schedule 13G.
    pub previously_filed_13g: Option<bool>,
    /// 13G: the rules the filer relies on, e.g. `Rule 13d-1(c)`.
    pub rules: Vec<String>,
    pub persons: Vec<ReportingPerson>,
    /// 13D Item 4, "Purpose of Transaction", as written.
    pub purpose_of_transaction: Option<String>,
    pub signature_date: Option<Date>,
}

impl Schedule13 {
    pub fn is_amendment(&self) -> bool {
        self.submission_type.ends_with("/A")
    }

    /// The largest percentage any one reporting person states. Joint filers
    /// usually state the same shares, so adding them up would double count.
    pub fn largest_percent(&self) -> Option<f64> {
        self.persons
            .iter()
            .filter_map(|p| p.percent_of_class)
            .fold(None, |m, v| Some(m.map_or(v, |x: f64| x.max(v))))
    }

    /// The largest share count any one reporting person states.
    pub fn largest_aggregate_shares(&self) -> Option<f64> {
        self.persons
            .iter()
            .filter_map(|p| p.aggregate_shares)
            .fold(None, |m, v| Some(m.map_or(v, |x: f64| x.max(v))))
    }
}
