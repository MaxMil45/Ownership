//! Decides whether an amendment's rows may replace the rows it amends.
//! Pure logic, tested offline; `db` supplies the rows (ADR 0005).
//!
//! A 4/A or 5/A names the *date* of the report it amends, not its accession
//! number. Real amendments (36 checked on 2026-11-16..19) come in two kinds:
//!
//! * **Restatements**: the whole report again, with a date, a share count, a
//!   transaction code or a holding corrected, sometimes with an omitted row
//!   added at the end.
//! * **Supplements**: only the corrected or the omitted rows. Replacing the
//!   report with one of these would hide real trades from the ledger.
//!
//! The rows may replace the current version only when the amendment reads as a
//! restatement: it has at least as many rows, and each existing row is matched
//! by the amendment row in the same position, in the same table (derivative or
//! not), with **at least two of** its code, date and share count unchanged.
//! Anything else is held back. Missing data is better than wrong data.
//!
//! The rule first required the same code. On 2026 Q3 (727 amendments, bulk
//! data) that held back 32 amendments whose remarks say they correct a code,
//! A to P and D to S among them, in all 8 read; holding those back hid real
//! purchases and sales, so a corrected code alone is now a restatement.

/// The fields of one transaction row that the comparison uses.
#[derive(Debug, Clone, PartialEq)]
pub struct RowShape {
    pub is_derivative: bool,
    pub code: String,
    /// `YYYY-MM-DD`.
    pub date: String,
    /// Share count as stored, as text (PostgreSQL `numeric::text`), so that
    /// `100` and `100.0` from different filings compare by value, see
    /// [`same_number`].
    pub shares: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Row `i` of the amendment replaces row `i` of the current version;
    /// rows past the end are new facts.
    Restates { replaced: usize, added: usize },
    /// Held back; the reason is stored in `amendment_link.reason`.
    HeldBack(String),
}

/// Compares the amendment's rows with the current version of the report.
pub fn compare(current: &[RowShape], amendment: &[RowShape]) -> Verdict {
    if amendment.len() < current.len() {
        return Verdict::HeldBack(format!(
            "amendment has {} row(s), the report it amends has {}: it does not restate the report",
            amendment.len(),
            current.len()
        ));
    }
    for (i, (old, new)) in current.iter().zip(amendment).enumerate() {
        let table = |d: bool| if d { "derivative" } else { "non-derivative" };
        if old.is_derivative != new.is_derivative {
            return Verdict::HeldBack(format!(
                "row {i}: a {} row became a {} row",
                table(old.is_derivative),
                table(new.is_derivative)
            ));
        }
        let same_code = old.code == new.code;
        let same_date = old.date == new.date;
        let same_shares = same_number(old.shares.as_deref(), new.shares.as_deref());
        if u8::from(same_code) + u8::from(same_date) + u8::from(same_shares) < 2 {
            let mut changed = Vec::new();
            if !same_code {
                changed.push(format!("code {} to {}", old.code, new.code));
            }
            if !same_date {
                changed.push(format!("date {} to {}", old.date, new.date));
            }
            if !same_shares {
                changed.push("share count".to_string());
            }
            return Verdict::HeldBack(format!(
                "row {i}: {} changed, more than one of code, date and share count",
                changed.join(", ")
            ));
        }
    }
    Verdict::Restates {
        replaced: current.len(),
        added: amendment.len() - current.len(),
    }
}

/// Equal decimal numbers written differently (`100`, `100.0`, `100.00`).
/// Two blanks are equal; a blank and a number are not.
pub fn same_number(a: Option<&str>, b: Option<&str>) -> bool {
    fn canon(s: &str) -> &str {
        let s = s.trim();
        if s.contains('.') {
            s.trim_end_matches('0').trim_end_matches('.')
        } else {
            s
        }
    }
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => canon(a) == canon(b),
        _ => false,
    }
}

