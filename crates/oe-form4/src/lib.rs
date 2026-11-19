//! Parser for SEC ownership documents (Forms 3, 4, 5 and their amendments).
//!
//! ```
//! let xml = std::fs::read_to_string(concat!(
//!     env!("CARGO_MANIFEST_DIR"),
//!     "/../../fixtures/form4/synthetic/01_director_a_buy.xml"
//! )).unwrap();
//! let doc = oe_form4::parse(&xml).unwrap();
//! assert_eq!(doc.issuer.ticker.as_deref(), Some("SYNW"));
//! ```

mod parse;
pub mod xml;

pub use parse::{parse, ParseError};
