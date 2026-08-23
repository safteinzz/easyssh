//! Discover the SSH keypairs sitting in `~/.ssh` so the TUI can list them,
//! generate new ones, and copy them to hosts - the "organizing keys? WHEN is a
//! mess" problem. We treat any `*.pub` with a matching private file as a key.
//!
//! Beyond what is written in the file we answer the two questions you actually
//! have in front of a key: is it loaded in the agent (so a connect will just
//! work), and is it passphrase-protected (so it will ask). Both come from the
//! real tools - `ssh-add -l` and `ssh-keygen` - never from a guess.

use std::collections::HashSet;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

/// One keypair on disk, with the human-facing details pulled from its `.pub`.
pub struct Key {
    /// Private key path (the thing you pass to `ssh -i`).
    pub path: PathBuf,
    /// Algorithm, e.g. `ssh-ed25519`, prettified to `ED25519`.
    pub kind: String,
    /// Key size in bits, as `ssh-keygen -lf` reports it.
    pub bits: String,
    /// Trailing comment (usually the email you gave `-C`).
    pub comment: String,
    /// `SHA256:…` fingerprint from `ssh-keygen -lf`.
    pub fingerprint: String,
    /// This key's fingerprint is currently loaded in ssh-agent, so connecting
    /// with it will not ask for anything.
    pub agent_loaded: bool,
    /// The private key on disk is encrypted with a passphrase.
    pub encrypted: bool,
    /// Config aliases whose `IdentityFile` points at this key. Filled in by
    /// `link_hosts`, because only the caller knows the host list.
    pub used_by: Vec<String>,
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
    // One `ssh-add -l` for the whole listing rather than one per key.
    let loaded = agent_fingerprints();

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
        let (bits, fingerprint) = fingerprint(&pubpath);
        keys.push(Key {
            agent_loaded: loaded.contains(&fingerprint),
            encrypted: is_encrypted(&priv_path),
            fingerprint,
            bits,
            path: priv_path,
            kind,
            comment,
            used_by: Vec::new(),
        });
    }

    keys.sort_by_key(|k| k.name());
    keys
}

/// Fill in `used_by`: which config aliases pin this key with `IdentityFile`.
/// Paths are compared after `~` expansion, so `~/.ssh/id_rsa` and the absolute
/// form are the same key; a bare filename is matched against `~/.ssh` too.
pub fn link_hosts(keys: &mut [Key], hosts: &[crate::sshcfg::Host]) {
    for key in keys.iter_mut() {
        key.used_by.clear();
        for host in hosts {
            let Some(identity) = &host.identity else {
                continue;
            };
            if same_key(&key.path, identity) {
                key.used_by.push(host.alias.clone());
            }
        }
    }
}

/// Whether an `IdentityFile` value points at this private key file.
fn same_key(path: &Path, identity: &str) -> bool {
    // ssh strips surrounding quotes; a config written by hand often has them.
    let identity = identity.trim().trim_matches(['"', '\'']);
    let candidate = crate::sshcfg::expand_tilde(identity);
    if candidate == path {
        return true;
    }
    // A relative IdentityFile resolves against ~/.ssh, and `id_rsa.pub` in the
    // config still means the same pair as `id_rsa`.
    let stripped = identity.strip_suffix(".pub").unwrap_or(identity);
    let stripped = crate::sshcfg::expand_tilde(stripped);
    stripped == path || crate::sshcfg::ssh_dir().join(&stripped) == path
}

/// Fingerprints of everything ssh-agent currently holds. An agent that is not
/// running, or holds nothing, is an empty set - never an error, since a missing
/// agent just means every key shows as not loaded.
fn agent_fingerprints() -> HashSet<String> {
    let Ok(out) = Command::new("ssh-add").arg("-l").output() else {
        return HashSet::new();
    };
    if !out.status.success() {
        return HashSet::new();
    }
    // Each line is `<bits> SHA256:<hash> <comment> (<TYPE>)`.
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|l| {
            l.split_whitespace()
                .find(|t| t.starts_with("SHA256:"))
                .map(str::to_string)
        })
        .collect()
}

/// Is the private key passphrase-protected? `ssh-keygen -y` derives the public
/// key from the private one; handed an empty passphrase it succeeds only when
/// the key is unencrypted. `-P ""` is what stops it prompting.
fn is_encrypted(priv_path: &Path) -> bool {
    Command::new("ssh-keygen")
        .arg("-y")
        .arg("-P")
        .arg("")
        .arg("-f")
        .arg(priv_path)
        .output()
        .map(|o| !o.status.success())
        .unwrap_or(false)
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
/// `"<bits> SHA256:<hash> <comment> (<TYPE>)"`. We keep the size and the hash.
fn fingerprint(pubpath: &PathBuf) -> (String, String) {
    let out = Command::new("ssh-keygen").arg("-lf").arg(pubpath).output();
    match out {
        Ok(o) if o.status.success() => {
            let text = String::from_utf8_lossy(&o.stdout);
            let mut fields = text.split_whitespace();
            let bits = fields.next().unwrap_or("").to_string();
            let fp = fields
                .find(|t| t.starts_with("SHA256:"))
                .unwrap_or("")
                .to_string();
            (bits, fp)
        }
        _ => (String::new(), String::new()),
    }
}
