//! Argument helpers shared by the `oe` commands, so every command reads
//! counts, folders, numbers and dates the same way and words its errors the
//! same way.

use oe_core::Date;
use std::path::PathBuf;
use std::str::FromStr;

pub type Args<'a> = std::slice::Iter<'a, String>;

/// The value after `flag`, parsed (thousands separators allowed) and accepted
/// by `ok`; otherwise the error "`flag` needs `what`".
pub fn value<T: FromStr>(
    it: &mut Args,
    flag: &str,
    what: &str,
    ok: impl Fn(&T) -> bool,
) -> Result<T, String> {
    it.next()
        .and_then(|v| v.replace(',', "").parse().ok())
        .filter(|v| ok(v))
        .ok_or_else(|| format!("{flag} needs {what}"))
}

/// A whole number greater than zero after `flag`.
pub fn positive<T: FromStr + PartialOrd + Default>(it: &mut Args, flag: &str) -> Result<T, String> {
    value(it, flag, "a positive number", |n: &T| *n > T::default())
}

/// A finite number of zero or more after `flag`.
pub fn non_negative(it: &mut Args, flag: &str, what: &str) -> Result<f64, String> {
    value(it, flag, what, |v: &f64| v.is_finite() && *v >= 0.0)
}

/// The folder after `flag`.
pub fn folder(it: &mut Args, flag: &str) -> Result<PathBuf, String> {
    it.next()
        .map(PathBuf::from)
        .ok_or_else(|| format!("{flag} needs a folder"))
}

/// A date written exactly as `YYYY-MM-DD`; `example` is shown in the error.
/// (Only the network commands take a date.)
#[cfg_attr(not(feature = "net"), allow(dead_code))]
pub fn date(text: &str, example: &str) -> Result<Date, String> {
    text.parse::<Date>()
        .ok()
        .filter(|_| text.len() == 10)
        .ok_or_else(|| format!("{text:?} is not a date like {example}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(list: &[&str]) -> Vec<String> {
        list.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn helpers_parse_and_word_errors_consistently() {
        let a = args(&["5", "0", "x", "1,000"]);
        let mut it = a.iter();
        assert_eq!(positive::<usize>(&mut it, "--limit"), Ok(5));
        assert_eq!(
            positive::<usize>(&mut it, "--limit"),
            Err("--limit needs a positive number".into())
        );
        assert_eq!(
            positive::<usize>(&mut it, "--limit"),
            Err("--limit needs a positive number".into())
        );
        assert_eq!(
            non_negative(&mut it, "--min-value", "a number of dollars"),
            Ok(1000.0)
        );
        assert_eq!(
            positive::<usize>(&mut it, "--limit"),
            Err("--limit needs a positive number".into()),
            "nothing left"
        );

        let b = args(&["data/x"]);
        assert_eq!(folder(&mut b.iter(), "--out"), Ok(PathBuf::from("data/x")));
        assert_eq!(
            folder(&mut [].iter(), "--out"),
            Err("--out needs a folder".into())
        );

        assert_eq!(
            date("2026-11-18", "2026-11-18"),
            Ok(Date::from_ymd(2026, 11, 18).unwrap())
        );
        assert_eq!(
            date("yesterday", "2026-11-18"),
            Err("\"yesterday\" is not a date like 2026-11-18".into())
        );
        assert!(date("2026-11-18x", "2026-11-18").is_err());
        assert!(date("2026-13-01", "2026-11-18").is_err());
    }
}
