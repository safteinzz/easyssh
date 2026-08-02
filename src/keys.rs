//! Discover the SSH keypairs sitting in `~/.ssh` so the TUI can list them,
//! generate new ones, and copy them to hosts - the "organizing keys? WHEN is a
//! mess" problem. We treat any `*.pub` with a matching private file as a key.

use std::collections::HashMap;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

/// One keypair on disk, with the human-facing details pulled from its `.pub`.
pub struct Key {
    /// Private key path (the thing you pass to `ssh -i`).
    pub path: PathBuf,
    /// Algorithm, e.g. `ssh-ed25519`, prettified to `ED25519`.
    pub kind: String,
    /// Trailing comment (usually the email you gave `-C`).
    pub comment: String,
    /// `SHA256:…` fingerprint from `ssh-keygen -lf`.
    pub fingerprint: String,
    /// Hosts this key has been copied to (via ssh-copy-id), newest first.
    pub copied_to: Vec<String>,
}

impl Key {
    /// Just the filename of the private key (what you'd recognize it by).
    pub fn name(&self) -> String {
        self.path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default()
    }
}

/// Every keypair in `~/.ssh`, sorted by filename. A `.pub` without its private
/// counterpart is skipped - you can't `ssh -i` half a pair.
pub fn list() -> Vec<Key> {
    let dir = crate::sshcfg::ssh_dir();
    let mut keys = Vec::new();

    let Ok(entries) = fs::read_dir(&dir) else {
        return keys;
    };

    let history = copy_history();
    for entry in entries.flatten() {
        let pubpath = entry.path();
        // We iterate `.pub` files and derive the private path by dropping `.pub`.
        if pubpath.extension().and_then(|e| e.to_str()) != Some("pub") {
            continue;
        }
        let priv_path = pubpath.with_extension(""); // strip ".pub"
        if !priv_path.exists() {
            continue;
        }

        let name = priv_path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        let (kind, comment) = parse_pub(&pubpath);
        keys.push(Key {
            fingerprint: fingerprint(&pubpath),
            copied_to: history.get(&name).cloned().unwrap_or_default(),
            path: priv_path,
            kind,
            comment,
        });
    }

    keys.sort_by_key(|k| k.name());
    keys
}

/// Where the copy log lives: `~/.local/state/easyssh/copied-keys.tsv`.
fn history_path() -> PathBuf {
    dirs::state_dir()
        .or_else(dirs::data_local_dir)
        .unwrap_or_else(|| PathBuf::from("."))
        .join("easyssh")
        .join("copied-keys.tsv")
}

/// Append a `key -> host` record after a successful ssh-copy-id, so the Keys tab
/// can show where each key has been installed. TSV: `epoch\tkeyname\thost`.
pub fn record_copy(key_name: &str, host: &str) {
    let path = history_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let stamp = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    use std::io::Write;
    if let Ok(mut f) = fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "{stamp}\t{key_name}\t{host}");
    }
}

/// Map of key name to the hosts it was copied to, newest first, deduped.
fn copy_history() -> HashMap<String, Vec<String>> {
    let mut map: HashMap<String, Vec<String>> = HashMap::new();
    let Ok(text) = fs::read_to_string(history_path()) else {
        return map;
    };
    // Read newest first so the most recent copy of a key/host pair wins its slot.
    for line in text.lines().rev() {
        let mut f = line.split('\t');
        let (Some(_stamp), Some(name), Some(host)) = (f.next(), f.next(), f.next()) else {
            continue;
        };
        let hosts = map.entry(name.to_string()).or_default();
        if !hosts.iter().any(|h| h == host) {
            hosts.push(host.to_string());
        }
    }
    map
}

/// A `.pub` line is `"<type> <base64> [comment...]"`. Return (pretty type, comment).
fn parse_pub(pubpath: &PathBuf) -> (String, String) {
    let text = fs::read_to_string(pubpath).unwrap_or_default();
    let mut parts = text.split_whitespace();
    let kind = parts.next().unwrap_or("").to_string();
    // Skip the base64 blob; whatever remains is the comment.
    let _blob = parts.next();
    let comment = parts.collect::<Vec<_>>().join(" ");
    (pretty_kind(&kind), comment)
}

/// `ssh-ed25519` → `ED25519`, `ssh-rsa` → `RSA`, `ecdsa-sha2-nistp256` → `ECDSA`.
fn pretty_kind(kind: &str) -> String {
    let k = kind.trim_start_matches("ssh-");
    let base = k.split('-').next().unwrap_or(k);
    base.to_ascii_uppercase()
}

/// Ask `ssh-keygen -lf` for the fingerprint; its output is
/// `"<bits> SHA256:<hash> <comment> (<TYPE>)"`. We keep just the `SHA256:…`.
fn fingerprint(pubpath: &PathBuf) -> String {
    let out = Command::new("ssh-keygen").arg("-lf").arg(pubpath).output();
    match out {
        Ok(o) if o.status.success() => String::from_utf8_lossy(&o.stdout)
            .split_whitespace()
            .find(|t| t.starts_with("SHA256:"))
            .unwrap_or("")
            .to_string(),
        _ => String::new(),
    }
}
