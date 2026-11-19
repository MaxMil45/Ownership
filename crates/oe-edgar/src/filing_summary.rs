//! `FilingSummary.xml`, the index SEC's renderer writes for a filing that has
//! XBRL (`.../<accession without dashes>/FilingSummary.xml`, 30 to 60 KB).
//!
//! It names the main document and lists the rendered report pages (`R1.htm`,
//! `R2.htm`, ...), among them "Insider Trading Arrangements", which is a small
//! page (about 10 to 60 KB) that shows the Item 408 facts. Reading that page
//! first avoids downloading a 2 MB report that has nothing to report.

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ReportRef {
    pub short_name: String,
    /// The rendered page, e.g. `R21.htm`.
    pub html_file: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct FilingSummary {
    /// The main document's file name, e.g. `pltr-20260930.htm`.
    pub main_document: Option<String>,
    /// Its form type as the summary states it, e.g. `10-Q`.
    pub document_type: Option<String>,
    pub reports: Vec<ReportRef>,
}

impl FilingSummary {
    /// The rendered "Insider Trading Arrangements" page, if the filing has one.
    pub fn insider_trading_report(&self) -> Option<&ReportRef> {
        self.reports.iter().find(|r| {
            r.short_name
                .trim()
                .eq_ignore_ascii_case("Insider Trading Arrangements")
        })
    }
}

/// Reads a `FilingSummary.xml`. Anything it cannot read is left empty.
pub fn parse_filing_summary(text: &str) -> FilingSummary {
    let mut summary = FilingSummary::default();

    if let Some(files) = between(text, "<InputFiles>", "</InputFiles>") {
        // The first <File> with a doctype attribute is the main document.
        for chunk in files.split("<File").skip(1) {
            let Some(gt) = chunk.find('>') else { continue };
            let attrs = &chunk[..gt];
            if let Some(doctype) = attribute(attrs, "doctype") {
                let name = chunk[gt + 1..].split("</File>").next().unwrap_or("").trim();
                if !name.is_empty() {
                    summary.main_document = Some(unescape(name));
                    summary.document_type = Some(doctype);
                    break;
                }
            }
        }
    }

    for report in text.split("<Report ").skip(1) {
        let report = report.split("</Report>").next().unwrap_or(report);
        let (Some(file), Some(name)) = (
            between(report, "<HtmlFileName>", "</HtmlFileName>"),
            between(report, "<ShortName>", "</ShortName>"),
        ) else {
            continue;
        };
        summary.reports.push(ReportRef {
            short_name: unescape(name.trim()),
            html_file: file.trim().to_string(),
        });
    }
    summary
}

fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = s.find(open)? + open.len();
    let end = s[start..].find(close)? + start;
    Some(&s[start..end])
}

fn attribute(attrs: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let at = attrs.find(&needle)? + needle.len();
    let len = attrs[at..].find('"')?;
    Some(unescape(&attrs[at..at + len]))
}

fn unescape(s: &str) -> String {
    s.replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&apos;", "'")
        .replace("&amp;", "&")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixture() -> String {
        let path = format!(
            "{}/../../fixtures/edgar/filing_summary_synthetic.xml",
            env!("CARGO_MANIFEST_DIR")
        );
        std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{path}: {e}"))
    }

    #[test]
    fn finds_the_main_document_and_the_insider_report() {
        let s = parse_filing_summary(&fixture());
        assert_eq!(s.main_document.as_deref(), Some("synw-20260930.htm"));
        assert_eq!(s.document_type.as_deref(), Some("10-Q"));
        assert_eq!(s.reports.len(), 4);
        let r = s.insider_trading_report().unwrap();
        assert_eq!(r.html_file, "R21.htm");
        assert_eq!(r.short_name, "Insider Trading Arrangements");
        // Other reports are listed too, and a similar name is not mistaken for it.
        assert_eq!(s.reports[0].short_name, "Cover Page");
        assert!(s
            .reports
            .iter()
            .any(|r| r.short_name == "Insider Trading Policies and Procedures"));
    }

    #[test]
    fn a_filing_without_the_report_or_with_junk_gives_none() {
        let no_report = fixture().replace("Insider Trading Arrangements", "Something Else");
        assert!(parse_filing_summary(&no_report)
            .insider_trading_report()
            .is_none());
        let empty = parse_filing_summary("not xml at all");
        assert_eq!(empty, FilingSummary::default());
        assert!(empty.insider_trading_report().is_none());
    }
}
