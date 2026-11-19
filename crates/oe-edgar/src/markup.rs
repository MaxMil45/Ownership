//! Tiny string helpers for the small, regular XML files EDGAR serves (the
//! Atom feed, `FilingSummary.xml`). Not a parser: each file reads a handful of
//! fields by position.

/// The text between the first `open` and the next `close` after it.
pub(crate) fn between<'a>(s: &'a str, open: &str, close: &str) -> Option<&'a str> {
    let start = s.find(open)? + open.len();
    let end = s[start..].find(close)? + start;
    Some(&s[start..end])
}

/// The value of `name="..."` in a run of attributes, with entities decoded.
pub(crate) fn attribute(attrs: &str, name: &str) -> Option<String> {
    let needle = format!("{name}=\"");
    let at = attrs.find(&needle)? + needle.len();
    let len = attrs[at..].find('"')?;
    Some(unescape(&attrs[at..at + len]))
}

/// The value of attribute `name` on the first element that starts with `tag`
/// (for example `<category`) in `s`.
pub(crate) fn element_attribute(s: &str, tag: &str, name: &str) -> Option<String> {
    let element = &s[s.find(tag)?..];
    attribute(&element[..element.find('>')?], name)
}

/// Entity references decoded (`&amp;`, `&#39;`, ...); unknown ones kept.
pub(crate) fn unescape(s: &str) -> String {
    oe_core::text::decode_entities_lenient(s)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn helpers() {
        assert_eq!(between("<a>x</a><a>y</a>", "<a>", "</a>"), Some("x"));
        assert_eq!(between("<a>x", "<a>", "</a>"), None);
        assert_eq!(
            attribute(r#"doc="10-Q" x="a&amp;b""#, "x").as_deref(),
            Some("a&b")
        );
        assert_eq!(attribute(r#"doc="10-Q""#, "y"), None);
        let s = r#"<link href="/x?a=1&amp;b=2"/><category term="4/A"/>"#;
        assert_eq!(
            element_attribute(s, "<category", "term").as_deref(),
            Some("4/A")
        );
        assert_eq!(
            element_attribute(s, "<link", "href").as_deref(),
            Some("/x?a=1&b=2")
        );
        assert_eq!(element_attribute(s, "<id", "x"), None);
        assert_eq!(
            unescape("Widgets &amp; Sons &#39;A&#39;"),
            "Widgets & Sons 'A'"
        );
    }
}