/// The form an amendment amends: `4/A` amends a `4`.
pub fn base_form(form: &str) -> Option<&str> {
    form.strip_suffix("/A")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(d: bool, code: &str, date: &str, shares: &str) -> RowShape {
        RowShape {
            is_derivative: d,
            code: code.into(),
            date: date.into(),
            shares: (!shares.is_empty()).then(|| shares.into()),
        }
    }

    // Each case below is a real amendment and its original, found on
    // 2026-11-16..19 (accession numbers in the comments).

    #[test]
    fn corrected_dates_with_the_same_shares_restate() {
        // 0001213900-26-121280 corrects 0001213900-26-108158: dates moved
        // from 2026-10-06 to 2026-09-29, shares unchanged.
        let old = [
            r(false, "A", "2026-10-06", "20000"),
            r(true, "A", "2026-10-06", "32110"),
        ];
        let new = [
            r(false, "A", "2026-09-29", "20000"),
            r(true, "A", "2026-09-29", "32110"),
        ];
        assert_eq!(
            compare(&old, &new),
            Verdict::Restates {
                replaced: 2,
                added: 0
            }
        );
    }

    #[test]
    fn a_corrected_share_count_on_the_same_date_restates() {
        // NVIDIA 4/A 0002152188-26-000007: "correct ... the number of acquired shares".
        let old = [r(false, "A", "2026-09-09", "1000")];
        let new = [r(false, "A", "2026-09-09", "1100")];
        assert!(matches!(compare(&old, &new), Verdict::Restates { .. }));
    }

    #[test]
    fn an_omitted_row_added_at_the_end_restates() {
        // 0000950142-26-003050 repeats the purchase of 0000950142-26-003013
        // and adds the restricted stock units it had left out.
        let old = [r(false, "P", "2026-11-11", "10000")];
        let new = [
            r(false, "P", "2026-11-11", "10000"),
            r(true, "A", "2026-10-29", "6439"),
        ];
        assert_eq!(
            compare(&old, &new),
            Verdict::Restates {
                replaced: 1,
                added: 1
            }
        );
    }

    #[test]
    fn a_supplement_that_reports_only_the_omitted_row_is_held_back() {
        // 0001493152-26-052649 reports only RSUs "not reported on the original
        // Form 4"; the original's J row must stay visible.
        let old = [r(false, "J", "2025-12-05", "10562500")];
        let new = [r(true, "A", "2025-12-05", "8000000")];
        assert!(matches!(compare(&old, &new), Verdict::HeldBack(_)));
    }

    #[test]
    fn a_shorter_amendment_is_held_back() {
        // 0002126944-26-000009 (8 rows) adds holdings omitted from the 13-row
        // 0002126944-26-000004.
        let old: Vec<RowShape> = (0..13).map(|_| r(false, "C", "2026-04-20", "1")).collect();
        let new: Vec<RowShape> = (0..8).map(|_| r(false, "C", "2026-04-20", "1")).collect();
        assert!(matches!(compare(&old, &new), Verdict::HeldBack(_)));
    }

    #[test]
    fn a_different_trade_in_the_same_position_is_held_back() {
        // 0001214659-26-014514: a purchase of 236,116 shares on 2026-11-09
        // became 2,825 shares on 2026-11-10. Not the same row corrected.
        let old = [r(false, "P", "2026-11-09", "236116")];
        let new = [r(false, "P", "2026-11-10", "2825")];
        assert!(matches!(compare(&old, &new), Verdict::HeldBack(_)));
    }

    #[test]
    fn a_reordered_or_longer_report_is_held_back() {
        // 0001193125-26-468244 (22 rows) amends the single derivative row of
        // 0001193125-26-461072 and starts with non-derivative rows.
        let old = [r(true, "M", "2026-11-11", "360")];
        let new = [
            r(false, "M", "2026-11-10", "374"),
            r(true, "M", "2026-11-11", "360"),
        ];
        assert!(matches!(compare(&old, &new), Verdict::HeldBack(_)));
    }

    #[test]
    fn a_corrected_code_on_the_same_row_restates() {
        // 0001193125-26-354277 amends 0001193125-26-350734: "to correct the
        // transaction code ... from 'A' to 'P' ... an open market purchase".
        let old = [r(false, "A", "2026-08-12", "5000")];
        let new = [r(false, "P", "2026-08-12", "5000")];
        assert!(matches!(compare(&old, &new), Verdict::Restates { .. }));
        // A code and a date changed together: not the same row corrected.
        let moved = [r(false, "P", "2026-08-13", "5000")];
        assert!(matches!(compare(&old, &moved), Verdict::HeldBack(_)));
    }

    #[test]
    fn held_back_reasons_say_what_changed() {
        let old = [r(false, "P", "2026-11-09", "236116")];
        let new = [r(false, "P", "2026-11-10", "2825")];
        let Verdict::HeldBack(why) = compare(&old, &new) else {
            panic!("held back expected")
        };
        assert_eq!(
            why,
            "row 0: date 2026-11-09 to 2026-11-10, share count changed, more than one of code, date and share count"
        );
    }

    #[test]
    fn empty_reports_restate_trivially() {
        // A Form 3/A of a Form 3: no transaction rows on either side.
        assert_eq!(
            compare(&[], &[]),
            Verdict::Restates {
                replaced: 0,
                added: 0
            }
        );
    }

    #[test]
    fn numbers_compare_by_value() {
        assert!(same_number(Some("100"), Some("100.00")));
        assert!(same_number(Some("12.50"), Some("12.5")));
        assert!(same_number(None, None));
        assert!(!same_number(Some("100"), None));
        assert!(!same_number(Some("100"), Some("1000")));
        assert!(!same_number(Some("10"), Some("1.0")));
    }

    #[test]
    fn base_forms() {
        assert_eq!(base_form("4/A"), Some("4"));
        assert_eq!(base_form("5/A"), Some("5"));
        assert_eq!(base_form("4"), None);
    }
}
