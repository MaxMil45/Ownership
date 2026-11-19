//! Append-only, point-in-time ledger.
//!
//! Every fact carries two dates:
//!
//! * `valid_on`: when the fact was true in the world (a transaction date, a
//!   quarter end);
//! * `known_on`: when the public could first know it (the SEC acceptance date).
//!
//! Amendments are appended, never written over. Queries ask "what did we
//! know about this key as of date X?", which is what makes backtests honest.
//!
//! This in-memory version defines the semantics and the tests. The PostgreSQL
//! schema in `migrations/0001_init.sql` implements the same rules for the
//! live system.

use oe_core::Date;

/// Index of a fact in the ledger. Stable: facts are never removed.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct FactId(pub usize);

#[derive(Debug, Clone, PartialEq)]
pub struct Fact<T> {
    pub key: String,
    pub value: T,
    pub valid_on: Date,
    pub known_on: Date,
    /// Where the fact came from, for example an accession number.
    pub source: String,
    /// The earlier fact this one corrects, if it is an amendment.
    pub supersedes: Option<FactId>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LedgerError {
    /// An amendment pointed at a fact that does not exist.
    UnknownFact(FactId),
    /// An amendment pointed at a fact with a different key.
    KeyMismatch { expected: String, found: String },
    /// An amendment claimed to be known before the fact it corrects.
    KnownBeforeOriginal,
}

impl std::fmt::Display for LedgerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LedgerError::UnknownFact(id) => write!(f, "unknown fact {}", id.0),
            LedgerError::KeyMismatch { expected, found } => {
                write!(f, "amendment key {found:?} does not match {expected:?}")
            }
            LedgerError::KnownBeforeOriginal => {
                f.write_str("an amendment cannot be known before the fact it corrects")
            }
        }
    }
}

impl std::error::Error for LedgerError {}

#[derive(Debug, Clone, Default)]
pub struct Ledger<T> {
    facts: Vec<Fact<T>>,
}

impl<T> Ledger<T> {
    pub fn new() -> Self {
        Ledger { facts: Vec::new() }
    }

    pub fn len(&self) -> usize {
        self.facts.len()
    }

    pub fn is_empty(&self) -> bool {
        self.facts.is_empty()
    }

    pub fn get(&self, id: FactId) -> Option<&Fact<T>> {
        self.facts.get(id.0)
    }

    /// Appends a fact. Amendments are checked against what they supersede.
    pub fn append(&mut self, fact: Fact<T>) -> Result<FactId, LedgerError> {
        if let Some(prev_id) = fact.supersedes {
            let prev = self.get(prev_id).ok_or(LedgerError::UnknownFact(prev_id))?;
            if prev.key != fact.key {
                return Err(LedgerError::KeyMismatch {
                    expected: prev.key.clone(),
                    found: fact.key,
                });
            }
            if fact.known_on < prev.known_on {
                return Err(LedgerError::KnownBeforeOriginal);
            }
        }
        self.facts.push(fact);
        Ok(FactId(self.facts.len() - 1))
    }

    /// The latest version of `key` that the public could know on `as_of`.
    ///
    /// "Latest" means the most recently known; ties go to the later append.
    pub fn as_of(&self, key: &str, as_of: Date) -> Option<&Fact<T>> {
        self.facts
            .iter()
            .filter(|f| f.key == key && f.known_on <= as_of)
            .max_by_key(|f| f.known_on)
    }

    /// Every version of `key`, in the order they were appended.
    pub fn history<'a>(&'a self, key: &'a str) -> impl Iterator<Item = &'a Fact<T>> + 'a {
        self.facts.iter().filter(move |f| f.key == key)
    }

    /// All facts known on `as_of`, latest version per key.
    pub fn snapshot(&self, as_of: Date) -> Vec<&Fact<T>> {
        let mut keys: Vec<&str> = self.facts.iter().map(|f| f.key.as_str()).collect();
        keys.sort_unstable();
        keys.dedup();
        keys.into_iter()
            .filter_map(|k| self.as_of(k, as_of))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(y: i32, m: u32, day: u32) -> Date {
        Date::from_ymd(y, m, day).unwrap()
    }

    fn fact(value: f64, known_on: Date, supersedes: Option<FactId>) -> Fact<f64> {
        Fact {
            key: "holding:0009000001:0009999901".into(),
            value,
            valid_on: d(2026, 3, 2),
            known_on,
            source: "test".into(),
            supersedes,
        }
    }

    #[test]
    fn amendment_does_not_rewrite_the_past() {
        let mut ledger = Ledger::new();
        let original = ledger.append(fact(45_000.0, d(2026, 3, 4), None)).unwrap();
        ledger
            .append(fact(46_000.0, d(2026, 4, 10), Some(original)))
            .unwrap();

        let key = "holding:0009000001:0009999901";
        assert_eq!(ledger.as_of(key, d(2026, 3, 3)), None);
        assert_eq!(ledger.as_of(key, d(2026, 3, 31)).unwrap().value, 45_000.0);
        assert_eq!(ledger.as_of(key, d(2026, 4, 10)).unwrap().value, 46_000.0);
        assert_eq!(ledger.history(key).count(), 2);
        assert_eq!(ledger.snapshot(d(2026, 3, 31)).len(), 1);
    }

    #[test]
    fn rejects_bad_amendments() {
        let mut ledger = Ledger::new();
        let original = ledger.append(fact(1.0, d(2026, 3, 4), None)).unwrap();
        assert_eq!(
            ledger.append(fact(2.0, d(2026, 3, 1), Some(original))),
            Err(LedgerError::KnownBeforeOriginal)
        );
        assert_eq!(
            ledger.append(fact(2.0, d(2026, 3, 9), Some(FactId(7)))),
            Err(LedgerError::UnknownFact(FactId(7)))
        );
        let mut other = fact(2.0, d(2026, 3, 9), Some(original));
        other.key = "something-else".into();
        assert!(matches!(
            ledger.append(other),
            Err(LedgerError::KeyMismatch { .. })
        ));
    }
}
