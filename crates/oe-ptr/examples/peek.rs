//! Prints a report's lines and cells, for writing and checking the reader.
fn main() {
    let path = std::env::args().nth(1).expect("a PDF path");
    let bytes = std::fs::read(&path).expect("read");
    for (n, page) in oe_ptr::report_lines(&bytes)
        .expect("lines")
        .iter()
        .enumerate()
    {
        println!("=== page {}", n + 1);
        for l in page {
            let xs: Vec<String> = l.cells.iter().map(|c| format!("{:.0}", c.x)).collect();
            println!("{:6.1} [{}] {}", l.y, xs.join(","), l.text());
        }
    }
}
