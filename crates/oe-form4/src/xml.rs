//! A minimal XML tokenizer for SEC ownership documents.
//!
//! SEC ownership XML is simple: no DTDs, no namespaces in practice, small
//! files. This tokenizer handles exactly what those files use (declaration,
//! comments, CDATA, attributes, character and entity references) so the
//! parser builds with no dependencies. See `docs/adr/0001-std-only-core.md`
//! for when to swap it for `quick-xml`.

use oe_core::text::{entity_char, tag_end};
use std::fmt;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Token<'a> {
    /// `<name ...>`; the local name, without any namespace prefix.
    Start(&'a str),
    /// `</name>`.
    End(&'a str),
    /// `<name ... />`.
    Empty(&'a str),
    /// Text or CDATA content, entity-decoded, never whitespace-only.
    Text(String),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct XmlError {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for XmlError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "XML error at byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for XmlError {}

fn err(offset: usize, message: impl Into<String>) -> XmlError {
    XmlError {
        offset,
        message: message.into(),
    }
}

/// Splits a document into tokens and checks that tags are balanced.
pub fn tokenize(input: &str) -> Result<Vec<Token<'_>>, XmlError> {
    let input = input.strip_prefix('\u{feff}').unwrap_or(input);
    let bytes = input.as_bytes();
    let mut tokens = Vec::new();
    let mut open: Vec<&str> = Vec::new();
    let mut i = 0;

    while i < bytes.len() {
        if bytes[i] != b'<' {
            let end = input[i..].find('<').map_or(bytes.len(), |p| i + p);
            let raw = &input[i..end];
            if !raw.trim().is_empty() {
                if open.is_empty() {
                    return Err(err(i, "text outside the root element"));
                }
                tokens.push(Token::Text(decode_entities(raw.trim(), i)?));
            }
            i = end;
            continue;
        }

        let rest = &input[i..];
        if rest.starts_with("<?") {
            i += find_from(rest, "?>", i)? + 2;
        } else if rest.starts_with("<!--") {
            i += find_from(rest, "-->", i)? + 3;
        } else if rest.starts_with("<![CDATA[") {
            let close = find_from(rest, "]]>", i)?;
            let text = &rest[9..close];
            if !text.trim().is_empty() {
                tokens.push(Token::Text(text.trim().to_string()));
            }
            i += close + 3;
        } else if rest.starts_with("<!") {
            i += find_from(rest, ">", i)? + 1;
        } else if let Some(after) = rest.strip_prefix("</") {
            let close = after.find('>').ok_or_else(|| err(i, "unclosed end tag"))?;
            let name = local_name(after[..close].trim());
            match open.pop() {
                Some(expected) if expected == name => {}
                Some(expected) => {
                    return Err(err(i, format!("expected </{expected}>, found </{name}>")))
                }
                None => return Err(err(i, format!("unexpected </{name}>"))),
            }
            tokens.push(Token::End(name));
            i += 2 + close + 1;
        } else {
            let close = tag_end(rest).ok_or_else(|| err(i, "unclosed start tag"))?;
            let inner = &rest[1..close];
            let self_closing = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let raw_name = inner
                .split(|c: char| c.is_whitespace())
                .next()
                .unwrap_or("");
            if raw_name.is_empty() {
                return Err(err(i, "tag without a name"));
            }
            let name = local_name(raw_name);
            if self_closing {
                tokens.push(Token::Empty(name));
            } else {
                open.push(name);
                tokens.push(Token::Start(name));
            }
            i += close + 1;
        }
    }

    if let Some(name) = open.pop() {
        return Err(err(bytes.len(), format!("<{name}> is never closed")));
    }
    Ok(tokens)
}

fn find_from(haystack: &str, needle: &str, offset: usize) -> Result<usize, XmlError> {
    haystack
        .find(needle)
        .ok_or_else(|| err(offset, format!("missing '{needle}'")))
}

fn local_name(name: &str) -> &str {
    name.rsplit(':').next().unwrap_or(name)
}

fn decode_entities(raw: &str, offset: usize) -> Result<String, XmlError> {
    if !raw.contains('&') {
        return Ok(raw.to_string());
    }
    let mut out = String::with_capacity(raw.len());
    let mut rest = raw;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        let after = &rest[amp + 1..];
        let semi = after
            .find(';')
            .ok_or_else(|| err(offset, "unterminated entity"))?;
        let entity = &after[..semi];
        // Strict: XML allows only the five named entities and numeric references.
        let decoded =
            entity_char(entity).ok_or_else(|| err(offset, format!("unknown entity &{entity};")))?;
        out.push(decoded);
        rest = &after[semi + 1..];
    }
    out.push_str(rest);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tokenizes_a_small_document() {
        let xml = r#"<?xml version="1.0"?><!-- c --><a x="1>2"><b>Tom &amp; Jerry &#65;&#x42;</b><c/><d><![CDATA[<raw>]]></d></a>"#;
        let tokens = tokenize(xml).unwrap();
        assert_eq!(
            tokens,
            vec![
                Token::Start("a"),
                Token::Start("b"),
                Token::Text("Tom & Jerry AB".into()),
                Token::End("b"),
                Token::Empty("c"),
                Token::Start("d"),
                Token::Text("<raw>".into()),
                Token::End("d"),
                Token::End("a"),
            ]
        );
    }

    #[test]
    fn strips_namespace_prefixes() {
        let tokens = tokenize("<ns:a><ns:b>1</ns:b></ns:a>").unwrap();
        assert_eq!(tokens[0], Token::Start("a"));
        assert_eq!(tokens[3], Token::End("b"));
        assert_eq!(tokens[4], Token::End("a"));
    }

    #[test]
    fn rejects_mismatched_and_unclosed_tags() {
        assert!(tokenize("<a><b></a>").is_err());
        assert!(tokenize("<a>").is_err());
        assert!(tokenize("<a>&bogus;</a>").is_err());
    }
}
