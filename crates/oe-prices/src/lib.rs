//! Daily stock prices from a file the user downloaded themselves: Stooq's
//! ASCII database (one text file per symbol, in a zip or the folder it
//! unzips to), or any CSV with a date and a close column. Nothing here
//! fetches prices, and no price data ships with the project (ADR 0003).
//!
//! Stooq's files look like
//!
//! ```text
//! <TICKER>,<PER>,<DATE>,<TIME>,<OPEN>,<HIGH>,<LOW>,<CLOSE>,<VOL>,<OPENINT>
//! AAPL.US,D,20240102,000000,187.15,188.44,183.89,185.64,82488700,0
//! ```
//!
//! and live under paths such as `data/daily/us/nasdaq stocks/1/aapl.us.txt`.
//! Their prices are adjusted for splits (and dividends), so a split does not
//! look like a crash.

use oe_core::Date;
use std::collections::HashSet;
use std::path::Path;

/// The Stooq name of a US ticker as Forms 3/4/5 write it: lower case,
/// `.` and `/` as `-`, then `.us`. `BRK.B` -> `brk-b.us`.
pub fn stooq_symbol(ticker: &str) -> Option<String> {
    let t = ticker.trim();
    if t.is_empty()
        || t.len() > 12
        || !t
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b".-/".contains(&b))
    {
        return None;
    }
    Some(format!(
        "{}.us",
        t.to_ascii_lowercase().replace(['.', '/'], "-")
    ))
}

/// The symbol a file holds, from its name: `.../aapl.us.txt` -> `aapl.us`.
pub fn symbol_from_path(name: &str) -> Option<String> {
    let file = name.rsplit(['/', '\\']).next()?;
    let stem = file
        .strip_suffix(".txt")
        .or_else(|| file.strip_suffix(".csv"))?;
    (!stem.is_empty()).then(|| stem.to_ascii_lowercase())
}

fn parse_day(s: &str) -> Option<Date> {
    let s = s.trim();
    if s.len() == 8 && s.bytes().all(|b| b.is_ascii_digit()) {
        let (y, m, d) = (&s[..4], &s[4..6], &s[6..]);
        return Date::from_ymd(y.parse().ok()?, m.parse().ok()?, d.parse().ok()?);
    }
    s.get(..10)?.parse().ok()
}

/// A series' days and closes, oldest first, one per day. Rows that are not
/// a date and a positive close are skipped (and counted).
#[derive(Debug, Clone, PartialEq, Default)]
pub struct Series {
    pub days: Vec<(Date, f64)>,
    pub skipped: usize,
}

/// Reads Stooq's format, Stooq's CSV download (`Date,Open,High,Low,Close,Volume`)
/// or any CSV whose header names a date and a close column.
pub fn parse_series(text: &str) -> Result<Series, String> {
    let mut lines = text.lines().filter(|l| !l.trim().is_empty());
    let header = lines.next().ok_or("empty file")?;
    let cols: Vec<String> = header
        .split(',')
        .map(|c| {
            c.trim()
                .trim_matches(['<', '>', '"', '\u{feff}'])
                .to_ascii_lowercase()
        })
        .collect();
    let find = |names: &[&str]| cols.iter().position(|c| names.contains(&c.as_str()));
    let date = find(&["date", "day"]).ok_or_else(|| format!("no date column in {header:?}"))?;
    let close = find(&["close", "adj close", "adj_close", "adjclose"])
        .ok_or_else(|| format!("no close column in {header:?}"))?;
    let mut s = Series::default();
    for line in lines {
        let f: Vec<&str> = line.split(',').collect();
        let parsed = (|| {
            let d = parse_day(f.get(date)?)?;
            let c: f64 = f.get(close)?.trim().parse().ok()?;
            (c.is_finite() && c > 0.0).then_some((d, c))
        })();
        match parsed {
            Some(p) => s.days.push(p),
            None => s.skipped += 1,
        }
    }
    s.days.sort_by_key(|a| a.0);
    let before = s.days.len();
    s.days.dedup_by(|a, b| a.0 == b.0);
    s.skipped += before - s.days.len();
    Ok(s)
}

/// What [`read_wanted`] found.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ReadReport {
    /// Price files in the source.
    pub files: usize,
    /// Of those, symbols that were wanted and read.
    pub read: usize,
    /// Wanted files that would not parse, with the reason.
    pub failed: Vec<(String, String)>,
}

