//! The transaction table, rebuilt from a report's lines (every year's House
//! form has the same columns: ID, Owner, Asset, Transaction Type, Date,
//! Notification Date, Amount, and from about 2020 Cap. Gains > $200?).
//!
//! A transaction starts on a line with a transaction type (P, S, S (partial),
//! E) and its dates. Below it, until the next one: more of the asset's name
//! (indented under the asset), the rest of the amount (under the amount), and
//! labelled lines (`FILING STATUS: New`, `DESCRIPTION: ...`, written `F S:`
//! and `D:` in later reports) with their continuations. Everything else
//! (headings, the certification, page footers) is ignored.

use crate::{Cell, Line};

/// One reported transaction, as written (dates as `YYYY-MM-DD`).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PtrLine {
    /// `SP` spouse, `JT` joint, `DC` dependent child; `None` the member.
    pub owner: Option<String>,
    pub asset: String,
    /// The ticker in brackets after the asset's name, upper case.
    pub ticker: Option<String>,
    /// The House's asset type code: `ST` stock, `OP` option, `GS`, `MF`, ...
    pub asset_type: Option<String>,
    /// `P` purchase, `S` sale, `S (partial)`, `E` exchange.
    pub tx_type: String,
    pub tx_date: Option<String>,
    pub notified_on: Option<String>,
    /// The amount range as written, e.g. `$1,001 - $15,000`.
    pub amount: String,
    pub amount_min: Option<u64>,
    /// `None` for an open top band ("Over $50,000,000").
    pub amount_max: Option<u64>,
    pub filing_status: Option<String>,
    pub description: Option<String>,
    pub comments: Option<String>,
    pub location: Option<String>,
    pub subholding_of: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default)]
pub struct PtrReport {
    pub lines: Vec<PtrLine>,
    /// Whether the transaction table's heading was found at all.
    pub has_table: bool,
    /// Transactions whose date, type or amount could not be read.
    pub problems: Vec<String>,
}

/// `$1,001 - $15,000` -> (1001, Some(15000)); `Over $50,000,000` ->
/// (50000001, None); `Spouse/DC Over $1,000,000` -> (1000001, None).
pub fn parse_amount(s: &str) -> Option<(u64, Option<u64>)> {
    let nums: Vec<u64> = s
        .split('$')
        .skip(1)
        .filter_map(|part| {
            let digits: String = part
                .chars()
                .take_while(|c| c.is_ascii_digit() || *c == ',' || *c == ' ')
                .filter(|c| c.is_ascii_digit())
                .collect();
            digits.parse().ok()
        })
        .collect();
    let over = s.to_ascii_lowercase().contains("over");
    match (nums.as_slice(), over) {
        ([lo, hi, ..], _) if lo <= hi => Some((*lo, Some(*hi))),
        ([v], true) => Some((v + 1, None)),
        ([v], false) => Some((*v, Some(*v))),
        _ => None,
    }
}

/// `5/9/2015` or `05/09/2015` -> `2015-05-09`, if it is a real date.
pub fn parse_date(s: &str) -> Option<String> {
    let mut it = s.trim().split('/');
    let (m, d, y) = (it.next()?, it.next()?, it.next()?);
    if it.next().is_some() || y.len() != 4 {
        return None;
    }
    let date = oe_core::Date::from_ymd(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?)?;
    Some(date.to_string())
}

/// `$100,000`, `$1,001 - $15,000`, `$333.37`, `Over $1,000,000`: not the
/// heading `$200?` of the capital gains column.
fn looks_like_amount(s: &str) -> bool {
    let t = s.trim();
    let body = t.trim_start_matches(|c: char| c.is_ascii_alphabetic() || c == '/' || c == ' ');
    body.starts_with('$')
        && !t.contains('?')
        && body
            .chars()
            .all(|c| c.is_ascii_digit() || " $,.-".contains(c))
        && body.chars().any(|c| c.is_ascii_digit())
}

/// Every `m/d/yyyy` in `s`, in order.
fn dates_in(s: &str) -> Vec<String> {
    s.split(|c: char| !(c.is_ascii_digit() || c == '/'))
        .filter_map(parse_date)
        .collect()
}

