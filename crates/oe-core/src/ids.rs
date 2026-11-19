//! SEC identifiers.

use std::fmt;
use std::str::FromStr;

/// SEC Central Index Key: the permanent ID of every EDGAR filer.
///
/// Stored as a number; displayed zero-padded to ten digits as EDGAR does.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Cik(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IdParseError(pub String);

impl fmt::Display for IdParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "invalid identifier: {}", self.0)
    }
}

impl std::error::Error for IdParseError {}

impl FromStr for Cik {
    type Err = IdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim();
        if t.is_empty() || t.len() > 10 || !t.bytes().all(|b| b.is_ascii_digit()) {
            return Err(IdParseError(s.to_string()));
        }
        t.parse::<u64>()
            .map(Cik)
            .map_err(|_| IdParseError(s.to_string()))
    }
}

impl fmt::Display for Cik {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:010}", self.0)
    }
}

/// EDGAR accession number, e.g. `0000950170-26-012345`.
///
/// Format: filer-agent CIK (10 digits), year (2 digits), sequence (6 digits).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Accession(String);

impl Accession {
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The form without dashes, as used in EDGAR archive folder paths.
    pub fn compact(&self) -> String {
        self.0.replace('-', "")
    }
}

impl FromStr for Accession {
    type Err = IdParseError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let t = s.trim();
        let parts: Vec<&str> = t.split('-').collect();
        let ok = parts.len() == 3
            && parts[0].len() == 10
            && parts[1].len() == 2
            && parts[2].len() == 6
            && parts.iter().all(|p| p.bytes().all(|b| b.is_ascii_digit()));
        if ok {
            Ok(Accession(t.to_string()))
        } else {
            Err(IdParseError(s.to_string()))
        }
    }
}

impl fmt::Display for Accession {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cik_parses_and_pads() {
        let c: Cik = "0000320193".parse().unwrap();
        assert_eq!(c, Cik(320193));
        assert_eq!(c.to_string(), "0000320193");
        assert!("12a".parse::<Cik>().is_err());
        assert!("".parse::<Cik>().is_err());
        assert!("12345678901".parse::<Cik>().is_err());
    }

    #[test]
    fn accession_validates() {
        let a: Accession = "0000950170-26-012345".parse().unwrap();
        assert_eq!(a.compact(), "000095017026012345");
        assert!("0000950170-2026-012345".parse::<Accession>().is_err());
        assert!("not-an-accession".parse::<Accession>().is_err());
    }
}
