//! A small reader for Inline XBRL (iXBRL): the XHTML format in which 10-Q and
//! 10-K reports carry their XBRL facts. It finds two things only:
//!
//! * `<xbrli:context>` blocks, for the dimension members (which trading plan,
//!   which individual) and the period end;
//! * `<ix:nonNumeric>` and `<ix:nonFraction>` facts whose name the caller wants.
//!
//! It is a scanner, not an XML parser: reports are 1 to 3 MB, almost all of it
//! formatted text that has nothing to do with these facts. Malformed input
//! gives fewer results, never a panic.

use oe_core::text::tag_end;
use oe_core::Date;
use std::collections::HashMap;

#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Context {
    pub id: String,
    /// Member of `ecd:TradingArrAxis`: names one trading plan.
    pub arrangement: Option<String>,
    /// Member of `ecd:IndividualAxis`: names one person.
    pub individual: Option<String>,
    /// End date of the period, or the instant.
    pub end: Option<Date>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Fact {
    /// Prefixed concept name, e.g. `ecd:TrdArrIndName`.
    pub name: String,
    pub context_ref: String,
    /// The `format` attribute, e.g. `ixt:fixed-true`, or empty.
    pub format: String,
    /// The `scale` attribute (power of ten), 0 if absent.
    pub scale: i32,
    pub negative: bool,
    /// Visible text with tags removed, entities decoded, whitespace collapsed.
    pub text: String,
    /// `xsi:nil="true"`: the fact is explicitly empty.
    pub nil: bool,
}

/// All contexts in the document, by id.
pub fn contexts(html: &str) -> HashMap<String, Context> {
    let mut out = HashMap::new();
    let mut from = 0;
    while let Some(rel) = html[from..].find("context id=\"") {
        let at = from + rel;
        from = at + 1;
        // Must be a start tag: `<xbrli:context id=` or `<context id=`.
        let before = &html[..at];
        let prefix_len = if before.ends_with("<xbrli:") {
            7
        } else if before.ends_with('<') {
            1
        } else {
            continue;
        };
        let id_start = at + "context id=\"".len();
        let Some(id_len) = html[id_start..].find('"') else {
            continue;
        };
        let id = &html[id_start..id_start + id_len];
        let close = if prefix_len == 7 {
            "</xbrli:context>"
        } else {
            "</context>"
        };
        let Some(end_rel) = html[id_start..].find(close) else {
            continue;
        };
        let body = &html[id_start..id_start + end_rel];
        out.insert(
            id.to_string(),
            Context {
                id: id.to_string(),
                arrangement: member(body, "ecd:TradingArrAxis"),
                individual: member(body, "ecd:IndividualAxis"),
                end: tag_text(body, "endDate>")
                    .or_else(|| tag_text(body, "instant>"))
                    .and_then(|d| d.parse().ok()),
            },
        );
    }
    out
}

