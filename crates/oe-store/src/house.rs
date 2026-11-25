//! The transactions read from House members' reports (migration
//! `0011_house_transactions`, ADR 0011). Derived from the PDFs: every load
//! replaces them. Use is limited by 5 U.S.C. 13107 (no commercial use).

use crate::db::{check_schema, StoreError};
use crate::rows::copy_field;
use postgres::types::ToSql;
use postgres::Client;
use std::io::Write;

/// One transaction ready to store.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HouseTxRow {
    pub line_no: i32,
    pub owner: Option<String>,
    pub asset: String,
    pub ticker: Option<String>,
    pub asset_type: Option<String>,
    pub tx_type: String,
    pub tx_date: Option<String>,
    pub notified_on: Option<String>,
    pub amount: String,
    pub amount_min: Option<u64>,
    pub amount_max: Option<u64>,
    pub filing_status: Option<String>,
    pub description: Option<String>,
    pub comments: Option<String>,
    pub subholding_of: Option<String>,
    pub issuer_cik: Option<i64>,
}

/// One report as read.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct HouseRead {
    pub doc_id: String,
    pub rows: Vec<HouseTxRow>,
    pub problems: Vec<String>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HouseLoadReport {
    pub reports: u64,
    pub transactions: u64,
    pub with_issuer: u64,
}

fn opt(v: &Option<String>) -> String {
    v.as_deref().map_or_else(|| "\\N".to_string(), copy_field)
}

fn optn<T: ToString>(v: Option<T>) -> String {
    v.map_or_else(|| "\\N".to_string(), |x| x.to_string())
}

/// Replaces every stored transaction with `reads`, in one transaction.
/// Reports not in `house_report` are skipped.
pub fn replace_house_transactions(
    client: &mut Client,
    reads: &[HouseRead],
    reader: &str,
) -> Result<HouseLoadReport, StoreError> {
    check_schema(client)?;
    let known: std::collections::HashSet<String> = client
        .query("SELECT doc_id FROM house_report", &[])?
        .iter()
        .map(|r| r.get(0))
        .collect();
    let mut tx = client.transaction()?;
    tx.batch_execute("TRUNCATE house_transaction, house_report_read")?;
    let mut report = HouseLoadReport::default();
    {
        let mut w = tx.copy_in(
            "COPY house_transaction (doc_id, line_no, owner, asset, ticker, asset_type, tx_type, tx_date,
                 notified_on, amount, amount_min, amount_max, filing_status, description, comments,
                 subholding_of, issuer_cik) FROM STDIN",
        )?;
        for r in reads.iter().filter(|r| known.contains(&r.doc_id)) {
            for t in &r.rows {
                let line = [
                    copy_field(&r.doc_id),
                    t.line_no.to_string(),
                    opt(&t.owner),
                    copy_field(&t.asset),
                    opt(&t.ticker),
                    opt(&t.asset_type),
                    copy_field(&t.tx_type),
                    opt(&t.tx_date),
                    opt(&t.notified_on),
                    copy_field(&t.amount),
                    optn(t.amount_min),
                    optn(t.amount_max),
                    opt(&t.filing_status),
                    opt(&t.description),
                    opt(&t.comments),
                    opt(&t.subholding_of),
                    optn(t.issuer_cik),
                ]
                .join("\t");
                w.write_all(line.as_bytes())?;
                w.write_all(b"\n")?;
                report.transactions += 1;
                if t.issuer_cik.is_some() {
                    report.with_issuer += 1;
                }
            }
        }
        w.finish()?;
    }
    {
        let mut w = tx.copy_in(
            "COPY house_report_read (doc_id, transactions, problems, reader) FROM STDIN",
        )?;
        for r in reads.iter().filter(|r| known.contains(&r.doc_id)) {
            let problems = if r.problems.is_empty() {
                None
            } else {
                Some(r.problems.join("; "))
            };
            w.write_all(
                format!(
                    "{}\t{}\t{}\t{}\n",
                    copy_field(&r.doc_id),
                    r.rows.len(),
                    opt(&problems),
                    copy_field(reader)
                )
                .as_bytes(),
            )?;
            report.reports += 1;
        }
        w.finish()?;
    }
    tx.commit()?;
    Ok(report)
}

