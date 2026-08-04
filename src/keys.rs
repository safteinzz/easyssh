//! Discover the SSH keypairs sitting in `~/.ssh` so the TUI can list them,
//! generate new ones, and copy them to hosts - the "organizing keys? WHEN is a
//! mess" problem. We treat any `*.pub` with a matching private file as a key.

use std::fs;
use std::path::PathBuf;
use std::process::Command;

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

        let (kind, comment) = parse_pub(&pubpath);
        keys.push(Key {
            fingerprint: fingerprint(&pubpath),
            path: priv_path,
            kind,
            comment,
        });
    }

    keys.sort_by_key(|k| k.name());
    keys
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
