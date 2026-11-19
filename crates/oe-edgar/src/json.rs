//! A small JSON reader for SEC API responses.
//!
//! The SEC's JSON is plain (objects, arrays, strings, numbers), so a
//! dependency-free reader keeps `oe-edgar` testable offline. See
//! `docs/adr/0001-std-only-core.md`.

use std::fmt;

#[derive(Debug, Clone, PartialEq)]
pub enum Json {
    Null,
    Bool(bool),
    Number(f64),
    String(String),
    Array(Vec<Json>),
    /// Keys in document order.
    Object(Vec<(String, Json)>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct JsonError {
    pub offset: usize,
    pub message: String,
}

impl fmt::Display for JsonError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "JSON error at byte {}: {}", self.offset, self.message)
    }
}

impl std::error::Error for JsonError {}

impl Json {
    pub fn get(&self, key: &str) -> Option<&Json> {
        match self {
            Json::Object(pairs) => pairs.iter().find(|(k, _)| k == key).map(|(_, v)| v),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Json::String(s) => Some(s),
            _ => None,
        }
    }

    pub fn as_f64(&self) -> Option<f64> {
        match self {
            Json::Number(n) => Some(*n),
            _ => None,
        }
    }

    pub fn as_array(&self) -> Option<&[Json]> {
        match self {
            Json::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_object(&self) -> Option<&[(String, Json)]> {
        match self {
            Json::Object(pairs) => Some(pairs),
            _ => None,
        }
    }

    /// A string array, or `None` if any element is not a string.
    pub fn as_str_vec(&self) -> Option<Vec<&str>> {
        self.as_array()?.iter().map(Json::as_str).collect()
    }
}

pub fn parse(input: &str) -> Result<Json, JsonError> {
    let mut p = Parser {
        bytes: input.as_bytes(),
        src: input,
        pos: 0,
        depth: 0,
    };
    p.skip_ws();
    let value = p.value()?;
    p.skip_ws();
    if p.pos != p.bytes.len() {
        return Err(p.err("trailing characters after the document"));
    }
    Ok(value)
}

const MAX_DEPTH: usize = 128;

struct Parser<'a> {
    bytes: &'a [u8],
    src: &'a str,
    pos: usize,
    depth: usize,
}

impl Parser<'_> {
    fn err(&self, message: &str) -> JsonError {
        JsonError {
            offset: self.pos,
            message: message.to_string(),
        }
    }

    fn skip_ws(&mut self) {
        while self.pos < self.bytes.len()
            && matches!(self.bytes[self.pos], b' ' | b'\t' | b'\n' | b'\r')
        {
            self.pos += 1;
        }
    }

    fn peek(&self) -> Option<u8> {
        self.bytes.get(self.pos).copied()
    }

    fn expect_literal(&mut self, lit: &str, value: Json) -> Result<Json, JsonError> {
        if self.src[self.pos..].starts_with(lit) {
            self.pos += lit.len();
            Ok(value)
        } else {
            Err(self.err("invalid literal"))
        }
    }

    fn value(&mut self) -> Result<Json, JsonError> {
        match self.peek() {
            Some(b'{') => self.object(),
            Some(b'[') => self.array(),
            Some(b'"') => self.string().map(Json::String),
            Some(b't') => self.expect_literal("true", Json::Bool(true)),
            Some(b'f') => self.expect_literal("false", Json::Bool(false)),
            Some(b'n') => self.expect_literal("null", Json::Null),
            Some(b'-' | b'0'..=b'9') => self.number(),
            Some(_) => Err(self.err("unexpected character")),
            None => Err(self.err("unexpected end of input")),
        }
    }

    fn enter(&mut self) -> Result<(), JsonError> {
        self.depth += 1;
        if self.depth > MAX_DEPTH {
            Err(self.err("nesting too deep"))
        } else {
            Ok(())
        }
    }