fn tx_type(s: &str) -> Option<&'static str> {
    let t = s.trim().to_ascii_uppercase();
    match t.as_str() {
        "P" | "PURCHASE" => Some("P"),
        "S" | "SALE" => Some("S"),
        "E" | "EXCHANGE" => Some("E"),
        _ if t.starts_with("S (PARTIAL") || t == "S(PARTIAL)" => Some("S (partial)"),
        _ => None,
    }
}

const OWNERS: [&str; 3] = ["SP", "JT", "DC"];

/// The ticker and type code an asset's name ends with:
/// `Intel Corporation - Common Stock (INTC) [OP]` -> (`INTC`, `OP`).
pub fn ticker_and_type(asset: &str) -> (Option<String>, Option<String>) {
    let kind = asset.rfind('[').and_then(|i| {
        let rest = &asset[i + 1..];
        let code = rest.split(']').next()?.trim();
        (code.len() == 2 && code.chars().all(|c| c.is_ascii_alphabetic()))
            .then(|| code.to_ascii_uppercase())
    });
    let before = match asset.rfind('[') {
        Some(i) => &asset[..i],
        None => asset,
    };
    let ticker = before.rfind('(').and_then(|i| {
        let inner = before[i + 1..].split(')').next()?.trim();
        let ok = !inner.is_empty()
            && inner.len() <= 8
            && inner
                .chars()
                .all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '-')
            && inner.chars().any(|c| c.is_ascii_alphabetic())
            && !inner.eq_ignore_ascii_case("partial");
        ok.then(|| inner.to_ascii_uppercase())
    });
    (ticker, kind)
}

/// Which field a labelled line fills: `FIlINg STaTuS: New`, `F S: New`,
/// `DESCRIPTIoN: ...`, `D: ...`, `CommENTS:`, `loCaTIoN:`, `S O:`.
fn label(text: &str) -> Option<(&'static str, String)> {
    let (head, value) = text.split_once(':')?;
    let letters: String = head
        .chars()
        .filter(|c| !matches!(c, ' ' | '|' | '.'))
        .collect::<String>()
        .to_ascii_uppercase();
    if letters.is_empty() || letters.len() > 16 || !letters.chars().all(|c| c.is_ascii_alphabetic())
    {
        return None;
    }
    let field = match letters.as_str() {
        "FS" | "FILINGSTATUS" => "filing_status",
        "D" | "DESCRIPTION" => "description",
        "C" | "COMMENTS" | "COMMENT" => "comments",
        "L" | "LOCATION" => "location",
        "SO" | "SUBHOLDINGOF" => "subholding_of",
        _ => return None,
    };
    Some((field, value.trim().to_string()))
}

fn joined(cells: &[&Cell]) -> String {
    cells
        .iter()
        .map(|c| c.text.as_str())
        .collect::<Vec<_>>()
        .join(" ")
}

/// Where a transaction's columns are, from the line it starts on.
struct Columns {
    asset_x: f64,
    type_x: f64,
    amount_x: Option<f64>,
}

/// A type written so close to the asset that both are one cell
/// (`Keurig Dr Pepper Inc. (KDP) [ST] S`): split it off, when the next cell
/// holds the dates.
fn split_glued_type(cells: &[Cell]) -> Vec<Cell> {
    let mut out: Vec<Cell> = Vec::with_capacity(cells.len() + 1);
    for (i, c) in cells.iter().enumerate() {
        let next_has_date = cells
            .get(i + 1)
            .is_some_and(|n| !dates_in(&n.text).is_empty());
        // Small-caps fonts may write the type in lower case.
        let upper = c.text.to_ascii_uppercase();
        let glued = ["S (PARTIAL)", "P", "S", "E"].iter().find(|t| {
            upper.len() > t.len() + 1
                && upper.ends_with(*t)
                && upper[..upper.len() - t.len()].ends_with(' ')
        });
        match glued {
            Some(t) if next_has_date && tx_type(&c.text).is_none() => {
                let head = c.text[..c.text.len() - t.len()].trim_end().to_string();
                let t = &c.text[c.text.len() - t.len()..];
                let w = (c.x_end - c.x) / c.text.len().max(1) as f64;
                let type_x = c.x_end - w * t.len() as f64;
                out.push(Cell {
                    x: c.x,
                    x_end: type_x,
                    size: c.size,
                    text: head,
                });
                out.push(Cell {
                    x: type_x,
                    x_end: c.x_end,
                    size: c.size,
                    text: t.to_string(),
                });
            }
            _ => out.push(c.clone()),
        }
    }
    out
}

