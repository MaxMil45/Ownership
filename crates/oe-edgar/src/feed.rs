//! The EDGAR "latest filings" Atom feed: the closest thing EDGAR has to a live
//! stream, updated within a minute or so of acceptance.
//!
//! `https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=4&owner=only&count=100&output=atom`
//!
//! `owner=only` is what limits the feed to ownership forms (Form 4 and 4/A);
//! with `owner=include` the `type=4` filter is ignored and the feed is mostly
//! prospectuses. Like the daily index, the feed lists a Form 4 once per filer
//! (issuer and each reporting owner), so entries repeat accession numbers.

use crate::markup::{between, element_attribute, unescape};

/// The feed URL for ownership forms, newest 100 entries (about 50 filings).
pub const LATEST_OWNERSHIP_FEED_URL: &str =
    "https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=4&owner=only&count=100&output=atom";

/// The newest 100 feed entries whose form type **starts with** `type_prefix`
/// (checked 2026-11-20: `SCHEDULE 13` gives 13D, 13D/A, 13G and 13G/A; `DEF`
/// gives every DEF form; `SC 13D`, the pre-2025 name, gives nothing).
/// `owner_only` must be true for Forms 3/4/5 (see the module docs) and false
/// for everything else, where `owner=only` would hide the filings.
pub fn latest_filings_url(type_prefix: &str, owner_only: bool) -> String {
    format!(
        "https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type={}&owner={}&count=100&output=atom",
        type_prefix.replace(' ', "%20"),
        if owner_only { "only" } else { "include" }
    )
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FeedEntry {
    /// Form type from the entry's category, e.g. `4` or `4/A`.
    pub form: String,
    pub accession: String,
    /// `Issuer`, `Reporting` or `Filer`.
    pub role: String,
    pub name: String,
    pub cik: u64,
    /// Entry time as the feed writes it, e.g. `2026-11-19T08:54:50-05:00`.
    pub updated: String,
    /// URL of the filing's index page.
    pub index_url: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FeedError {
    /// The text does not look like an Atom feed at all (an HTML error page,
    /// for example).
    NotAFeed,
}

impl std::fmt::Display for FeedError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FeedError::NotAFeed => f.write_str("the response is not an EDGAR Atom feed"),
        }
    }
}

impl std::error::Error for FeedError {}

/// Parses the feed into entries, newest first as the SEC lists them. Entries
/// that lack an accession number or a readable title are skipped.
pub fn parse_atom_feed(text: &str) -> Result<Vec<FeedEntry>, FeedError> {
    if !text.contains("<feed") {
        return Err(FeedError::NotAFeed);
    }
    Ok(text
        .split("<entry>")
        .skip(1)
        .filter_map(parse_entry)
        .collect())
}

fn parse_entry(chunk: &str) -> Option<FeedEntry> {
    let chunk = chunk.split("</entry>").next()?;
    let title = unescape(between(chunk, "<title>", "</title>")?);
    let (form_and_name, role) = split_role(&title)?;
    let (form_in_title, name_and_cik) = form_and_name.split_once(" - ")?;
    let (name, cik) = name_and_cik.rsplit_once(" (")?;
    let cik: u64 = cik.strip_suffix(')')?.parse().ok()?;

    let id = between(chunk, "<id>", "</id>")?;
    let accession = id.split("accession-number=").nth(1)?.trim().to_string();

    let form =
        element_attribute(chunk, "<category", "term").unwrap_or_else(|| form_in_title.to_string());
    Some(FeedEntry {
        form,
        accession,
        role: role.to_string(),
        name: name.trim().to_string(),
        cik,
        updated: between(chunk, "<updated>", "</updated>")?
            .trim()
            .to_string(),
        index_url: element_attribute(chunk, "<link", "href").unwrap_or_default(),
    })
}

/// `"4 - Name (0001) (Issuer)"` → (`"4 - Name (0001)"`, `"Issuer"`).
fn split_role(title: &str) -> Option<(&str, &str)> {
    let (rest, role) = title.trim_end().rsplit_once(" (")?;
    Some((rest, role.strip_suffix(')')?))
}

