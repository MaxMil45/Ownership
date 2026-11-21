//! `oe dashboard`: one self-contained HTML page with charts and a searchable
//! list of everything the rules found in the downloaded folders.
//!
//! The data is embedded in the page as JSON; the page needs no server and no
//! internet connection, and sends nothing anywhere. Open it in a browser and
//! run the command again to refresh it.

use super::alerts::{build_alerts, source_flag, Sources};
use super::{find_files, find_xml_files, load};
use oe_alerts::render::{json, json_number, json_string};
use oe_alerts::select::{eastern_date_from_unix, now_unix, utc_timestamp};
use std::path::{Path, PathBuf};

pub const DEFAULT_OUT: &str = "data/report/dashboard.html";
const TEMPLATE: &str = include_str!("dashboard.html");
const PLACEHOLDER: &str = "/*__OE_DATA__*/null";

/// Every rule the page knows, in the fixed order that gives each its color.
pub const RULES: &[(&str, &str)] = &[
    ("insider.cluster_buy@1", "Cluster buy"),
    ("insider.planned_sale@1", "Planned sale (Form 144)"),
    ("insider.plan_terminated@1", "10b5-1 plan ended"),
    ("activist.new_13d@1", "New 13D"),
    ("activist.passive_to_active@1", "Passive to active"),
    ("activist.proxy_contest@1", "Proxy contest"),
];

#[derive(Debug, PartialEq)]
pub struct DashboardArgs {
    pub sources: Sources,
    pub out: PathBuf,
    /// Add the history chart from the PostgreSQL ledger (`DATABASE_URL`).
    pub db: bool,
}

pub fn parse_args(args: &[String]) -> Result<DashboardArgs, String> {
    let mut out = PathBuf::from(DEFAULT_OUT);
    let mut db = false;
    let mut only = None;
    let mut it = args.iter();
    while let Some(arg) = it.next() {
        if source_flag(arg, &mut it, &mut only)? {
            continue;
        }
        match arg.as_str() {
            "--out" => out = super::args::file(&mut it, "--out")?,
            "--db" => db = true,
            other => return Err(format!("unexpected argument {other:?}")),
        }
    }
    Ok(DashboardArgs {
        sources: only.unwrap_or_else(Sources::defaults),
        out,
        db,
    })
}

/// The ledger's daily buying and selling as compact JSON rows
/// `[day, buy value, buys, sell value, sells]`.
#[cfg(feature = "db")]
fn history_json() -> Result<(String, usize), String> {
    let url = std::env::var("DATABASE_URL")
        .map_err(|_| "--db needs DATABASE_URL (see `oe db-status`)".to_string())?;
    let mut client = oe_store::connect(&url).map_err(|e| e.to_string())?;
    let (days, left_out) = oe_store::flow_by_day(&mut client).map_err(|e| e.to_string())?;
    let rows: Vec<String> = days
        .iter()
        .map(|d| {
            format!(
                "[{},{},{},{},{}]",
                json_string(&d.day),
                json_number(Some(d.buy_value)),
                d.buys,
                json_number(Some(d.sell_value)),
                d.sells
            )
        })
        .collect();
    Ok((
        format!("{{\"days\":[{}],\"left_out\":{left_out}}}", rows.join(",")),
        days.len(),
    ))
}

#[cfg(not(feature = "db"))]
fn history_json() -> Result<(String, usize), String> {
    Err("this build has no database support; leave out --db".into())
}