/// Calls `each(symbol, series)` for every file in `path` (a `.zip` or a
/// folder, searched through) whose symbol is in `wanted`.
pub fn read_wanted(
    path: &Path,
    wanted: &HashSet<String>,
    mut each: impl FnMut(&str, Series),
) -> Result<ReadReport, String> {
    let mut report = ReadReport::default();
    let mut take = |name: &str, text: &str, report: &mut ReadReport| {
        if let Some(sym) = symbol_from_path(name) {
            report.files += 1;
            if wanted.contains(&sym) {
                match parse_series(text) {
                    Ok(s) => {
                        report.read += 1;
                        each(&sym, s);
                    }
                    Err(e) => report.failed.push((sym, e)),
                }
            }
        }
    };
    if path.is_dir() {
        let mut stack = vec![path.to_path_buf()];
        while let Some(dir) = stack.pop() {
            let entries = std::fs::read_dir(&dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            for e in entries.flatten() {
                let p = e.path();
                if p.is_dir() {
                    stack.push(p);
                } else if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
                    let wanted_here = symbol_from_path(name).is_some_and(|s| wanted.contains(&s));
                    if wanted_here {
                        let text = std::fs::read_to_string(&p)
                            .map_err(|e| format!("{}: {e}", p.display()))?;
                        take(name, &text, &mut report);
                    } else if symbol_from_path(name).is_some() {
                        report.files += 1;
                    }
                }
            }
        }
        return Ok(report);
    }
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let zip = oe_bulk::zip::Archive::open(&bytes).map_err(|e| {
        format!(
            "{}: {e} (if the zip is very large, unzip it and give the folder instead)",
            path.display()
        )
    })?;
    for entry in &zip.entries {
        if entry.name.ends_with('/') {
            continue;
        }
        let wanted_here = symbol_from_path(&entry.name).is_some_and(|s| wanted.contains(&s));
        if !wanted_here {
            if symbol_from_path(&entry.name).is_some() {
                report.files += 1;
            }
            continue;
        }
        let data = zip
            .read(entry)
            .map_err(|e| format!("{}: {e}", entry.name))?;
        let text = String::from_utf8_lossy(&data);
        take(&entry.name, &text, &mut report);
    }
    Ok(report)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn d(s: &str) -> Date {
        s.parse().unwrap()
    }

    #[test]
    fn tickers_become_stooq_symbols() {
        assert_eq!(stooq_symbol("AAPL").as_deref(), Some("aapl.us"));
        assert_eq!(stooq_symbol(" BRK.B ").as_deref(), Some("brk-b.us"));
        assert_eq!(stooq_symbol("BF/B").as_deref(), Some("bf-b.us"));
        assert_eq!(stooq_symbol(""), None);
        assert_eq!(stooq_symbol("NONE OF IT"), None);
        assert_eq!(
            symbol_from_path("data/daily/us/nasdaq stocks/1/aapl.us.txt").as_deref(),
            Some("aapl.us")
        );
        assert_eq!(
            symbol_from_path(r"C:\x\nyse stocks\2\BRK-B.US.txt").as_deref(),
            Some("brk-b.us")
        );
        assert_eq!(symbol_from_path("readme.md"), None);
    }

    #[test]
    fn reads_stooq_text_files() {
        // Invented numbers in Stooq's layout.
        let text = "<TICKER>,<PER>,<DATE>,<TIME>,<OPEN>,<HIGH>,<LOW>,<CLOSE>,<VOL>,<OPENINT>\n\
                    EXM.US,D,20240103,000000,10,11,9,10.5,1000,0\n\
                    EXM.US,D,20240102,000000,9,10,8,9.5,1000,0\n\
                    EXM.US,D,20240104,000000,10,11,9,0,1000,0\n\
                    EXM.US,D,2024x105,000000,10,11,9,11,1000,0\n\
                    EXM.US,D,20240103,000000,10,11,9,10.5,1000,0\n";
        let s = parse_series(text).unwrap();
        assert_eq!(
            s.days,
            vec![(d("2024-01-02"), 9.5), (d("2024-01-03"), 10.5)]
        );
        assert_eq!(s.skipped, 3, "a zero close, a bad date, a repeated day");
    }

    #[test]
    fn reads_plain_csv_with_a_date_and_a_close() {
        let s =
            parse_series("Date,Open,High,Low,Close,Volume\n2024-01-02,1,2,0.5,1.5,10\n").unwrap();
        assert_eq!(s.days, vec![(d("2024-01-02"), 1.5)]);
        let s = parse_series("\u{feff}day,adj close\n2024-01-05,2\n").unwrap();
        assert_eq!(s.days, vec![(d("2024-01-05"), 2.0)]);
        assert!(parse_series("a,b\n1,2\n").is_err());
        assert!(parse_series("").is_err());
    }

    #[test]
    fn reads_only_the_wanted_symbols_from_a_folder_or_a_zip() {
        let root = std::env::temp_dir().join(format!("oe-prices-{}", std::process::id()));
        let dir = root.join("data/daily/us/nyse stocks/1");
        std::fs::create_dir_all(&dir).unwrap();
        let a = "<TICKER>,<PER>,<DATE>,<TIME>,<OPEN>,<HIGH>,<LOW>,<CLOSE>,<VOL>,<OPENINT>\nA.US,D,20240102,000000,1,1,1,1.25,1,0\n";
        std::fs::write(dir.join("exa.us.txt"), a).unwrap();
        std::fs::write(dir.join("exb.us.txt"), a).unwrap();
        let wanted: HashSet<String> = ["exa.us".to_string()].into();
        let mut got = Vec::new();
        let r = read_wanted(&root, &wanted, |sym, s| {
            got.push((sym.to_string(), s.days.len()))
        })
        .unwrap();
        assert_eq!((r.files, r.read), (2, 1));
        assert_eq!(got, vec![("exa.us".to_string(), 1)]);

        let zip = oe_bulk::zip::tests_support::write(&[
            ("data/daily/us/nyse stocks/1/exa.us.txt", a.as_bytes(), true),
            (
                "data/daily/us/nyse stocks/1/exb.us.txt",
                a.as_bytes(),
                false,
            ),
            ("data/daily/us/nyse stocks/1/", b"", false),
        ]);
        let zpath = root.join("prices.zip");
        std::fs::write(&zpath, zip).unwrap();
        let mut got = Vec::new();
        let r = read_wanted(&zpath, &wanted, |sym, s| {
            got.push((sym.to_string(), s.days[0].1))
        })
        .unwrap();
        assert_eq!((r.files, r.read), (2, 1));
        assert_eq!(got, vec![("exa.us".to_string(), 1.25)]);
        let _ = std::fs::remove_dir_all(root);
    }
}
