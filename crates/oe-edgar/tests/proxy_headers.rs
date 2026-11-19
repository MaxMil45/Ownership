//! Real contested-solicitation headers in `fixtures/proxy/real/` must read the
//! way `MANIFEST.tsv` says (form, subject company, filers). The manifest comes
//! from regexes over the raw headers, not from this crate.

use oe_edgar::parse_header_sgml;
use std::path::PathBuf;

#[test]
fn real_contest_headers_match_the_manifest() {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../fixtures/proxy/real");
    let manifest = std::fs::read_to_string(dir.join("MANIFEST.tsv")).unwrap();
    let mut rows = 0;
    for line in manifest.lines().skip(1).filter(|l| !l.trim().is_empty()) {
        let c: Vec<&str> = line.split('\t').collect();
        let h = parse_header_sgml(&std::fs::read_to_string(dir.join(c[0])).unwrap())
            .unwrap_or_else(|e| panic!("{}: {e}", c[0]));
        let subject = h
            .subject
            .as_ref()
            .unwrap_or_else(|| panic!("{}: no subject", c[0]));
        assert_eq!(h.form, c[1], "{}: form", c[0]);
        assert_eq!(subject.cik.0.to_string(), c[2], "{}: subject", c[0]);
        let filers: Vec<String> = h.filed_by.iter().map(|p| p.cik.0.to_string()).collect();
        assert_eq!(filers.join("+"), c[3], "{}: filed by", c[0]);
        assert_eq!(
            h.filed_by.iter().any(|p| p.cik != subject.cik),
            c[4] == "yes",
            "{}: challenger",
            c[0]
        );
        assert!(
            h.known_on().is_some() && !subject.name.is_empty(),
            "{}",
            c[0]
        );
        rows += 1;
    }
    assert!(rows >= 10);
}