/// Open-market buys and sales from the Form 4 folder, as compact JSON
/// objects: `d` known on, `t` trade date, `i` issuer CIK, `k` ticker,
/// `o` owner, `r` role, `c` code (P or S), `v` value, `s` shares, `p` price,
/// `a` accession.
fn trade_rows(folder: &Path) -> Result<Vec<String>, String> {
    let trades = load::insider_trades(folder)?;
    let mut rows = Vec::new();
    for t in &trades.items {
        // Amendment rows repeat their original's trades; leave them out so a
        // trade is not counted twice.
        if t.from_amendment {
            continue;
        }
        let is_buy = t.is_open_market_purchase;
        let is_sale = t.code.as_str() == "S";
        if !(is_buy || is_sale) {
            continue;
        }
        let (Some(value), Some(known_on)) = (t.value(), t.known_on) else {
            continue;
        };
        // Left out like the ledger's totals: filing errors such as the total
        // typed into the price field (oe_core::MAX_PLAUSIBLE_PRICE).
        if !value.is_finite()
            || value <= 0.0
            || !oe_core::plausible_amounts(t.price_per_share, Some(value))
        {
            continue;
        }
        let role = match (&t.officer_title, t.is_director) {
            (Some(title), _) => title.clone(),
            (None, true) => "Director".to_string(),
            (None, false) => String::new(),
        };
        rows.push(format!(
            "{{\"d\":{},\"t\":{},\"i\":{},\"k\":{},\"o\":{},\"r\":{},\"c\":{},\"v\":{},\"s\":{},\"p\":{},\"a\":{}}}",
            json_string(&known_on.to_string()),
            json_string(&t.date.to_string()),
            t.issuer.0,
            t.ticker.as_deref().map_or("null".into(), json_string),
            json_string(&t.owner_name),
            json_string(&role),
            json_string(if is_buy { "P" } else { "S" }),
            json_number(Some(value)),
            json_number(t.shares),
            json_number(t.price_per_share),
            t.accession.as_deref().map_or("null".into(), json_string),
        ));
    }
    Ok(rows)
}

fn count(dir: &Option<PathBuf>, ext: &str) -> Result<usize, String> {
    match dir {
        Some(d) if ext == "xml" => Ok(find_xml_files(d)?.len()),
        Some(d) => Ok(find_files(d, ext)?.len()),
        None => Ok(0),
    }
}

/// The page's data as one JSON object.
/// `history` is the ledger's daily flow ([`history_json`]), if wanted;
/// `db_history` gives the cluster-buy routine filter the ledger's history.
pub fn data_json(
    sources: &Sources,
    generated_unix: u64,
    history: Option<&str>,
    db_history: bool,
) -> Result<(String, usize, usize), String> {
    let (mut alerts, _) = build_alerts(sources, db_history)?;
    // One entry per alert key, as `oe alerts` delivers them (one person's two
    // plans ended in one report are one event).
    let mut seen = std::collections::HashSet::new();
    alerts.retain(|a| seen.insert(a.key.clone()));
    let trades = match &sources.form4 {
        Some(dir) => trade_rows(dir)?,
        None => Vec::new(),
    };
    let rules: Vec<String> = RULES
        .iter()
        .map(|(id, label)| {
            format!(
                "{{\"id\":{},\"label\":{}}}",
                json_string(id),
                json_string(label)
            )
        })
        .collect();
    let counts = format!(
        "{{\"form4\":{},\"form144\":{},\"sched13\":{},\"proxy\":{},\"periodic\":{}}}",
        count(&sources.form4, "xml")?,
        count(&sources.form144, "xml")?,
        count(&sources.sched13, "xml")?,
        count(&sources.proxy, "sgml")?,
        count(&sources.periodic, "htm")?,
    );
    let alert_json: Vec<String> = alerts.iter().map(json).collect();
    let data = format!(
        "{{\"generated\":{},\"today\":{},\"counts\":{},\"rules\":[{}],\"alerts\":[{}],\"trades\":[{}],\"history\":{}}}",
        json_string(&utc_timestamp(generated_unix)),
        json_string(&eastern_date_from_unix(generated_unix).to_string()),
        counts,
        rules.join(","),
        alert_json.join(","),
        trades.join(","),
        history.unwrap_or("null")
    );
    Ok((data, alerts.len(), trades.len()))
}

/// The template with the data put in. Inside a `<script>` element the text
/// `</` would end the script early, so it is written `<\/`, which JSON reads
/// as the same characters; U+2028 and U+2029 are escaped for old browsers.
pub fn page(data: &str) -> String {
    let safe = data
        .replace("</", "<\\/")
        .replace('\u{2028}', "\\u2028")
        .replace('\u{2029}', "\\u2029");
    TEMPLATE.replacen(PLACEHOLDER, &safe, 1)
}

