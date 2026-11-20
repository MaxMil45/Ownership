//! The log of alerts already delivered: one line per delivery, appended,
//! never rewritten. A line is `<UTC time>\t<destination>\t<alert key>`.
//! An alert counts as sent to a destination once a line for both exists, so
//! a failed delivery is simply tried again next time.

use std::collections::HashSet;
use std::io::Write;
use std::path::{Path, PathBuf};

#[derive(Debug, Default)]
pub struct SentLog {
    path: Option<PathBuf>,
    done: HashSet<(String, String)>,
}

impl SentLog {
    /// A log kept only in memory (for `--dry-run` and tests).
    pub fn in_memory() -> SentLog {
        SentLog::default()
    }

    /// Reads the log at `path`; a missing file is an empty log.
    pub fn open(path: &Path) -> Result<SentLog, String> {
        let mut log = SentLog {
            path: Some(path.to_path_buf()),
            done: HashSet::new(),
        };
        match std::fs::read_to_string(path) {
            Ok(text) => log.done = parse(&text),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
            Err(e) => return Err(format!("{}: {e}", path.display())),
        }
        Ok(log)
    }

    pub fn contains(&self, destination: &str, key: &str) -> bool {
        self.done
            .contains(&(destination.to_string(), key.to_string()))
    }

    pub fn len(&self) -> usize {
        self.done.len()
    }

    pub fn is_empty(&self) -> bool {
        self.done.is_empty()
    }

    /// Records one delivery, on disk first when the log has a file.
    pub fn record(&mut self, destination: &str, key: &str, at_utc: &str) -> Result<(), String> {
        if let Some(path) = &self.path {
            if let Some(dir) = path.parent().filter(|d| !d.as_os_str().is_empty()) {
                std::fs::create_dir_all(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
            }
            let mut f = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(path)
                .map_err(|e| format!("{}: {e}", path.display()))?;
            writeln!(f, "{}\t{}\t{}", at_utc, clean(destination), clean(key))
                .map_err(|e| format!("{}: {e}", path.display()))?;
        }
        self.done.insert((destination.to_string(), key.to_string()));
        Ok(())
    }
}

/// Tabs and line breaks would break the line format.
fn clean(s: &str) -> String {
    s.replace(['\t', '\n', '\r'], " ")
}

fn parse(text: &str) -> HashSet<(String, String)> {
    text.lines()
        .filter_map(|l| {
            let mut parts = l.splitn(3, '\t');
            let _time = parts.next()?;
            Some((parts.next()?.to_string(), parts.next()?.to_string()))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_recorded_delivery_survives_reopening_and_is_per_destination() {
        let dir = std::env::temp_dir().join(format!("oe-sent-{}", std::process::id()));
        let path = dir.join("nested").join("sent.tsv");
        let mut log = SentLog::open(&path).unwrap();
        assert!(log.is_empty());
        log.record("discord", "rule|1|2026-11-02", "2026-11-20T15:00:00Z")
            .unwrap();
        log.record("webhook", "rule|1|2026-11-02", "2026-11-20T15:00:01Z")
            .unwrap();
        let again = SentLog::open(&path).unwrap();
        std::fs::remove_dir_all(&dir).unwrap();
        assert!(again.contains("discord", "rule|1|2026-11-02"));
        assert!(again.contains("webhook", "rule|1|2026-11-02"));
        assert!(!again.contains("console", "rule|1|2026-11-02"));
        assert_eq!(again.len(), 2);
    }

    #[test]
    fn tabs_in_keys_cannot_break_the_format() {
        assert_eq!(clean("a\tb\nc"), "a b c");
        let parsed = parse("t\tdiscord\ta|b c\nbroken line\n");
        assert!(parsed.contains(&("discord".to_string(), "a|b c".to_string())));
        assert_eq!(parsed.len(), 1);
    }

    #[test]
    fn in_memory_logs_write_nothing() {
        let mut log = SentLog::in_memory();
        log.record("console", "k", "t").unwrap();
        assert!(log.contains("console", "k"));
    }
}
