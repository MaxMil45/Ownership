//! Which alerts a person wants. A watchlist is a text file, one entry per line:
//!
//! ```text
//! # comments and blank lines are ignored
//! ticker AAPL          # the issuer's trading symbol (case does not matter)
//! issuer 320193        # the issuer's CIK
//! insider 1494730      # a person or entity by CIK (insiders, challengers)
//! name Elliott         # any named person or holder containing this text
//! rule insider.cluster_buy@1   # every alert of one rule
//! ```
//!
//! An alert matches if any line matches. No watchlist means every alert.

use crate::Alert;
use oe_core::Cik;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Watchlist {
    pub tickers: Vec<String>,
    pub issuers: Vec<Cik>,
    pub insiders: Vec<Cik>,
    /// Lower case.
    pub names: Vec<String>,
    pub rules: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WatchlistError {
    pub line: usize,
    pub message: String,
}

impl std::fmt::Display for WatchlistError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "watchlist line {}: {}", self.line, self.message)
    }
}

impl std::error::Error for WatchlistError {}

impl Watchlist {
    pub fn parse(text: &str) -> Result<Watchlist, WatchlistError> {
        let mut w = Watchlist::default();
        for (i, raw) in text.lines().enumerate() {
            let line = raw.split('#').next().unwrap_or("").trim();
            if line.is_empty() {
                continue;
            }
            let err = |message: String| WatchlistError {
                line: i + 1,
                message,
            };
            let (kind, value) = line
                .split_once(char::is_whitespace)
                .map(|(k, v)| (k, v.trim()))
                .ok_or_else(|| err(format!("expected `<kind> <value>`, got {line:?}")))?;
            let cik = |v: &str| {
                v.parse::<Cik>()
                    .map_err(|_| err(format!("{v:?} is not a CIK number")))
            };
            match kind.to_ascii_lowercase().as_str() {
                "ticker" => w.tickers.push(value.to_ascii_uppercase()),
                "issuer" => w.issuers.push(cik(value)?),
                "insider" => w.insiders.push(cik(value)?),
                "name" => w.names.push(value.to_lowercase()),
                "rule" => w.rules.push(value.to_string()),
                other => {
                    return Err(err(format!(
                        "unknown kind {other:?} (use ticker, issuer, insider, name or rule)"
                    )))
                }
            }
        }
        Ok(w)
    }

    pub fn is_empty(&self) -> bool {
        self.tickers.is_empty()
            && self.issuers.is_empty()
            && self.insiders.is_empty()
            && self.names.is_empty()
            && self.rules.is_empty()
    }

    pub fn len(&self) -> usize {
        self.tickers.len()
            + self.issuers.len()
            + self.insiders.len()
            + self.names.len()
            + self.rules.len()
    }

    pub fn matches(&self, a: &Alert) -> bool {
        let ticker = a.ticker.as_deref().map(str::to_ascii_uppercase);
        ticker.is_some_and(|t| self.tickers.contains(&t))
            || a.issuer_cik.is_some_and(|c| self.issuers.contains(&c))
            || a.people
                .iter()
                .any(|p| p.cik.is_some_and(|c| self.insiders.contains(&c)))
            || a.people.iter().any(|p| {
                let n = p.name.to_lowercase();
                self.names.iter().any(|w| n.contains(w.as_str()))
            })
            || self.rules.iter().any(|r| r == a.rule_id)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Person;

    fn alert() -> Alert {
        Alert {
            key: "k".into(),
            rule_id: "insider.cluster_buy@1",
            known_on: None,
            issuer_cik: Some(Cik(320193)),
            issuer_name: Some("Apple Inc.".into()),
            ticker: Some("aapl".into()),
            headline: String::new(),
            details: Vec::new(),
            people: vec![Person {
                cik: Some(Cik(1_494_730)),
                name: "Elliott Investment Management L.P.".into(),
            }],
            receipts: Vec::new(),
        }
    }

    #[test]
    fn parses_every_kind_and_ignores_comments() {
        let w = Watchlist::parse(
            "# mine\n\nticker aapl\nissuer 0000320193 # Apple\ninsider 1494730\nNAME  Elliott \nrule activist.new_13d@1\n",
        )
        .unwrap();
        assert_eq!(w.tickers, ["AAPL"]);
        assert_eq!(w.issuers, [Cik(320193)]);
        assert_eq!(w.insiders, [Cik(1_494_730)]);
        assert_eq!(w.names, ["elliott"]);
        assert_eq!(w.rules, ["activist.new_13d@1"]);
        assert_eq!(w.len(), 5);
    }

    #[test]
    fn bad_lines_name_their_line_number() {
        let e = Watchlist::parse("ticker A\nissuer apple\n").unwrap_err();
        assert_eq!(e.line, 2);
        assert!(Watchlist::parse("tickr AAPL").is_err());
        assert!(Watchlist::parse("AAPL").is_err());
    }

    #[test]
    fn each_kind_matches_on_its_own() {
        let a = alert();
        for text in [
            "ticker AAPL",
            "issuer 320193",
            "insider 1494730",
            "name elliott investment",
            "rule insider.cluster_buy@1",
        ] {
            assert!(Watchlist::parse(text).unwrap().matches(&a), "{text}");
        }
        for text in [
            "ticker MSFT",
            "issuer 789019",
            "insider 1",
            "name Icahn",
            "rule activist.new_13d@1",
        ] {
            assert!(!Watchlist::parse(text).unwrap().matches(&a), "{text}");
        }
        assert!(Watchlist::default().is_empty());
    }
}
