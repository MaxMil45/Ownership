//! Reads every typed report under a folder and prints how well it went:
//! `cargo run --release -p oe-ptr --example survey -- data/officials/house`.
use std::path::{Path, PathBuf};

fn pdfs(dir: &Path, out: &mut Vec<PathBuf>) {
    if let Ok(rd) = std::fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                pdfs(&p, out);
            } else if p.extension().is_some_and(|x| x == "pdf")
                && p.file_stem().is_some_and(|s| s.len() == 8)
            // 8-digit ids: typed reports
            {
                out.push(p);
            }
        }
    }
}

fn main() {
    let root = std::env::args().nth(1).expect("folder");
    let mut files = Vec::new();
    pdfs(Path::new(&root), &mut files);
    files.sort();
    let started = std::time::Instant::now();
    let (mut ok, mut failed, mut empty, mut lines, mut problems, mut no_ticker) =
        (0, 0, 0, 0, 0, 0);
    let mut examples_empty = Vec::new();
    let mut examples_problem = Vec::new();
    let mut examples_failed = Vec::new();
    let mut types = std::collections::BTreeMap::new();
    for f in &files {
        let bytes = std::fs::read(f).unwrap();
        match oe_ptr::read_report(&bytes) {
            Ok(r) => {
                ok += 1;
                if r.lines.is_empty() {
                    empty += 1;
                    if examples_empty.len() < 8 {
                        examples_empty.push(f.display().to_string());
                    }
                }
                lines += r.lines.len();
                problems += r.problems.len();
                for l in &r.lines {
                    *types.entry(l.tx_type.clone()).or_insert(0) += 1;
                    if l.ticker.is_none() {
                        no_ticker += 1;
                    }
                }
                if !r.problems.is_empty() && examples_problem.len() < 8 {
                    examples_problem.push(format!("{}: {}", f.display(), r.problems[0]));
                }
            }
            Err(e) => {
                failed += 1;
                if examples_failed.len() < 5 {
                    examples_failed.push(format!("{}: {e}", f.display()));
                }
            }
        }
    }
    println!(
        "{} reports in {:.1} s: {ok} read, {failed} failed, {empty} with no transaction found",
        files.len(),
        started.elapsed().as_secs_f64()
    );
    println!("{lines} transactions ({no_ticker} without a ticker), {problems} with a missing field; types {types:?}");
    println!("no transaction found, e.g.:");
    for e in &examples_empty {
        println!("  {e}");
    }
    println!("missing fields, e.g.:");
    for e in &examples_problem {
        println!("  {e}");
    }
    println!("failed, e.g.:");
    for e in &examples_failed {
        println!("  {e}");
    }
}
