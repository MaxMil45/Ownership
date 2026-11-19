//! Small text helpers that every SEC form parser needs: yes/no flags, numbers
//! as forms write them, entity references and markup tags. One copy, here, so
//! the parsers agree on what `"Y"` or `"1,250,000.00"` means.

/// `1` / `true` / `y` / `yes` → true, `0` / `false` / `n` / `no` → false
/// (ignoring case and surrounding spaces). Anything else is `None`.
pub fn parse_flag(text: &str) -> Option<bool> {
    match text.trim().to_ascii_lowercase().as_str() {
        "1" | "true" | "y" | "yes" => Some(true),
        "0" | "false" | "n" | "no" => Some(false),
        _ => None,
    }
}

/// A number as SEC forms write it: thousands separators, `$` and `%` signs and
/// surrounding spaces are ignored. `None` if what is left is empty or is not a
/// finite number (so `"NaN"` and `"inf"` are rejected, not read).
pub fn parse_decimal(text: &str) -> Option<f64> {
    let cleaned: String = text
        .chars()
        .filter(|c| !matches!(c, ',' | '$' | '%'))
        .collect();
    cleaned.trim().parse::<f64>().ok().filter(|v| v.is_finite())
}

/// The character an entity stands for, given its name without `&` and `;`:
/// the five XML entities and numeric references (`#8217`, `#x41`).
pub fn entity_char(name: &str) -> Option<char> {
    match name {
        "amp" => Some('&'),
        "lt" => Some('<'),
        "gt" => Some('>'),
        "quot" => Some('"'),
        "apos" => Some('\''),
        _ => name
            .strip_prefix("#x")
            .or_else(|| name.strip_prefix("#X"))
            .and_then(|h| u32::from_str_radix(h, 16).ok())
            .or_else(|| name.strip_prefix('#').and_then(|d| d.parse().ok()))
            .and_then(char::from_u32),
    }
}

/// Decodes entities leniently, as HTML readers must: the XML entities,
/// numeric references and `&nbsp;` (as a space). Anything unknown, or an `&`
/// with no `;` soon after it, is left as written.
pub fn decode_entities_lenient(text: &str) -> String {
    if !text.contains('&') {
        return text.to_string();
    }
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(amp) = rest.find('&') {
        out.push_str(&rest[..amp]);
        rest = &rest[amp..];
        let decoded = rest.find(';').filter(|&semi| semi <= 10).and_then(|semi| {
            let name = &rest[1..semi];
            let c = if name == "nbsp" {
                Some(' ')
            } else {
                entity_char(name)
            };
            c.map(|c| (c, semi))
        });
        match decoded {
            Some((c, semi)) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

/// The `key=value` lines of a `.meta.txt` sidecar, trimmed; lines without`n/// `=` and empty values are skipped.
pub fn key_values(text: &str) -> impl Iterator<Item = (&str, &str)> {
    text.lines().filter_map(|line| {
        let (key, value) = line.split_once('=')?;
        let value = value.trim();
        (!value.is_empty()).then(|| (key.trim(), value))
    })
}

/// Byte index of the `>` that closes the tag starting at `s[0]` (`<`),
/// ignoring any `>` inside quoted attribute values. `None` if it never closes.
pub fn tag_end(s: &str) -> Option<usize> {
    let mut quote: Option<char> = None;
    for (i, c) in s.char_indices().skip(1) {
        match (quote, c) {
            (None, '"') | (None, '\'') => quote = Some(c),
            (Some(q), c) if c == q => quote = None,
            (None, '>') => return Some(i),
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn flags() {
        for yes in ["1", "true", "Y", " yes ", "TRUE"] {
            assert_eq!(parse_flag(yes), Some(true), "{yes:?}");
        }
        for no in ["0", "false", "n", "No"] {
            assert_eq!(parse_flag(no), Some(false), "{no:?}");
        }
        for neither in ["", "maybe", "2", "t"] {
            assert_eq!(parse_flag(neither), None, "{neither:?}");
        }
    }

    #[test]
    fn decimals_as_forms_write_them() {
        assert_eq!(parse_decimal("1,250,000.00"), Some(1_250_000.0));
        assert_eq!(parse_decimal(" $12.8125 "), Some(12.8125));
        assert_eq!(parse_decimal("5.4%"), Some(5.4));
        assert_eq!(parse_decimal("-3"), Some(-3.0));
        for bad in ["", "  ", "n/a", "lots", "NaN", "inf", "1.2.3"] {
            assert_eq!(parse_decimal(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn entities() {
        assert_eq!(entity_char("amp"), Some('&'));
        assert_eq!(entity_char("#8217"), Some('\u{2019}'));
        assert_eq!(entity_char("#x41"), Some('A'));
        assert_eq!(entity_char("#X41"), Some('A'));
        assert_eq!(entity_char("nbsp"), None, "not an XML entity");
        assert_eq!(entity_char("#xZZ"), None);
        assert_eq!(
            decode_entities_lenient("A &amp; B &lt;c&gt; &quot;d&quot; &#8217; &#x41; &nbsp;x"),
            "A & B <c> \"d\" \u{2019} A  x"
        );
        assert_eq!(
            decode_entities_lenient("fish & chips &unknown; &#zzz;"),
            "fish & chips &unknown; &#zzz;"
        );
    }

    #[test]
    fn sidecar_key_values() {
        let text =
            "accession = 0001-26-000001\nform=4\nknown_on=\nno equals sign\nurl=https://x/y?a=b\n";
        let pairs: Vec<_> = key_values(text).collect();
        assert_eq!(
            pairs,
            [
                ("accession", "0001-26-000001"),
                ("form", "4"),
                ("url", "https://x/y?a=b")
            ]
        );
    }

    #[test]
    fn tag_ends_skip_quoted_greater_than_signs() {
        assert_eq!(tag_end("<a b=\"x>y\">rest"), Some(10));
        assert_eq!(tag_end("<a b='>'/>"), Some(9));
        assert_eq!(tag_end("<br>"), Some(3));
        assert_eq!(tag_end("<never closed"), None);
    }
}