/// The distinct accession numbers of entries whose form is in `forms`, in feed
/// order (newest first).
pub fn unique_accessions<'a>(entries: &'a [FeedEntry], forms: &[&str]) -> Vec<&'a FeedEntry> {
    let mut seen = std::collections::HashSet::new();
    entries
        .iter()
        .filter(|e| forms.contains(&e.form.as_str()))
        .filter(|e| seen.insert(e.accession.as_str()))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        let path = format!(
            "{}/../../fixtures/edgar/latest_feed_synthetic.atom",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    #[test]
    fn parses_entries() {
        let entries = parse_atom_feed(&fixture()).unwrap();
        assert_eq!(entries.len(), 5);
        let first = &entries[0];
        assert_eq!(first.form, "4");
        assert_eq!(first.role, "Reporting");
        assert_eq!(first.name, "Insider, Alpha");
        assert_eq!(first.cik, 9_999_911);
        assert_eq!(first.accession, "0009999901-26-000011");
        assert_eq!(first.updated, "2026-03-11T16:54:50-05:00");
        assert_eq!(
            first.index_url,
            "https://www.sec.gov/Archives/edgar/data/9999911/000999990126000011/0009999901-26-000011-index.htm"
        );
    }

    #[test]
    fn unescapes_names_and_keeps_parentheses_and_dashes() {
        let entries = parse_atom_feed(&fixture()).unwrap();
        let issuer = &entries[1];
        assert_eq!(issuer.role, "Issuer");
        assert_eq!(issuer.name, "Widgets & Sons - Class A (Holdings) Inc");
        assert_eq!(issuer.cik, 9_999_901);
    }

    #[test]
    fn filing_listed_for_each_filer_is_one_accession() {
        let entries = parse_atom_feed(&fixture()).unwrap();
        let only4: Vec<&str> = unique_accessions(&entries, &["4"])
            .iter()
            .map(|e| e.accession.as_str())
            .collect();
        assert_eq!(only4, ["0009999901-26-000011", "0009999902-26-000007"]);
        assert_eq!(unique_accessions(&entries, &["4", "4/A"]).len(), 3);
    }

    #[test]
    fn builds_feed_urls() {
        assert_eq!(latest_filings_url("4", true), LATEST_OWNERSHIP_FEED_URL);
        assert_eq!(
            latest_filings_url("SCHEDULE 13", false),
            "https://www.sec.gov/cgi-bin/browse-edgar?action=getcurrent&type=SCHEDULE%2013&owner=include&count=100&output=atom"
        );
    }

    #[test]
    fn reads_a_real_schedule_13_feed() {
        // Six entries of the real feed for `type=SCHEDULE 13`, 2026-11-20.
        let path = format!(
            "{}/../../fixtures/edgar/latest_feed_sched13_real.atom",
            env!("CARGO_MANIFEST_DIR")
        );
        let text = std::fs::read_to_string(&path).unwrap();
        let entries = parse_atom_feed(&text).unwrap();
        assert_eq!(entries.len(), 6);
        assert_eq!(entries[0].role, "Filed by");
        assert_eq!(entries[0].name, "Saba Capital Management, L.P.");
        assert_eq!(entries[1].role, "Subject");
        assert_eq!(entries[1].cik, 836_412);
        let d = unique_accessions(&entries, &["SCHEDULE 13D"]);
        // Saba filed two 13Ds (Virtus, Destiny); each is listed once per party.
        assert_eq!(d.len(), 2);
        assert_eq!(d[0].accession, "0001062993-26-006214");
        assert_eq!(unique_accessions(&entries, &["SCHEDULE 13G"]).len(), 1);
    }

    #[test]
    fn rejects_html_and_skips_broken_entries() {
        assert_eq!(
            parse_atom_feed("<html><body>Request Rate Threshold Exceeded</body></html>"),
            Err(FeedError::NotAFeed)
        );
        let broken = "<feed><entry><title>garbage</title></entry></feed>";
        assert!(parse_atom_feed(broken).unwrap().is_empty());
        assert!(parse_atom_feed("<feed></feed>").unwrap().is_empty());
    }
}
