//! House periodic transaction reports (PTRs): the transactions a member of
//! Congress reports, read from the PDFs the House Clerk publishes (ADR 0011).
//!
//! * [`text`]: text with positions from the PDF (lopdf opens and decrypts it;
//!   the reports use RC4 with an empty password and forbid copying, which
//!   the user chose to read past on 2026-11-25; the reports are public
//!   records, their use is limited by 5 U.S.C. 13107).
//! * [`lines`]: positioned characters grouped into lines and cells.
//! * [`table`]: the transaction table rebuilt from the lines.

pub mod table;
pub mod text;

pub use table::{parse_amount, parse_date, read_lines, ticker_and_type, PtrLine, PtrReport};
pub use text::Item;

/// A cell: characters close together on one line.
#[derive(Debug, Clone, PartialEq)]
pub struct Cell {
    pub x: f64,
    pub x_end: f64,
    pub size: f64,
    pub text: String,
}

/// One line of text across the page, cells left to right.
#[derive(Debug, Clone, PartialEq)]
pub struct Line {
    pub y: f64,
    pub cells: Vec<Cell>,
}

impl Line {
    pub fn text(&self) -> String {
        self.cells
            .iter()
            .map(|c| c.text.as_str())
            .collect::<Vec<_>>()
            .join(" | ")
    }
}

/// Groups a page's characters into lines (top first) and cells. Characters
/// on the same baseline (within a third of their size) form a line; within
/// it, a gap wider than about half a character starts a new word, and a gap
/// wider than [`CELL_GAP`] characters a new cell.
pub const CELL_GAP: f64 = 1.2;

pub fn lines(items: &[Item]) -> Vec<Line> {
    let mut sorted: Vec<&Item> = items.iter().filter(|i| i.size > 0.0).collect();
    sorted.sort_by(|a, b| b.y.total_cmp(&a.y).then(a.x.total_cmp(&b.x)));
    let mut rows: Vec<Vec<&Item>> = Vec::new();
    for it in sorted {
        match rows.last_mut() {
            Some(row) if (row[0].y - it.y).abs() <= row[0].size.max(it.size) * 0.35 => row.push(it),
            _ => rows.push(vec![it]),
        }
    }
    rows.into_iter()
        .map(|mut row| {
            row.sort_by(|a, b| a.x.total_cmp(&b.x));
            let y = row.iter().map(|i| i.y).sum::<f64>() / row.len() as f64;
            let mut cells: Vec<Cell> = Vec::new();
            for it in row {
                let size = it.size.max(1.0);
                match cells.last_mut() {
                    Some(c) if it.x - c.x_end < size * CELL_GAP => {
                        let gap = it.x - c.x_end;
                        if gap > size * 0.18 && !c.text.ends_with(' ') && !it.text.starts_with(' ')
                        {
                            c.text.push(' ');
                        }
                        c.text.push_str(&it.text);
                        c.x_end = c.x_end.max(it.x_end);
                    }
                    _ => cells.push(Cell {
                        x: it.x,
                        x_end: it.x_end,
                        size: it.size,
                        text: it.text.clone(),
                    }),
                }
            }
            for c in &mut cells {
                let t = c.text.split_whitespace().collect::<Vec<_>>().join(" ");
                c.text = t;
            }
            cells.retain(|c| !c.text.is_empty());
            Line { y, cells }
        })
        .filter(|l| !l.cells.is_empty())
        .collect()
}

/// Opens a report (decrypting it when needed) and returns its pages' lines.
pub fn report_lines(bytes: &[u8]) -> Result<Vec<Vec<Line>>, String> {
    let doc = lopdf::Document::load_mem(bytes).map_err(|e| format!("not a readable PDF: {e}"))?;
    Ok(text::page_items(&doc)
        .iter()
        .map(|items| lines(items))
        .collect())
}

/// Reads a report's transactions from the PDF's bytes.
pub fn read_report(bytes: &[u8]) -> Result<PtrReport, String> {
    Ok(read_lines(&report_lines(bytes)?))
}