    fn object(&mut self) -> Result<Json, JsonError> {
        self.enter()?;
        self.pos += 1;
        let mut pairs = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b'}') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Json::Object(pairs));
        }
        loop {
            self.skip_ws();
            if self.peek() != Some(b'"') {
                return Err(self.err("expected a string key"));
            }
            let key = self.string()?;
            self.skip_ws();
            if self.peek() != Some(b':') {
                return Err(self.err("expected ':'"));
            }
            self.pos += 1;
            self.skip_ws();
            let value = self.value()?;
            pairs.push((key, value));
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b'}') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(Json::Object(pairs));
                }
                _ => return Err(self.err("expected ',' or '}'")),
            }
        }
    }

    fn array(&mut self) -> Result<Json, JsonError> {
        self.enter()?;
        self.pos += 1;
        let mut items = Vec::new();
        self.skip_ws();
        if self.peek() == Some(b']') {
            self.pos += 1;
            self.depth -= 1;
            return Ok(Json::Array(items));
        }
        loop {
            self.skip_ws();
            items.push(self.value()?);
            self.skip_ws();
            match self.peek() {
                Some(b',') => self.pos += 1,
                Some(b']') => {
                    self.pos += 1;
                    self.depth -= 1;
                    return Ok(Json::Array(items));
                }
                _ => return Err(self.err("expected ',' or ']'")),
            }
        }
    }

    fn number(&mut self) -> Result<Json, JsonError> {
        let start = self.pos;
        while let Some(b) = self.peek() {
            if b.is_ascii_digit() || matches!(b, b'-' | b'+' | b'.' | b'e' | b'E') {
                self.pos += 1;
            } else {
                break;
            }
        }
        self.src[start..self.pos]
            .parse::<f64>()
            .map(Json::Number)
            .map_err(|_| JsonError {
                offset: start,
                message: "invalid number".into(),
            })
    }

    fn hex4(&mut self) -> Result<u32, JsonError> {
        let digits = self
            .src
            .get(self.pos..self.pos + 4)
            .ok_or_else(|| self.err("short \\u escape"))?;
        let code = u32::from_str_radix(digits, 16).map_err(|_| self.err("bad \\u escape"))?;
        self.pos += 4;
        Ok(code)
    }

    fn string(&mut self) -> Result<String, JsonError> {
        self.pos += 1; // opening quote
        let mut out = String::new();
        loop {
            // Copy the run of ordinary characters in one go.
            let run_start = self.pos;
            while let Some(b) = self.peek() {
                if b == b'"' || b == b'\\' || b < 0x20 {
                    break;
                }
                self.pos += 1;
            }
            out.push_str(&self.src[run_start..self.pos]);
            match self.peek() {
                Some(b'"') => {
                    self.pos += 1;
                    return Ok(out);
                }
                Some(b'\\') => {
                    self.pos += 1;
                    let esc = self.peek().ok_or_else(|| self.err("unterminated escape"))?;
                    self.pos += 1;
                    match esc {
                        b'"' => out.push('"'),
                        b'\\' => out.push('\\'),
                        b'/' => out.push('/'),
                        b'b' => out.push('\u{8}'),
                        b'f' => out.push('\u{c}'),
                        b'n' => out.push('\n'),
                        b'r' => out.push('\r'),
                        b't' => out.push('\t'),
                        b'u' => {
                            let first = self.hex4()?;
                            let code = if (0xD800..0xDC00).contains(&first) {
                                if !self.src[self.pos..].starts_with("\\u") {
                                    return Err(self.err("unpaired surrogate"));
                                }
                                self.pos += 2;
                                let second = self.hex4()?;
                                if !(0xDC00..0xE000).contains(&second) {
                                    return Err(self.err("invalid low surrogate"));
                                }
                                0x10000 + ((first - 0xD800) << 10) + (second - 0xDC00)
                            } else {
                                first
                            };
                            out.push(
                                char::from_u32(code)
                                    .ok_or_else(|| self.err("invalid code point"))?,
                            );
                        }
                        _ => return Err(self.err("unknown escape")),
                    }
                }
                Some(_) => return Err(self.err("control character in string")),
                None => return Err(self.err("unterminated string")),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_nested_values() {
        let v = parse(r#" {"a": [1, -2.5e2, true, null], "b": {"c": "d"}, "e": []} "#).unwrap();
        assert_eq!(v.get("a").unwrap().as_array().unwrap().len(), 4);
        assert_eq!(
            v.get("a").unwrap().as_array().unwrap()[1].as_f64(),
            Some(-250.0)
        );
        assert_eq!(v.get("b").unwrap().get("c").unwrap().as_str(), Some("d"));
        assert_eq!(v.get("e").unwrap().as_array().unwrap().len(), 0);
        assert!(v.get("missing").is_none());
    }

    #[test]
    fn decodes_escapes_and_unicode() {
        let v = parse(r#""a\"b\\c\/d\n\u00e9\ud83d\ude00 caf\u00E9""#).unwrap();
        assert_eq!(v.as_str(), Some("a\"b\\c/d\né😀 café"));
        assert_eq!(
            parse("\"Société Générale\"").unwrap().as_str(),
            Some("Société Générale")
        );
    }

    #[test]
    fn rejects_malformed_input() {
        for bad in [
            "{",
            "[1,]",
            "{\"a\" 1}",
            "tru",
            "\"\\ud800\"",
            "1 2",
            "\"a\nb\"",
        ] {
            assert!(parse(bad).is_err(), "should reject {bad:?}");
        }
    }
}