/// The typed reports to read: (doc id, year).
pub fn house_reports_to_read(client: &mut Client) -> Result<Vec<(String, i32)>, StoreError> {
    check_schema(client)?;
    Ok(client
        .query(
            "SELECT doc_id, year FROM house_report WHERE NOT scanned AND filing_type = 'P' ORDER BY year, doc_id",
            &[],
        )?
        .iter()
        .map(|r| (r.get(0), r.get(1)))
        .collect())
}

/// One member's trade with where it was reported.
#[derive(Debug, Clone, PartialEq)]
pub struct OfficialTrade {
    pub bioguide: Option<String>,
    pub member: String,
    pub doc_id: String,
    pub line_no: i32,
    pub filed_on: Option<String>,
    pub report_url: String,
    pub owner: Option<String>,
    pub asset: String,
    pub ticker: Option<String>,
    pub asset_type: Option<String>,
    pub tx_type: String,
    pub tx_date: Option<String>,
    pub amount: String,
    pub amount_min: Option<i64>,
    pub amount_max: Option<i64>,
    pub description: Option<String>,
    pub issuer_cik: Option<i64>,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct OfficialTradeFilter {
    pub bioguide: Option<String>,
    pub ticker: Option<String>,
    pub issuer_cik: Option<i64>,
    /// `buy` or `sale`.
    pub side: Option<String>,
    pub since: Option<String>,
    pub limit: i64,
    pub offset: i64,
}

/// Trades, newest first, and how many match in all.
pub fn official_trades(
    client: &mut Client,
    f: &OfficialTradeFilter,
) -> Result<(i64, Vec<OfficialTrade>), StoreError> {
    check_schema(client)?;
    let mut clauses: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn ToSql + Sync>> = Vec::new();
    let mut add = |c: &str, v: Box<dyn ToSql + Sync>, clauses: &mut Vec<String>| {
        params.push(v);
        clauses.push(c.replace('?', &format!("${}", params.len())));
    };
    if let Some(b) = &f.bioguide {
        add("l.bioguide_id = ?", Box::new(b.clone()), &mut clauses);
    }
    if let Some(t) = &f.ticker {
        add(
            "upper(t.ticker) = upper(?)",
            Box::new(t.clone()),
            &mut clauses,
        );
    }
    if let Some(c) = f.issuer_cik {
        add("t.issuer_cik = ?", Box::new(c), &mut clauses);
    }
    if let Some(s) = &f.since {
        add(
            "t.tx_date >= ?::text::date",
            Box::new(s.clone()),
            &mut clauses,
        );
    }
    match f.side.as_deref() {
        Some("buy") => clauses.push("t.tx_type = 'P'".into()),
        Some("sale") => clauses.push("t.tx_type IN ('S', 'S (partial)')".into()),
        _ => {}
    }
    let filter = if clauses.is_empty() {
        String::new()
    } else {
        format!("WHERE {}", clauses.join(" AND "))
    };
    let from = "FROM house_transaction t
                JOIN house_report r USING (doc_id)
                LEFT JOIN house_report_link l USING (doc_id)
                LEFT JOIN official o ON o.bioguide_id = l.bioguide_id";
    let refs: Vec<&(dyn ToSql + Sync)> = params.iter().map(|b| b.as_ref()).collect();
    let total: i64 = client
        .query_one(&format!("SELECT count(*) {from} {filter}"), &refs)?
        .get(0);
    let n = refs.len();
    let mut page = refs.clone();
    page.push(&f.limit);
    page.push(&f.offset);
    let rows = client.query(
        &format!(
            "SELECT l.bioguide_id,
                    coalesce(o.official_full, o.first_name || ' ' || o.last_name,
                             trim(r.first_name || ' ' || r.last_name)),
                    t.doc_id, t.line_no, r.filed_on::text, r.source_url, t.owner, t.asset, t.ticker,
                    t.asset_type, t.tx_type, t.tx_date::text, t.amount, t.amount_min, t.amount_max,
                    t.description, t.issuer_cik
             {from} {filter}
             ORDER BY t.tx_date DESC NULLS LAST, r.filed_on DESC NULLS LAST, t.doc_id DESC, t.line_no
             LIMIT ${} OFFSET ${}",
            n + 1,
            n + 2
        ),
        &page,
    )?;
    Ok((
        total,
        rows.iter()
            .map(|r| OfficialTrade {
                bioguide: r.get(0),
                member: r.get::<_, Option<String>>(1).unwrap_or_default(),
                doc_id: r.get(2),
                line_no: r.get(3),
                filed_on: r.get(4),
                report_url: r.get(5),
                owner: r.get(6),
                asset: r.get(7),
                ticker: r.get(8),
                asset_type: r.get(9),
                tx_type: r.get(10),
                tx_date: r.get(11),
                amount: r.get(12),
                amount_min: r.get(13),
                amount_max: r.get(14),
                description: r.get(15),
                issuer_cik: r.get(16),
            })
            .collect(),
    ))
}

/// A member's trading in numbers.
#[derive(Debug, Clone, PartialEq)]
pub struct MemberActivity {
    pub bioguide: String,
    pub name: String,
    pub trades: i64,
    pub buys: i64,
    pub sales: i64,
    /// Sum of the ranges' lower and upper ends (an upper end is missing for
    /// open top bands; then only the lower end counts).
    pub low_total: i64,
    pub high_total: i64,
    pub first_trade: Option<String>,
    pub last_trade: Option<String>,
    /// The most traded tickers, most first.
    pub top_tickers: Vec<String>,
}

/// Members ranked by number of trades since `since` (all if None).
pub fn member_activity(
    client: &mut Client,
    since: Option<&str>,
    limit: i64,
) -> Result<Vec<MemberActivity>, StoreError> {
    check_schema(client)?;
    let since = since.unwrap_or("1900-01-01").to_string();
    Ok(client
        .query(
            "WITH t AS (
                 SELECT l.bioguide_id, t.* FROM house_transaction t
                 JOIN house_report_link l USING (doc_id)
                 WHERE l.bioguide_id IS NOT NULL AND t.tx_date >= $1::text::date
             )
             SELECT t.bioguide_id,
                    coalesce(o.official_full, o.first_name || ' ' || o.last_name),
                    count(*), count(*) FILTER (WHERE tx_type = 'P'),
                    count(*) FILTER (WHERE tx_type IN ('S', 'S (partial)')),
                    coalesce(sum(amount_min), 0)::bigint, coalesce(sum(coalesce(amount_max, amount_min)), 0)::bigint,
                    min(tx_date)::text, max(tx_date)::text,
                    (SELECT array_agg(k) FROM (SELECT upper(x.ticker) AS k FROM t x
                       WHERE x.bioguide_id = t.bioguide_id AND x.ticker IS NOT NULL
                       GROUP BY 1 ORDER BY count(*) DESC, 1 LIMIT 5) y)
             FROM t JOIN official o USING (bioguide_id)
             GROUP BY t.bioguide_id, o.official_full, o.first_name, o.last_name
             ORDER BY count(*) DESC, t.bioguide_id
             LIMIT $2",
            &[&since, &limit],
        )?
        .iter()
        .map(|r| MemberActivity {
            bioguide: r.get(0),
            name: r.get::<_, Option<String>>(1).unwrap_or_default(),
            trades: r.get(2),
            buys: r.get(3),
            sales: r.get(4),
            low_total: r.get(5),
            high_total: r.get(6),
            first_trade: r.get(7),
            last_trade: r.get(8),
            top_tickers: r.get::<_, Option<Vec<String>>>(9).unwrap_or_default(),
        })
        .collect())
}