/// The text of an `explicitMember` for a dimension.
fn member(body: &str, dimension: &str) -> Option<String> {
    let needle = format!("dimension=\"{dimension}\"");
    let at = body.find(&needle)? + needle.len();
    let rest = &body[at..];
    let gt = rest.find('>')?;
    let rest = &rest[gt + 1..];
    let lt = rest.find('<')?;
    let value = rest[..lt].trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// The text after the first `marker` (for example `endDate>`) up to the next `<`.
fn tag_text(body: &str, marker: &str) -> Option<String> {
    let at = body.find(marker)? + marker.len();
    let lt = body[at..].find('<')?;
    let value = body[at..at + lt].trim();
    (!value.is_empty()).then(|| value.to_string())
}

/// Every `ix:nonNumeric` / `ix:nonFraction` fact whose name satisfies `wanted`,
/// in document order. Facts nested inside other facts are found too.
pub fn facts(html: &str, wanted: impl Fn(&str) -> bool) -> Vec<Fact> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(rel) = html[from..].find("<ix:non") {
        let start = from + rel;
        from = start + 1;
        let element = if html[start..].starts_with("<ix:nonNumeric") {
            "ix:nonNumeric"
        } else if html[start..].starts_with("<ix:nonFraction") {
            "ix:nonFraction"
        } else {
            continue;
        };
        // The character after the element name must end the name.
        match html[start + 1 + element.len()..].chars().next() {
            Some(c) if c.is_whitespace() || c == '>' || c == '/' => {}
            _ => continue,
        }
        let Some(tag_len) = tag_end(&html[start..]).map(|i| i + 1) else {
            continue;
        };
        let tag = &html[start..start + tag_len];
        let Some(name) = attr(tag, "name") else {
            continue;
        };
        if !wanted(&name) {
            continue;
        }

        let self_closing = tag.ends_with("/>");
        let inner = if self_closing {
            ""
        } else {
            match matching_close(&html[start + tag_len..], element) {
                Some(len) => &html[start + tag_len..start + tag_len + len],
                None => continue,
            }
        };
        out.push(Fact {
            name,
            context_ref: attr(tag, "contextRef").unwrap_or_default(),
            format: attr(tag, "format").unwrap_or_default(),
            scale: attr(tag, "scale").and_then(|s| s.parse().ok()).unwrap_or(0),
            negative: attr(tag, "sign").as_deref() == Some("-"),
            text: text_of(inner),
            nil: attr(tag, "xsi:nil").as_deref() == Some("true"),
        });
    }
    out
}

/// The byte ranges of every fact whose name satisfies `wanted` and that is not
/// inside another such fact, each from its start tag to the end of its close
/// tag, with its `contextRef`, in document order. For [`crate::excerpt`].
pub fn fact_spans(html: &str, wanted: impl Fn(&str) -> bool) -> Vec<(usize, usize, String)> {
    let mut out: Vec<(usize, usize, String)> = Vec::new();
    let mut from = 0;
    while let Some(rel) = html[from..].find("<ix:non") {
        let start = from + rel;
        from = start + 1;
        let element = if html[start..].starts_with("<ix:nonNumeric") {
            "ix:nonNumeric"
        } else if html[start..].starts_with("<ix:nonFraction") {
            "ix:nonFraction"
        } else {
            continue;
        };
        match html[start + 1 + element.len()..].chars().next() {
            Some(c) if c.is_whitespace() || c == '>' || c == '/' => {}
            _ => continue,
        }
        let Some(tag_len) = tag_end(&html[start..]).map(|i| i + 1) else {
            continue;
        };
        let tag = &html[start..start + tag_len];
        let Some(name) = attr(tag, "name") else {
            continue;
        };
        if !wanted(&name) || out.last().is_some_and(|(_, end, _)| start < *end) {
            continue;
        }
        let end = if tag.ends_with("/>") {
            start + tag_len
        } else {
            match matching_close(&html[start + tag_len..], element) {
                Some(len) => start + tag_len + len + element.len() + 3,
                None => continue,
            }
        };
        out.push((start, end, attr(tag, "contextRef").unwrap_or_default()));
    }
    out
}

/// The byte range of each `<xbrli:context>` (or `<context>`) element, by id.
pub fn context_spans(html: &str) -> HashMap<String, (usize, usize)> {
    let mut out = HashMap::new();
    let mut from = 0;
    while let Some(rel) = html[from..].find("context id=\"") {
        let at = from + rel;
        from = at + 1;
        let before = &html[..at];
        let (open_at, close) = if before.ends_with("<xbrli:") {
            (at - 7, "</xbrli:context>")
        } else if before.ends_with('<') {
            (at - 1, "</context>")
        } else {
            continue;
        };
        let id_start = at + "context id=\"".len();
        let Some(id_len) = html[id_start..].find('"') else {
            continue;
        };
        let Some(end_rel) = html[id_start..].find(close) else {
            continue;
        };
        let id = html[id_start..id_start + id_len].to_string();
        // The last one wins, as in [`contexts`].
        out.insert(id, (open_at, id_start + end_rel + close.len()));
    }
    out
}

