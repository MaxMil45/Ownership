//! Alert text and JSON. Pure.

use crate::Alert;

/// Discord rejects messages over 2,000 characters.
pub const DISCORD_LIMIT: usize = 2000;

/// Plain text: headline, facts, receipts. Also used on the console.
pub fn text(a: &Alert) -> String {
    let mut out = String::new();
    out.push_str(&a.headline);
    out.push('\n');
    for d in &a.details {
        out.push_str("  ");
        out.push_str(d);
        out.push('\n');
    }
    out.push_str(&format!(
        "  Rule {} · known on {}\n",
        a.rule_id,
        a.known_on
            .map_or("an unknown date".to_string(), |d| d.to_string())
    ));
    for r in &a.receipts {
        out.push_str(&format!(
            "  Receipt: {}{} {}\n",
            r.form
                .as_deref()
                .map_or(String::new(), |f| format!("Form {f} ")),
            r.accession,
            r.url
        ));
    }
    out
}

/// A Discord message: bold headline, the facts, links without previews
/// (`<url>`), cut to fit the limit.
pub fn discord(a: &Alert) -> String {
    let mut out = format!("**{}**\n", escape_markdown(&a.headline));
    for d in &a.details {
        out.push_str(&escape_markdown(d));
        out.push('\n');
    }
    out.push_str(&format!(
        "-# {} · known on {}\n",
        a.rule_id,
        a.known_on.map_or("?".to_string(), |d| d.to_string())
    ));
    let mut receipts = String::new();
    for (i, r) in a.receipts.iter().enumerate() {
        let line = format!("Receipt: <{}>\n", r.url);
        if out.chars().count() + receipts.chars().count() + line.chars().count()
            > DISCORD_LIMIT - 40
        {
            receipts.push_str(&format!(
                "... and {} more receipt(s)\n",
                a.receipts.len() - i
            ));
            break;
        }
        receipts.push_str(&line);
    }
    out.push_str(&receipts);
    truncate(&out, DISCORD_LIMIT)
}

fn escape_markdown(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        if matches!(c, '*' | '_' | '~' | '`' | '|' | '>' | '#') {
            out.push('\\');
        }
        out.push(c);
    }
    out
}

fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut t: String = s.chars().take(max - 1).collect();
    t.push('…');
    t
}

/// A JSON string literal.
pub fn json_string(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if (c as u32) < 0x20 => out.push_str(&format!("\\u{:04x}", c as u32)),
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

fn opt_string(v: Option<String>) -> String {
    v.map_or_else(|| "null".to_string(), |s| json_string(&s))
}

/// The alert as one JSON object, for generic webhooks. Field names are part
/// of the public interface: add fields, never rename them.
pub fn json(a: &Alert) -> String {
    let details: Vec<String> = a.details.iter().map(|d| json_string(d)).collect();
    let people: Vec<String> = a
        .people
        .iter()
        .map(|p| {
            format!(
                "{{\"cik\":{},\"name\":{}}}",
                p.cik.map_or("null".to_string(), |c| c.0.to_string()),
                json_string(&p.name)
            )
        })
        .collect();
    let receipts: Vec<String> = a
        .receipts
        .iter()
        .map(|r| {
            format!(
                "{{\"accession\":{},\"form\":{},\"url\":{}}}",
                json_string(&r.accession),
                opt_string(r.form.clone()),
                json_string(&r.url)
            )
        })
        .collect();
    format!(
        "{{\"key\":{},\"rule_id\":{},\"known_on\":{},\"issuer_cik\":{},\"issuer_name\":{},\"ticker\":{},\"headline\":{},\"details\":[{}],\"people\":[{}],\"receipts\":[{}],\"text\":{}}}",
        json_string(&a.key),
        json_string(a.rule_id),
        opt_string(a.known_on.map(|d| d.to_string())),
        a.issuer_cik.map_or("null".to_string(), |c| c.0.to_string()),
        opt_string(a.issuer_name.clone()),
        opt_string(a.ticker.clone()),
        json_string(&a.headline),
        details.join(","),
        people.join(","),
        receipts.join(","),
        json_string(&text(a))
    )
}

/// The body Discord's webhook API expects. Mentions are switched off so an
/// insider's name can never ping anyone.
pub fn discord_payload(a: &Alert) -> String {
    format!(
        "{{\"content\":{},\"allowed_mentions\":{{\"parse\":[]}}}}",
        json_string(&discord(a))
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{receipt, Person};
    use oe_core::Cik;

    fn alert() -> Alert {
        Alert {
            key: "insider.cluster_buy@1|320193|2026-11-02".into(),
            rule_id: "insider.cluster_buy@1",
            known_on: Some("2026-11-04".parse().unwrap()),
            issuer_cik: Some(Cik(320193)),
            issuer_name: None,
            ticker: Some("AAPL".into()),
            headline: "Cluster buy at AAPL: 3 insiders, $1.25M".into(),
            details: vec!["Insiders: A \"Al\" B, C_D".into()],
            people: vec![Person {
                cik: Some(Cik(7)),
                name: "A \"Al\" B".into(),
            }],
            receipts: vec![receipt(Cik(320193), "0000320193-26-000001", Some("4"))],
        }
    }

    #[test]
    fn text_has_headline_facts_rule_and_receipts() {
        let t = text(&alert());
        assert!(t.starts_with("Cluster buy at AAPL"));
        assert!(t.contains("known on 2026-11-04"));
        assert!(t.contains(
            "Receipt: Form 4 0000320193-26-000001 https://www.sec.gov/Archives/edgar/data/320193/000032019326000001/"
        ));
    }

    #[test]
    fn json_escapes_and_keeps_every_field() {
        let j = json(&alert());
        assert!(j.contains("\"key\":\"insider.cluster_buy@1|320193|2026-11-02\""));
        assert!(j.contains("\"issuer_cik\":320193"));
        assert!(j.contains("\"issuer_name\":null"));
        assert!(j.contains("\"name\":\"A \\\"Al\\\" B\""));
        assert!(j.contains("\"form\":\"4\""));
        assert!(
            j.contains("\\n"),
            "the text field keeps its line breaks escaped"
        );
        assert!(!j.contains('\n'));
        assert_eq!(json_string("a\u{1}b"), "\"a\\u0001b\"");
    }

    #[test]
    fn discord_messages_escape_markdown_disable_mentions_and_fit() {
        let d = discord(&alert());
        assert!(d.starts_with("**Cluster buy at AAPL"));
        assert!(d.contains("C\\_D"));
        assert!(d.contains("<https://www.sec.gov/"));
        assert!(discord_payload(&alert()).contains("\"allowed_mentions\":{\"parse\":[]}"));

        let mut big = alert();
        big.details = vec!["x".repeat(300); 10];
        big.receipts = (0..40)
            .map(|i| receipt(Cik(1), &format!("0000000001-26-{i:06}"), None))
            .collect();
        let d = discord(&big);
        assert!(d.chars().count() <= DISCORD_LIMIT);
        assert!(d.contains("more receipt(s)") || d.ends_with('…'));
    }
}
