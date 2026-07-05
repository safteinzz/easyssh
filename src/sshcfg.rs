//! Read and write `~/.ssh/config` — the backbone every other feature keys off.
//!
//! We deliberately do NOT reimplement ssh's config semantics; we parse just
//! enough to *list* hosts (replacing the classic `grep '^Host' ~/.ssh/config`
//! alias) and to *append* new host blocks safely. Writes always back the file
//! up first and only ever append — your hand edits, comments and ordering are
//! never rewritten.

use anyhow::{Context, Result};
use std::fs;
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// One resolved host entry from the config. `alias` is what you type after
/// `ssh`; the rest are the settings we care to surface. A block listing several
/// aliases produces one `Host` per alias, all sharing the block's settings.
#[derive(Clone)]
pub struct Host {
    pub alias: String,
    pub hostname: Option<String>,
    pub user: Option<String>,
    pub port: Option<String>,
    pub identity: Option<String>,
}

impl Host {
    /// `user@hostname:port`-ish summary for the verbose listing / TUI second
    /// column. Falls back to the alias when no HostName is set.
    pub fn target(&self) -> String {
        let host = self.hostname.clone().unwrap_or_else(|| self.alias.clone());
        let mut s = match &self.user {
            Some(u) => format!("{u}@{host}"),
            None => host,
        };
        if let Some(p) = &self.port {
            s.push_str(&format!(":{p}"));
        }
        s
    }
}

/// Fields the add-host wizard collects. Empty strings mean "not set" and are
/// omitted from the written block.
pub struct NewHost {
    pub alias: String,
    pub hostname: String,
    pub user: String,
    pub port: String,
    pub identity: String,
}

/// `~/.ssh`, honoring $HOME. Everything hangs off here.
pub fn ssh_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_default().join(".ssh")
}

/// The main config file we read from and append to.
pub fn config_path() -> PathBuf {
    ssh_dir().join("config")
}

/// Every host defined in the config (and any `Include`d files), excluding
/// wildcard/pattern blocks like `Host *` which aren't real destinations.
/// Sorted by alias so the listing and TUI are stable.
pub fn list_hosts() -> Vec<Host> {
    let mut hosts = Vec::new();
    parse_file(&config_path(), &mut hosts);
    hosts.retain(|h| !is_pattern(&h.alias));
    hosts.sort_by(|a, b| a.alias.cmp(&b.alias));
    hosts
}

/// True for aliases that are match patterns rather than concrete hosts.
fn is_pattern(alias: &str) -> bool {
    alias.contains('*') || alias.contains('?') || alias.contains('!')
}

/// Recursively parse one config file, appending every host block it finds.
/// Unknown keywords are ignored; we only pick out the handful we display.
fn parse_file(path: &Path, out: &mut Vec<Host>) {
    let Ok(text) = fs::read_to_string(path) else {
        return; // missing/unreadable file → nothing to add, not an error here
    };

    // Settings accumulate under the most recent `Host` line; `aliases` holds the
    // patterns that line named. We flush into `out` when a new Host line (or EOF)
    // arrives so multi-alias blocks fan out into one entry per alias.
    let mut aliases: Vec<String> = Vec::new();
    let mut hostname = None;
    let mut user = None;
    let mut port = None;
    let mut identity = None;

    // Closure-free flush would need to borrow all the locals mutably; a small
    // helper macro keeps the loop readable without fighting the borrow checker.
    // It only emits the accumulated block; the per-block settings are reset in
    // the `host` arm below (so the final EOF flush leaves no dead writes).
    macro_rules! flush {
        () => {
            for a in aliases.drain(..) {
                out.push(Host {
                    alias: a,
                    hostname: hostname.clone(),
                    user: user.clone(),
                    port: port.clone(),
                    identity: identity.clone(),
                });
            }
        };
    }

    for raw in text.lines() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        // ssh config is "keyword value" separated by whitespace (or '='); keys
        // are case-insensitive.
        let (key, value) = split_kv(line);
        let key_lc = key.to_ascii_lowercase();

        match key_lc.as_str() {
            "host" => {
                flush!();
                // Start a fresh block: forget the previous one's settings.
                hostname = None;
                user = None;
                port = None;
                identity = None;
                aliases = value.split_whitespace().map(str::to_string).collect();
            }
            "include" => {
                // Includes are relative to ~/.ssh; expand a trailing `*` glob so
                // split configs (`Include config.d/*`) still list.
                for inc in expand_include(value) {
                    parse_file(&inc, out);
                }
            }
            _ if aliases.is_empty() => {} // settings before any Host line: ignore
            "hostname" => hostname = Some(value.to_string()),
            "user" => user = Some(value.to_string()),
            "port" => port = Some(value.to_string()),
            "identityfile" => identity = Some(value.to_string()),
            _ => {}
        }
    }
    flush!();
}