pub fn dashboard(args: &[String]) -> Result<(), String> {
    let args = parse_args(args)?;
    let history = if args.db {
        let (json, days) = history_json()?;
        println!("{days} day(s) of insider buying and selling from the database");
        Some(json)
    } else {
        None
    };
    let (data, alerts, trades) = data_json(&args.sources, now_unix(), history.as_deref(), args.db)?;
    if let Some(dir) = args.out.parent().filter(|d| !d.as_os_str().is_empty()) {
        std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    }
    std::fs::write(&args.out, page(&data)).map_err(|e| format!("{}: {e}", args.out.display()))?;
    let full = std::fs::canonicalize(&args.out).unwrap_or_else(|_| args.out.clone());
    let shown = full.display().to_string();
    let shown = shown.strip_prefix(r"\\?\").unwrap_or(&shown);
    println!("{alerts} event(s) and {trades} insider buy/sale row(s) written to {shown}");
    println!("open it in your browser; run `oe dashboard` again to refresh it");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_template_has_exactly_one_placeholder() {
        assert_eq!(TEMPLATE.matches(PLACEHOLDER).count(), 1);
    }

    #[test]
    fn data_cannot_close_the_script_element() {
        let p = page("{\"x\":\"</script><script>alert(1)</script>\"}");
        assert!(!p.contains("\"</script><script>alert(1)"));
        assert!(p.contains("<\\/script><script>alert(1)<\\/script>"));
        assert!(!p.contains(PLACEHOLDER));
    }

    #[test]
    fn real_fixtures_make_a_page_with_every_rule() {
        let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures");
        let sources = Sources {
            form4: Some(root.join("form4/real")),
            form144: Some(root.join("form144/real")),
            sched13: Some(root.join("sched13/real")),
            proxy: Some(root.join("proxy/real")),
            periodic: Some(root.join("item408/real")),
        };
        let (data, alerts, trades) = data_json(&sources, 1_795_188_104, None, false).unwrap();
        assert!(data.ends_with(",\"history\":null}"));
        let (with, _, _) = data_json(
            &sources,
            1_795_188_104,
            Some("{\"days\":[[\"2015-01-02\",10,1,20,2]],\"left_out\":0}"),
            false,
        )
        .unwrap();
        assert!(
            with.ends_with(",\"history\":{\"days\":[[\"2015-01-02\",10,1,20,2]],\"left_out\":0}}")
        );
        assert!(alerts > 20, "{alerts} alerts");
        assert!(trades > 0, "the fixtures hold open-market buys and sales");
        assert!(
            data.starts_with("{\"generated\":\"2026-11-20T15:21:44Z\",\"today\":\"2026-11-20\"")
        );
        assert!(data.contains("\"counts\":{\"form4\":25,"));
        for (id, _) in RULES
            .iter()
            .filter(|(id, _)| *id != "insider.cluster_buy@1")
        {
            assert!(data.contains(&format!("\"rule_id\":\"{id}\"")), "{id}");
        }
        // Every trade row has a value and a known-on date.
        assert!(!data.contains("\"v\":null"));
        // The ARS report's repeated plans appear once per person.
        assert_eq!(
            data.matches(
                "\"key\":\"insider.plan_terminated@1|0001193125-26-449778|Richard Lowenthal\""
            )
            .count(),
            1
        );
    }

    #[test]
    fn args() {
        let a = parse_args(&[
            "--out".into(),
            "x.html".into(),
            "--form4".into(),
            "f".into(),
        ])
        .unwrap();
        assert_eq!(a.out, PathBuf::from("x.html"));
        assert_eq!(a.sources.form4, Some(PathBuf::from("f")));
        assert_eq!(a.sources.form144, None);
        assert!(!a.db);
        assert!(parse_args(&["--db".into()]).unwrap().db);
        assert!(parse_args(&["--out".into()]).is_err());
        assert!(parse_args(&["--nope".into()]).is_err());
    }
}
