//! `oe people-index` and `oe lookup <name>` (ADR 0010, 2): the name index and
//! what a name is in the ledger.

use oe_alerts::convert::{money, shares};
use oe_store::{Lookup, PersonCard};

const MAX_CANDIDATES: usize = 10;

fn connect() -> Result<oe_store::Client, String> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "DATABASE_URL is not set (see `oe db-status`)".to_string())?;
    oe_store::connect(&url).map_err(|e| e.to_string())
}

pub fn people_index(argv: &[String]) -> Result<(), String> {
    if let Some(a) = argv.first() {
        return Err(format!("unexpected argument {a:?}"));
    }
    let mut client = connect()?;
    let started = std::time::Instant::now();
    let r = oe_store::build_people_index(&mut client).map_err(|e| e.to_string())?;
    println!(
        "name index rebuilt in {:.0} s: {} candidates, {} tokens, {} names \
         (from {} insider, {} 13D/G holder and {} contest-filer name rows, {} members of Congress)",
        started.elapsed().as_secs_f64(),
        r.candidates,
        r.tokens,
        r.names,
        r.insiders,
        r.holders,
        r.contest_filers,
        r.officials
    );
    Ok(())
}

pub fn lookup(argv: &[String]) -> Result<(), String> {
    let query = argv.join(" ");
    if query.trim().is_empty() {
        return Err("lookup needs a name, for example: oe lookup john smith".into());
    }
    let mut client = connect()?;
    match oe_store::lookup(&mut client, &query, MAX_CANDIDATES).map_err(|e| e.to_string())? {
        Lookup::NeedsAName => println!("give at least one full word of the name"),
        Lookup::TooBroad(n) => println!(
            "{n} candidates for a single word: add a first name or initial (for example \"jane {}\")",
            query.trim().to_lowercase()
        ),
        Lookup::Found(cards) if cards.is_empty() => println!(
            "No public ownership role found for {query:?}. (This says nothing about the person.)"
        ),
        Lookup::Found(cards) => {
            for c in &cards {
                print_card(c);
            }
            if cards.iter().any(|c| c.official.is_some()) {
                println!(
                    "\nHouse reports may not be used commercially (5 U.S.C. 13107); see ADR 0010."
                );
            }
        }
    }
    Ok(())
}

fn print_card(c: &PersonCard) {
    println!(
        "\n{} [{}]{}  {}; {} filing(s), {} to {}",
        c.display_name,
        c.key,
        if c.exact { "" } else { " (partial name match)" },
        c.kinds.join(", "),
        c.filings,
        c.first_on.as_deref().unwrap_or("?"),
        c.last_on.as_deref().unwrap_or("?"),
    );
    if !c.other_names.is_empty() {
        let shown: Vec<&str> = c.other_names.iter().take(4).map(String::as_str).collect();
        println!("  also filed as: {}", shown.join("; "));
    }
    for r in &c.roles {
        let mut roles = Vec::new();
        if r.director {
            roles.push("director".to_string());
        }
        if r.officer {
            roles.push(match r.titles.first() {
                Some(t) => format!("officer ({t})"),
                None => "officer".to_string(),
            });
        }
        if r.ten_percent_owner {
            roles.push("10% owner".to_string());
        }
        if r.other {
            roles.push("other".to_string());
        }
        println!(
            "  {} at {}{} (CIK {}), {} to {}, {} filing(s)",
            if roles.is_empty() {
                "insider".to_string()
            } else {
                roles.join(", ")
            },
            r.issuer_name,
            r.ticker
                .as_deref()
                .map(|t| format!(" ({t})"))
                .unwrap_or_default(),
            r.issuer_cik,
            r.first_on,
            r.last_on,
            r.filings
        );
    }
    for t in &c.trades {
        let what = if t.code == "P" { "bought" } else { "sold" };
        let value = match (t.shares, t.price) {
            (Some(s), Some(p)) if oe_core::plausible_amounts(Some(p), Some(s * p)) => {
                format!(" for {}", money(s * p))
            }
            _ => String::new(),
        };
        println!(
            "    {} {what} {} shares of {}{value}; filed {}{}  {}",
            t.transaction_date,
            t.shares.map(shares).unwrap_or_else(|| "?".into()),
            t.ticker.as_deref().unwrap_or(&t.issuer_name),
            t.receipt.known_on,
            if t.receipt.filing_date_only {
                " (filing date)"
            } else {
                ""
            },
            t.receipt.url
        );
    }
    for h in &c.holdings {
        println!(
            "  {} of {} ({}){}; filed {}  {}",
            h.submission_type,
            h.issuer_name,
            h.class_title,
            h.percent.map(|p| format!(", {p}%")).unwrap_or_default(),
            h.receipt.known_on,
            h.receipt.url
        );
    }
    for k in &c.contests {
        println!(
            "  contested-proxy filing {} about {}; filed {}  {}",
            k.form, k.subject_name, k.receipt.known_on, k.receipt.url
        );
    }
    if let Some(o) = &c.official {
        for t in o.terms.iter().take(2) {
            println!("  {t}");
        }
        if !o.committees.is_empty() {
            let names: Vec<&str> = o.committees.iter().map(|(n, _, _)| n.as_str()).collect();
            println!("  committees: {}", names.join("; "));
        }
        if o.reports > 0 {
            println!(
                "  {} House periodic transaction report(s), {} to {}",
                o.reports,
                o.first_filed.as_deref().unwrap_or("?"),
                o.last_filed.as_deref().unwrap_or("?")
            );
            for (filed, _, scanned, url) in o.recent.iter().take(3) {
                println!(
                    "    {filed}{}  {url}",
                    if *scanned { " (scanned)" } else { "" }
                );
            }
        }
    }
}