/// Split a config line into (keyword, value), accepting either whitespace or an
/// `=` as the separator, per ssh_config(5).
fn split_kv(line: &str) -> (&str, &str) {
    let sep = line.find(|c: char| c.is_whitespace() || c == '=');
    match sep {
        Some(i) => (line[..i].trim_end_matches('='), line[i + 1..].trim_start_matches(['=', ' ', '\t'])),
        None => (line, ""),
    }
}

/// Turn an `Include` value into concrete paths: expand `~`, resolve relative to
/// `~/.ssh`, and handle a single `*` glob in the final path component.
fn expand_include(value: &str) -> Vec<PathBuf> {
    let mut out = Vec::new();
    for token in value.split_whitespace() {
        let expanded = if let Some(rest) = token.strip_prefix("~/") {
            dirs::home_dir().unwrap_or_default().join(rest)
        } else {
            let p = Path::new(token);
            if p.is_absolute() { p.to_path_buf() } else { ssh_dir().join(p) }
        };

        match expanded.file_name().and_then(|n| n.to_str()) {
            Some(name) if name.contains('*') => {
                // Glob the parent dir against the `*` pattern (only `*` supported).
                if let Some(parent) = expanded.parent() {
                    if let Ok(entries) = fs::read_dir(parent) {
                        for e in entries.flatten() {
                            let fname = e.file_name();
                            if glob_match(name, &fname.to_string_lossy()) {
                                out.push(e.path());
                            }
                        }
                    }
                }
            }
            _ => out.push(expanded),
        }
    }
    out
}

/// Minimal glob: only `*` (any run of chars). Enough for `config.d/*` style
/// includes without pulling in a glob crate.
fn glob_match(pattern: &str, name: &str) -> bool {
    match pattern.split_once('*') {
        None => pattern == name,
        Some((pre, suf)) => name.starts_with(pre) && name.ends_with(suf) && name.len() >= pre.len() + suf.len(),
    }
}

/// Append a new `Host` block to the main config. Backs the file up first, makes
/// sure `~/.ssh` (0700) and `config` (0600) have safe permissions, and never
/// touches existing content.
pub fn add_host(h: &NewHost) -> Result<PathBuf> {
    let dir = ssh_dir();
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    harden(&dir, 0o700);

    let cfg = config_path();
    let mut backup = None;
    if cfg.exists() {
        let b = backup_path(&cfg);
        fs::copy(&cfg, &b).with_context(|| format!("backing up config to {}", b.display()))?;
        backup = Some(b);
    }

    let mut block = String::new();
    // Blank line before the block keeps it visually separated from prior entries.
    block.push_str(&format!("\nHost {}\n", h.alias.trim()));
    push_field(&mut block, "HostName", &h.hostname);
    push_field(&mut block, "User", &h.user);
    push_field(&mut block, "Port", &h.port);
    push_field(&mut block, "IdentityFile", &h.identity);

    let mut existing = fs::read_to_string(&cfg).unwrap_or_default();
    existing.push_str(&block);
    fs::write(&cfg, existing).with_context(|| format!("writing {}", cfg.display()))?;
    harden(&cfg, 0o600);

    Ok(backup.unwrap_or(cfg))
}

fn push_field(block: &mut String, key: &str, value: &str) {
    let v = value.trim();
    if !v.is_empty() {
        block.push_str(&format!("    {key} {v}\n"));
    }
}

/// `config` → `config.bak.<epoch>` so repeated adds don't clobber one backup.
fn backup_path(cfg: &Path) -> PathBuf {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_secs()).unwrap_or(0);
    cfg.with_extension(format!("bak.{secs}"))
}

/// Best-effort chmod; ssh refuses configs/keys that are group/world accessible,
/// so we proactively lock them down. Ignored on failure (e.g. non-unix).
fn harden(path: &Path, mode: u32) {
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let _ = fs::set_permissions(path, fs::Permissions::from_mode(mode));
    }
    #[cfg(not(unix))]
    let _ = (path, mode);
}