fn start_of_transaction(line: &Line) -> Option<(PtrLine, Columns)> {
    let split = split_glued_type(&line.cells);
    let cells = &split;
    let ti = cells.iter().position(|c| tx_type(&c.text).is_some())?;
    let after: Vec<&Cell> = cells[ti + 1..].iter().collect();
    // The dates come right after the type: a labelled line such as
    // "S O: ... 06/23/2023" is not a transaction.
    let starts_with_date = |c: &&Cell| {
        c.text
            .split_whitespace()
            .next()
            .and_then(parse_date)
            .is_some()
    };
    if !after.first().is_some_and(starts_with_date) {
        return None;
    }
    let rest = joined(&after);
    let dates = dates_in(&rest);
    let mut start = 0;
    let mut owner = None;
    if let Some(first) = cells.first() {
        if ti > 0 && OWNERS.iter().any(|o| first.text.eq_ignore_ascii_case(o)) {
            owner = Some(first.text.to_ascii_uppercase());
            start = 1;
        }
    }
    let asset_cells: Vec<&Cell> = cells[start..ti].iter().collect();
    let asset_x = asset_cells.first().map_or(cells[ti].x, |c| c.x);
    // The amount: what follows the dates.
    let amount_cell = after
        .iter()
        .position(|c| c.text.contains('$') || c.text.to_ascii_lowercase().contains("over"));
    let amount = match amount_cell {
        Some(i) => {
            let first = &after[i].text;
            let from = first.find('$').map_or(0, |p| {
                // Keep a leading "Over" or "Spouse/DC Over".
                let lower = first.to_ascii_lowercase();
                lower.find("over").filter(|o| *o < p).map_or(p, |o| {
                    lower[..o]
                        .rfind(|c: char| c.is_ascii_digit())
                        .map_or(0, |d| d + 1)
                })
            });
            let mut a = first[from..].trim().to_string();
            for c in &after[i + 1..] {
                a.push(' ');
                a.push_str(&c.text);
            }
            a
        }
        None => String::new(),
    };
    let tx = PtrLine {
        owner,
        asset: joined(&asset_cells),
        tx_type: tx_type(&cells[ti].text).unwrap_or("?").to_string(),
        tx_date: dates.first().cloned(),
        notified_on: dates.get(1).cloned(),
        amount,
        ..PtrLine::default()
    };
    let cols = Columns {
        asset_x,
        type_x: cells[ti].x,
        amount_x: amount_cell.map(|i| after[i].x),
    };
    Some((tx, cols))
}

fn finish(mut tx: PtrLine, out: &mut PtrReport) {
    tx.asset = tx.asset.split_whitespace().collect::<Vec<_>>().join(" ");
    tx.amount = tx.amount.split_whitespace().collect::<Vec<_>>().join(" ");
    if let Some((lo, hi)) = parse_amount(&tx.amount) {
        tx.amount_min = Some(lo);
        tx.amount_max = hi;
    }
    let (ticker, kind) = ticker_and_type(&tx.asset);
    tx.ticker = ticker;
    tx.asset_type = kind;
    let mut missing = Vec::new();
    if tx.tx_date.is_none() {
        missing.push("date");
    }
    if tx.amount_min.is_none() {
        missing.push("amount");
    }
    if tx.asset.is_empty() {
        missing.push("asset");
    }
    if !missing.is_empty() {
        out.problems.push(format!(
            "{} {}: no {}",
            tx.tx_type,
            if tx.asset.is_empty() { "?" } else { &tx.asset },
            missing.join(", ")
        ));
    }
    out.lines.push(tx);
}

