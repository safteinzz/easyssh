//! Which hosts you actually use. Every connect is stamped here so the Hosts tab
//! can lead with the box you were on ten minutes ago instead of whatever the
//! alphabet put first - the list you want is the list you used.
//!
//! The store is one small text file, `<state dir>/easyssh/history`, with one
//! line per alias: `<epoch> <count> <alias>`. No dependency, no schema, and a
//! corrupt or missing line just drops that host back to "never connected".

use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// What we remember about one alias.
#[derive(Clone, Copy, Default)]
pub struct Entry {
    /// Epoch seconds of the most recent connect.
    pub last: u64,
    /// How many connects we have seen in total.
    pub count: u32,
}

/// The whole file, keyed by alias.
#[derive(Default)]
pub struct History {
    entries: HashMap<String, Entry>,
}

impl History {
    pub fn get(&self, alias: &str) -> Option<Entry> {
        self.entries.get(alias).copied()
    }

    /// Sort key for the Hosts list: most recently connected first, then every
    /// host you have never used, alphabetically. Returning the negated stamp
    /// keeps it a plain ascending sort at the call site.
    pub fn rank(&self, alias: &str) -> (i64, String) {
        let last = self.get(alias).map(|e| e.last).unwrap_or(0);
        (-(last as i64), alias.to_string())
    }
}

/// `~/.local/state/easyssh/history` (falling back to the data dir on platforms
/// with no state dir). Connect history is state, not config: losing it costs
/// you an ordering, nothing more.
pub fn path() -> PathBuf {
    let base = dirs::state_dir()
        .or_else(dirs::data_dir)
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".local/state"));
    base.join("easyssh").join("history")
}

pub fn load() -> History {
    load_from(&path())
}

/// Stamp a connect. Best-effort: a history we cannot write is not worth
/// interrupting a connect over, so every failure here is silent.
pub fn record(alias: &str) {
    let p = path();
    if let Some(dir) = p.parent() {
        let _ = fs::create_dir_all(dir);
    }
    let _ = record_in(&p, alias, now());
}

/// Parse the store. Unreadable file, unparseable line: both mean "no history",
/// never an error the caller has to handle.
pub fn load_from(path: &Path) -> History {
    let mut entries = HashMap::new();
    let Ok(text) = fs::read_to_string(path) else {
        return History { entries };
    };
    for line in text.lines() {
        // `<epoch> <count> <alias>`; the alias is last because ssh aliases
        // cannot contain whitespace, so it needs no quoting.
        let mut f = line.split_whitespace();
        let (Some(last), Some(count), Some(alias)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let (Ok(last), Ok(count)) = (last.parse(), count.parse()) else {
            continue;
        };
        entries.insert(alias.to_string(), Entry { last, count });
    }
    History { entries }
}

/// Read-modify-write one alias. Takes the path and the clock so tests can drive
/// it against a temp file without touching the real state dir.
pub fn record_in(path: &Path, alias: &str, at: u64) -> std::io::Result<()> {
    let mut h = load_from(path);
    let e = h.entries.entry(alias.to_string()).or_default();
    e.last = at;
    e.count += 1;

    // Sorted output so the file is diffable and the order never depends on the
    // hash map's iteration order.
    let mut rows: Vec<(&String, &Entry)> = h.entries.iter().collect();
    rows.sort_by(|a, b| a.0.cmp(b.0));
    let body: String = rows
        .iter()
        .map(|(alias, e)| format!("{} {} {}\n", e.last, e.count, alias))
        .collect();
    fs::write(path, body)
}

/// Epoch seconds now, or 0 if the clock is before the epoch.
pub fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// "2d ago" - a coarse, glanceable age. Precision past the largest unit is
/// noise here: you want to know whether you were on a box today or last month.
pub fn ago(then: u64, now: u64) -> String {
    let secs = now.saturating_sub(then);
    match secs {
        0..=59 => "just now".to_string(),
        60..=3599 => format!("{}m ago", secs / 60),
        3600..=86399 => format!("{}h ago", secs / 3600),
        86400..=2591999 => format!("{}d ago", secs / 86400),
        2592000..=31535999 => format!("{}mo ago", secs / 2592000),
        _ => format!("{}y ago", secs / 31536000),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A throwaway history file under the temp dir, so the suite never touches
    /// the real `~/.local/state/easyssh/history`.
    fn temp_file() -> PathBuf {
        std::env::temp_dir().join(format!(
            "easyssh-history-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn no_file_means_no_history() {
        let h = load_from(&temp_file());
        assert!(h.get("raspi").is_none());
    }

    #[test]
    fn recording_stamps_the_time_and_counts() {
        let path = temp_file();
        record_in(&path, "raspi", 1_000).unwrap();
        record_in(&path, "raspi", 2_000).unwrap();
        record_in(&path, "web01", 1_500).unwrap();

        let h = load_from(&path);
        let raspi = h.get("raspi").unwrap();
        assert_eq!(raspi.count, 2, "a second connect adds to the count");
        assert_eq!(raspi.last, 2_000, "and the newest connect is the one kept");
        assert_eq!(h.get("web01").unwrap().count, 1);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn a_damaged_line_drops_only_that_host() {
        let path = temp_file();
        fs::write(&path, "not a row\n1000 x raspi\n2000 3 web01\n").unwrap();
        let h = load_from(&path);
        assert!(h.get("raspi").is_none(), "an unparseable count is skipped");
        assert_eq!(h.get("web01").unwrap().count, 3);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn most_recent_sorts_first_then_the_unused() {
        // This is the order the Hosts tab promises: what you used last, then
        // everything you have never opened, alphabetically.
        let path = temp_file();
        record_in(&path, "old", 1_000).unwrap();
        record_in(&path, "recent", 9_000).unwrap();
        let h = load_from(&path);

        let mut aliases = vec!["zeta", "old", "alpha", "recent"];
        aliases.sort_by_key(|a| h.rank(a));
        assert_eq!(aliases, vec!["recent", "old", "alpha", "zeta"]);
        fs::remove_file(&path).ok();
    }

    #[test]
    fn ago_reads_in_the_largest_unit_that_fits() {
        let now = 1_000_000_000;
        assert_eq!(ago(now, now), "just now");
        assert_eq!(ago(now - 59, now), "just now");
        assert_eq!(ago(now - 60, now), "1m ago");
        assert_eq!(ago(now - 3_600, now), "1h ago");
        assert_eq!(ago(now - 2 * 86_400, now), "2d ago");
        assert_eq!(ago(now - 40 * 86_400, now), "1mo ago");
        assert_eq!(ago(now - 400 * 86_400, now), "1y ago");
        // A clock that went backwards must not underflow into a huge age.
        assert_eq!(ago(now + 500, now), "just now");
    }
}