/// The value of ` name="..."` in a tag.
fn attr(tag: &str, name: &str) -> Option<String> {
    let needle = format!(" {name}=\"");
    let at = tag.find(&needle)? + needle.len();
    let len = tag[at..].find('"')?;
    Some(decode_entities(&tag[at..at + len]))
}

/// Length of the content of an element, up to its matching close tag, with
/// nesting of the same element name taken into account.
fn matching_close(after_open: &str, element: &str) -> Option<usize> {
    let open = format!("<{element}");
    let close = format!("</{element}>");
    let mut depth = 1usize;
    let mut pos = 0;
    loop {
        let next_close = after_open[pos..].find(&close)? + pos;
        let next_open = after_open[pos..].find(&open).map(|p| p + pos);
        match next_open {
            Some(o) if o < next_close => {
                // A nested start tag; self-closing ones do not add depth.
                let tag_len = tag_end(&after_open[o..])? + 1;
                if !after_open[o..o + tag_len].ends_with("/>") {
                    depth += 1;
                }
                pos = o + tag_len;
            }
            _ => {
                depth -= 1;
                if depth == 0 {
                    return Some(next_close);
                }
                pos = next_close + close.len();
            }
        }
    }
}

/// Visible text of an HTML fragment: tags removed (block tags become spaces),
/// entities decoded, runs of whitespace collapsed to one space.
pub fn text_of(fragment: &str) -> String {
    let mut text = String::with_capacity(fragment.len());
    let mut rest = fragment;
    while let Some(lt) = rest.find('<') {
        text.push_str(&rest[..lt]);
        let Some(gt) = rest[lt..].find('>') else {
            rest = "";
            break;
        };
        let tag = rest[lt + 1..lt + gt].trim_start_matches('/');
        let name = tag
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("");
        if matches!(name, "p" | "br" | "div" | "tr" | "td" | "li" | "th") {
            text.push(' ');
        }
        rest = &rest[lt + gt + 1..];
    }
    text.push_str(rest);
    decode_entities(&text)
        .split(|c: char| c.is_whitespace() || c == '\u{a0}')
        .filter(|w| !w.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Decodes the five XML entities and numeric references; `&nbsp;` becomes a
/// space. Anything else is left as written. (The one copy lives in `oe-core`.)
pub use oe_core::text::decode_entities_lenient as decode_entities;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_contexts_with_dimensions_and_periods() {
        let html = r#"<xbrli:context id="c-1"><xbrli:entity><xbrli:identifier scheme="x">1</xbrli:identifier>
            <xbrli:segment><xbrldi:explicitMember dimension="ecd:TradingArrAxis">co:PlanMember</xbrldi:explicitMember>
            <xbrldi:explicitMember dimension="ecd:IndividualAxis">co:PersonMember</xbrldi:explicitMember></xbrli:segment></xbrli:entity>
            <xbrli:period><xbrli:startDate>2026-07-01</xbrli:startDate><xbrli:endDate>2026-09-30</xbrli:endDate></xbrli:period></xbrli:context>
            <xbrli:context id="c-2"><xbrli:entity/><xbrli:period><xbrli:instant>2026-09-30</xbrli:instant></xbrli:period></xbrli:context>
            <p>context id="not a tag"</p>"#;
        let c = contexts(html);
        assert_eq!(c.len(), 2);
        let one = &c["c-1"];
        assert_eq!(one.arrangement.as_deref(), Some("co:PlanMember"));
        assert_eq!(one.individual.as_deref(), Some("co:PersonMember"));
        assert_eq!(one.end, Date::from_ymd(2026, 9, 30));
        let two = &c["c-2"];
        assert_eq!(
            (two.arrangement.as_deref(), two.individual.as_deref()),
            (None, None)
        );
        assert_eq!(two.end, Date::from_ymd(2026, 9, 30));
    }

    #[test]
    fn reads_facts_and_ignores_unwanted_ones() {
        let html = r#"<p>x</p><ix:nonNumeric contextRef="c-1" name="ecd:TrdArrIndName" id="f-1">Jane <span>Q.</span> Doe</ix:nonNumeric>
            <ix:nonNumeric contextRef="c-1" name="dei:Other" id="f-2">skip me</ix:nonNumeric>
            <ix:nonNumeric contextRef="c-2" name="ecd:Rule10b51ArrTrmntdFlag" format="ixt:fixed-true" id="f-3">terminated</ix:nonNumeric>
            <ix:nonFraction unitRef="shares" contextRef="c-3" decimals="INF" name="ecd:TrdArrSecuritiesAggAvailAmt" format="ixt:num-dot-decimal" scale="3" sign="-" id="f-4">1,450</ix:nonFraction>
            <ix:nonNumeric contextRef="c-4" name="ecd:TrdArrDuration" xsi:nil="true"/>"#;
        let f = facts(html, |n| n.starts_with("ecd:"));
        let names: Vec<&str> = f.iter().map(|x| x.name.as_str()).collect();
        assert_eq!(
            names,
            [
                "ecd:TrdArrIndName",
                "ecd:Rule10b51ArrTrmntdFlag",
                "ecd:TrdArrSecuritiesAggAvailAmt",
                "ecd:TrdArrDuration"
            ]
        );
        assert_eq!(f[0].text, "Jane Q. Doe");
        assert_eq!(f[0].context_ref, "c-1");
        assert_eq!(
            (f[1].format.as_str(), f[1].text.as_str()),
            ("ixt:fixed-true", "terminated")
        );
        assert_eq!(
            (f[2].scale, f[2].negative, f[2].text.as_str()),
            (3, true, "1,450")
        );
        assert!(f[3].nil && f[3].text.is_empty());
    }

    #[test]
    fn nested_facts_are_found_and_the_outer_text_is_whole() {
        let html = r#"<ix:nonNumeric contextRef="c-1" name="ecd:MtrlTermsOfTrdArrTextBlock"><p>On <ix:nonNumeric contextRef="c-1" name="ecd:TrdArrTerminationDate">August 6, 2026</ix:nonNumeric>, Jane terminated it.</p><p>Second&#160;paragraph &amp; more.</p></ix:nonNumeric>after"#;
        let f = facts(html, |n| n.starts_with("ecd:"));
        assert_eq!(f.len(), 2);
        assert_eq!(f[0].name, "ecd:MtrlTermsOfTrdArrTextBlock");
        assert_eq!(
            f[0].text,
            "On August 6, 2026, Jane terminated it. Second paragraph & more."
        );
        assert_eq!(f[1].text, "August 6, 2026");
    }

    #[test]
    fn broken_input_gives_less_not_a_panic() {
        for html in [
            "",
            "<ix:nonNumeric",
            "<ix:nonNumeric name=\"ecd:A\"",
            "<ix:nonNumeric name=\"ecd:A\">never closed",
            "<ix:nonNumericX name=\"ecd:A\">x</ix:nonNumericX>",
            "<xbrli:context id=\"c\"",
            "<xbrli:context id=\"c\">no close",
        ] {
            let _ = facts(html, |_| true);
            let _ = contexts(html);
        }
        assert!(facts("<ix:nonNumeric name=\"ecd:A\">never closed", |_| true).is_empty());
        assert!(
            facts("<ix:nonNumericX name=\"ecd:A\">x</ix:nonNumericX>", |_| {
                true
            })
            .is_empty()
        );
    }

    #[test]
    fn entities_decode() {
        assert_eq!(
            decode_entities("A &amp; B &lt;c&gt; &quot;d&quot; &#8217; &#x41; &nbsp;x"),
            "A & B <c> \"d\" \u{2019} A  x"
        );
        assert_eq!(
            decode_entities("fish & chips &unknown; &#zzz;"),
            "fish & chips &unknown; &#zzz;"
        );
        assert_eq!(text_of("  a\u{a0}\n  b  "), "a b");
    }
}