/// Reads the transactions from a report's pages of lines.
pub fn read_lines(pages: &[Vec<Line>]) -> PtrReport {
    let mut out = PtrReport::default();
    let mut current: Option<(PtrLine, Columns, Option<&'static str>)> = None;
    for line in pages.iter().flatten() {
        let text = line.text();
        let lower = text.to_ascii_lowercase();
        if lower.contains("owner") && lower.contains("asset") {
            out.has_table = true;
        }
        if let Some((tx, cols)) = start_of_transaction(line) {
            if let Some((prev, _, _)) = current.take() {
                finish(prev, &mut out);
            }
            out.has_table = true;
            current = Some((tx, cols, None));
            continue;
        }
        let Some((tx, cols, last_label)) = current.as_mut() else {
            continue;
        };
        let first = &line.cells[0];
        let at_asset = (first.x - cols.asset_x).abs() <= 6.0;
        // A labelled line under the asset.
        if at_asset {
            let all: Vec<&Cell> = line.cells.iter().collect();
            if let Some((field, value)) = label(&joined(&all)) {
                let slot = match field {
                    "filing_status" => &mut tx.filing_status,
                    "description" => &mut tx.description,
                    "comments" => &mut tx.comments,
                    "location" => &mut tx.location,
                    _ => &mut tx.subholding_of,
                };
                *slot = Some(value);
                *last_label = Some(field);
                continue;
            }
        }
        // More of the amount, under the amount column.
        let mut used = false;
        if let Some(ax) = cols.amount_x {
            // An amount cut by a page break ("$50,001 -" ... "$100,000") ends
            // wherever its second half is, right of the type column.
            let open = tx.amount.trim_end().ends_with('-');
            for c in &line.cells {
                let in_column = c.x >= ax - 15.0 || (open && c.x > cols.type_x);
                if in_column && looks_like_amount(&c.text) {
                    tx.amount.push(' ');
                    tx.amount.push_str(&c.text);
                    used = true;
                }
            }
        }
        if at_asset {
            let left: Vec<&Cell> = line
                .cells
                .iter()
                .filter(|c| {
                    c.x < cols.type_x - 3.0 && cols.amount_x.is_none_or(|ax| c.x < ax - 15.0)
                })
                .collect();
            match last_label {
                // A wrapped labelled line goes on with its label.
                Some(field) => {
                    let slot = match *field {
                        "filing_status" => &mut tx.filing_status,
                        "description" => &mut tx.description,
                        "comments" => &mut tx.comments,
                        "location" => &mut tx.location,
                        _ => &mut tx.subholding_of,
                    };
                    let s = slot.get_or_insert_with(String::new);
                    s.push(' ');
                    s.push_str(&joined(&line.cells.iter().collect::<Vec<_>>()));
                }
                None if !left.is_empty() => {
                    tx.asset.push(' ');
                    tx.asset.push_str(&joined(&left));
                }
                None => {}
            }
            used = true;
        }
        let _ = used;
    }
    if let Some((tx, _, _)) = current.take() {
        finish(tx, &mut out);
    }
    for tx in &mut out.lines {
        for f in [
            &mut tx.description,
            &mut tx.comments,
            &mut tx.location,
            &mut tx.subholding_of,
            &mut tx.filing_status,
        ] {
            if let Some(v) = f {
                *v = v.split_whitespace().collect::<Vec<_>>().join(" ");
                if v.is_empty() {
                    *f = None;
                }
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cell(x: f64, text: &str) -> Cell {
        Cell {
            x,
            x_end: x + text.len() as f64 * 5.0,
            size: 10.0,
            text: text.into(),
        }
    }
    fn line(y: f64, cells: &[(f64, &str)]) -> Line {
        Line {
            y,
            cells: cells.iter().map(|(x, t)| cell(*x, t)).collect(),
        }
    }

    #[test]
    fn amounts_dates_types_and_tickers() {
        assert_eq!(parse_amount("$1,001 - $15,000"), Some((1001, Some(15000))));
        assert_eq!(
            parse_amount("$1,000,001 - $5,000,000"),
            Some((1_000_001, Some(5_000_000)))
        );
        assert_eq!(parse_amount("Over $50,000,000"), Some((50_000_001, None)));
        assert_eq!(
            parse_amount("Spouse/DC Over $1,000,000"),
            Some((1_000_001, None))
        );
        assert_eq!(parse_amount("$15,000"), Some((15_000, Some(15_000))));
        assert_eq!(parse_amount("none"), None);
        assert_eq!(parse_date("5/9/2015").as_deref(), Some("2015-05-09"));
        assert_eq!(parse_date("02/30/2015"), None);
        assert_eq!(parse_date("??/??/????"), None);
        assert_eq!(tx_type("S (partial)"), Some("S (partial)"));
        assert_eq!(tx_type("X"), None);
        assert_eq!(
            ticker_and_type("Intel Corporation - Common Stock (INTC) [OP]"),
            (Some("INTC".into()), Some("OP".into()))
        );
        assert_eq!(
            ticker_and_type("apple Inc. (aaPl)"),
            (Some("AAPL".into()), None)
        );
        assert_eq!(
            ticker_and_type("Berkshire Hathaway Inc. (BRK.B) [ST]")
                .0
                .as_deref(),
            Some("BRK.B")
        );
        assert_eq!(
            ticker_and_type("US Treasury Bill [GS]"),
            (None, Some("GS".into()))
        );
    }

    /// The later layout (invented values): owner, asset, type, both dates in
    /// one cell, amount across two lines, short labels.
    #[test]
    fn reads_the_later_layout() {
        let page = vec![
            line(
                503.0,
                &[
                    (25.0, "ID"),
                    (65.0, "Owner Asset"),
                    (261.0, "Transaction Date"),
                    (381.0, "Notification Amount"),
                    (525.0, "Cap."),
                ],
            ),
            line(
                459.0,
                &[
                    (65.0, "SP"),
                    (104.0, "Example Widgets Corporation - Common Stock"),
                    (261.0, "P"),
                    (325.0, "05/29/2026 05/29/2026"),
                    (445.0, "$1,000,001 -"),
                ],
            ),
            line(448.5, &[(104.0, "(EXWG) [OP]"), (445.0, "$5,000,000")]),
            line(431.3, &[(104.0, "F"), (129.0, "S"), (158.0, ": New")]),
            line(
                418.5,
                &[
                    (104.0, "D"),
                    (
                        152.0,
                        ": Purchased 20 call options with a strike price of $5.",
                    ),
                ],
            ),
            line(
                392.3,
                &[
                    (104.0, "Example Bank Inc."),
                    (261.0, "S (partial)"),
                    (325.0, "06/01/2026 06/03/2026"),
                    (445.0, "$1,001 - $15,000"),
                ],
            ),
            line(364.5, &[(104.0, "F"), (129.0, "S"), (158.0, ": New")]),
            line(
                327.8,
                &[(
                    25.0,
                    "* For the complete list of asset type abbreviations, please visit ...",
                )],
            ),
            line(
                223.5,
                &[(37.0, "I CERTIFY that the statements I have made ...")],
            ),
        ];
        let r = read_lines(&[page]);
        assert!(r.has_table && r.problems.is_empty(), "{:?}", r.problems);
        assert_eq!(r.lines.len(), 2);
        let a = &r.lines[0];
        assert_eq!(a.owner.as_deref(), Some("SP"));
        assert_eq!(
            a.asset,
            "Example Widgets Corporation - Common Stock (EXWG) [OP]"
        );
        assert_eq!(
            (a.ticker.as_deref(), a.asset_type.as_deref()),
            (Some("EXWG"), Some("OP"))
        );
        assert_eq!(
            (
                a.tx_type.as_str(),
                a.tx_date.as_deref(),
                a.notified_on.as_deref()
            ),
            ("P", Some("2026-05-29"), Some("2026-05-29"))
        );
        assert_eq!(a.amount, "$1,000,001 - $5,000,000");
        assert_eq!(
            (a.amount_min, a.amount_max),
            (Some(1_000_001), Some(5_000_000))
        );
        assert_eq!(a.filing_status.as_deref(), Some("New"));
        assert_eq!(
            a.description.as_deref(),
            Some("Purchased 20 call options with a strike price of $5.")
        );
        let b = &r.lines[1];
        assert_eq!(
            (b.owner.as_deref(), b.tx_type.as_str(), b.amount_max),
            (None, "S (partial)", Some(15_000))
        );
        assert_eq!(b.ticker, None);
    }

    /// The earlier layout (invented values): long labels, wrapped comments,
    /// dates in separate cells, a second page with its heading again.
    #[test]
    fn reads_the_earlier_layout_across_pages() {
        let p1 = vec![
            line(
                489.0,
                &[
                    (26.0, "ID"),
                    (63.0, "Owner Asset"),
                    (269.0, "Transaction Date"),
                    (388.0, "Notification Amount"),
                ],
            ),
            line(
                456.0,
                &[
                    (63.0, "DC"),
                    (102.0, "example Corp. (eXmP)"),
                    (269.0, "P"),
                    (334.0, "10/29/2014 05/19/2015"),
                    (453.0, "$1,001 - $15,000"),
                ],
            ),
            line(438.8, &[(102.0, "FIlINg STaTuS: New")]),
            line(426.0, &[(102.0, "DESCRIPTIoN: Dependent child a")]),
            line(
                413.2,
                &[(
                    102.0,
                    "CommENTS: While preparing my report it was found that this was not",
                )],
            ),
            line(400.5, &[(102.0, "reported.")]),
            line(
                292.5,
                &[
                    (63.0, "JT"),
                    (102.0, "Example Savings Bank Common"),
                    (269.0, "E"),
                    (334.0, "02/9/2015 05/19/2015"),
                    (453.0, "$250,001 - $500,000"),
                ],
            ),
            line(282.0, &[(102.0, "Stock")]),
            line(264.8, &[(102.0, "loCaTIoN: uS")]),
        ];
        let p2 = vec![
            line(
                705.8,
                &[
                    (26.0, "ID"),
                    (63.0, "Owner Asset"),
                    (269.0, "Transaction Date"),
                    (388.0, "Notification Amount"),
                ],
            ),
            line(
                672.8,
                &[
                    (63.0, "DC"),
                    (102.0, "example Corp. (eXmP)"),
                    (269.0, "S"),
                    (334.0, "11/4/2014"),
                    (388.0, "05/19/2015"),
                    (453.0, "$1,001 - $15,000"),
                ],
            ),
        ];
        let r = read_lines(&[p1, p2]);
        assert_eq!(r.lines.len(), 3, "{:?}", r.lines);
        let a = &r.lines[0];
        assert_eq!(
            (a.owner.as_deref(), a.ticker.as_deref()),
            (Some("DC"), Some("EXMP"))
        );
        assert_eq!(
            a.comments.as_deref(),
            Some("While preparing my report it was found that this was not reported.")
        );
        assert_eq!(a.description.as_deref(), Some("Dependent child a"));
        let b = &r.lines[1];
        assert_eq!(b.asset, "Example Savings Bank Common Stock");
        assert_eq!(
            (
                b.tx_type.as_str(),
                b.tx_date.as_deref(),
                b.location.as_deref()
            ),
            ("E", Some("2015-02-09"), Some("uS"))
        );
        let c = &r.lines[2];
        assert_eq!(
            (
                c.tx_type.as_str(),
                c.tx_date.as_deref(),
                c.notified_on.as_deref()
            ),
            ("S", Some("2014-11-04"), Some("2015-05-19"))
        );
    }

    #[test]
    fn a_type_glued_to_the_asset_is_split_off() {
        let page = vec![
            line(
                456.0,
                &[
                    (102.0, "Example Water Dist 5% 9/1/25 [GS] S"),
                    (334.0, "01/2/2015 02/5/2015"),
                    (452.0, "$15,001 - $50,000"),
                ],
            ),
            line(438.8, &[(102.0, "FILINg STaTUS: New")]),
            line(
                191.2,
                &[
                    (62.0, "SP"),
                    (101.0, "Example Pepper Inc. (EXP) [ST] S"),
                    (311.0, "07/11/2018 07/17/2018"),
                    (430.0, "$240.15"),
                ],
            ),
        ];
        let r = read_lines(&[page]);
        assert_eq!(r.lines.len(), 2, "{:?}", r);
        assert_eq!(
            (r.lines[0].tx_type.as_str(), r.lines[0].asset.as_str()),
            ("S", "Example Water Dist 5% 9/1/25 [GS]")
        );
        assert_eq!(r.lines[0].filing_status.as_deref(), Some("New"));
        assert_eq!(
            (
                r.lines[1].ticker.as_deref(),
                r.lines[1].amount_min,
                r.lines[1].amount_max
            ),
            (Some("EXP"), Some(240), Some(240))
        );
    }

    #[test]
    fn lower_case_types_and_amounts_cut_by_a_page_break() {
        let p1 = vec![
            line(
                456.0,
                &[
                    (63.0, "JT"),
                    (102.0, "Example Corporate Bond ETF s"),
                    (335.0, "03/1/2017 03/1/2017"),
                    (452.0, "$1,001 - $15,000"),
                ],
            ),
            line(
                75.0,
                &[
                    (62.0, "JT"),
                    (100.0, "Example Technologies, Inc. (EXT) [ST] P"),
                    (312.0, "09/25/2018 10/01/2018"),
                    (430.0, "$50,001 -"),
                ],
            ),
        ];
        let p2 = vec![
            line(
                705.8,
                &[
                    (26.0, "ID"),
                    (62.0, "Owner Asset"),
                    (247.0, "Transaction Date"),
                    (365.0, "Notification Amount"),
                    (506.0, "Cap."),
                ],
            ),
            line(690.0, &[(432.0, "$100,000")]),
        ];
        let r = read_lines(&[p1, p2]);
        assert_eq!(r.lines.len(), 2, "{:?}", r);
        assert_eq!(
            (r.lines[0].tx_type.as_str(), r.lines[0].asset.as_str()),
            ("S", "Example Corporate Bond ETF")
        );
        assert_eq!(
            (r.lines[1].amount_min, r.lines[1].amount_max),
            (Some(50_001), Some(100_000))
        );
        assert!(r.problems.is_empty());
    }

    #[test]
    fn the_gains_heading_is_not_an_amount() {
        assert!(
            looks_like_amount("$100,000")
                && looks_like_amount("$1,001 - $15,000")
                && looks_like_amount("$333.37")
        );
        assert!(
            looks_like_amount("Over $1,000,000") && looks_like_amount("Spouse/DC Over $1,000,000")
        );
        assert!(
            !looks_like_amount("$200?") && !looks_like_amount("Gains >") && !looks_like_amount("$")
        );
        let p1 = vec![line(
            75.0,
            &[
                (62.0, "JT"),
                (100.0, "Example Tech (EXT) [ST] P"),
                (312.0, "09/25/2018 10/01/2018"),
                (430.0, "$50,001 -"),
            ],
        )];
        let p2 = vec![
            line(
                705.8,
                &[
                    (26.0, "ID"),
                    (62.0, "Owner Asset"),
                    (247.0, "Transaction Date"),
                    (365.0, "Notification Amount"),
                    (506.0, "Cap."),
                ],
            ),
            line(
                694.5,
                &[(247.0, "Type"), (365.0, "Date"), (506.0, "Gains >")],
            ),
            line(683.2, &[(506.0, "$200?")]),
            line(668.2, &[(430.0, "$100,000")]),
        ];
        let r = read_lines(&[p1, p2]);
        assert_eq!(
            (r.lines[0].amount_min, r.lines[0].amount_max),
            (Some(50_001), Some(100_000)),
            "{:?}",
            r.lines[0]
        );
    }

    #[test]
    fn a_labelled_line_with_a_date_is_not_a_transaction() {
        let page = vec![
            line(
                459.0,
                &[
                    (104.0, "Example St 5.000% Due 12-01-30"),
                    (262.0, "S"),
                    (327.0, "06/23/2023 07/07/2023"),
                    (447.0, "$1,001 - $15,000"),
                ],
            ),
            line(
                418.5,
                &[
                    (104.0, "S"),
                    (152.0, "O : Example Trust, sold 06/23/2023 by the adviser"),
                ],
            ),
        ];
        let r = read_lines(&[page]);
        assert_eq!(r.lines.len(), 1, "{:?}", r);
        assert_eq!(
            r.lines[0].subholding_of.as_deref(),
            Some("Example Trust, sold 06/23/2023 by the adviser")
        );
    }
}
