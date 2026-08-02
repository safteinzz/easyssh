//! Read and write `~/.ssh/config` - the backbone every other feature keys off.
//!
//! We deliberately do NOT reimplement ssh's config semantics; we parse just
//! enough to *list* hosts (replacing the classic `grep '^Host' ~/.ssh/config`
//! alias) and to *append* new host blocks safely. Writes always back the file
//! up first and only ever append - your hand edits, comments and ordering are
//! never rewritten.

use anyhow::{bail, Context, Result};
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
    if let Ok(text) = fs::read_to_string(path) {
        parse_lines(&text, true, out);
    }
}

/// Parse config text into hosts. With `follow_includes`, `Include` directives are
/// resolved and recursed into; tests pass `false` to keep parsing pure (no disk).
fn parse_lines(text: &str, follow_includes: bool, out: &mut Vec<Host>) {
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
                // split configs (`Include config.d/*`) still list. Skipped when
                // parsing pure text (tests) so nothing touches disk.
                if follow_includes {
                    for inc in expand_include(value) {
                        parse_file(&inc, out);
                    }
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
                if let Some(parent) = expanded.parent()
                    && let Ok(entries) = fs::read_dir(parent)
                {
                    for e in entries.flatten() {
                        let fname = e.file_name();
                        if glob_match(name, &fname.to_string_lossy()) {
                            out.push(e.path());
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

// ---- writing: add / edit / delete host blocks -------------------------------
//
// Every writer backs the file up first. `add` is append-only; `edit`/`delete`
// do minimal block surgery - they rewrite only the target block and copy every
// other line through verbatim. The `*_in` helpers take an explicit path so the
// tests can exercise them against a temp file instead of the real ~/.ssh/config.

/// Append a new `Host` block to the main config (never touches existing entries).
pub fn add_host(h: &NewHost) -> Result<()> {
    let dir = ssh_dir();
    fs::create_dir_all(&dir).with_context(|| format!("creating {}", dir.display()))?;
    harden(&dir, 0o700);
    let cfg = config_path();
    append_host_in(&cfg, h)?;
    harden(&cfg, 0o600);
    Ok(())
}

/// Replace the block that defines `original` (which must be the *sole* alias on
/// its `Host` line) with a freshly rendered one.
pub fn update_host(original: &str, h: &NewHost) -> Result<()> {
    let cfg = config_path();
    update_host_in(&cfg, original, h)?;
    harden(&cfg, 0o600);
    Ok(())
}

/// Remove the block that defines `alias` (sole alias only).
pub fn delete_host(alias: &str) -> Result<()> {
    delete_host_in(&config_path(), alias)
}

fn append_host_in(cfg: &Path, h: &NewHost) -> Result<()> {
    if cfg.exists() {
        backup(cfg)?;
    }
    let mut text = fs::read_to_string(cfg).unwrap_or_default();
    if !text.is_empty() && !text.ends_with('\n') {
        text.push('\n');
    }
    text.push('\n'); // blank line separates the new block from what precedes it
    text.push_str(&render_block(h));
    fs::write(cfg, text).with_context(|| format!("writing {}", cfg.display()))
}

fn update_host_in(cfg: &Path, original: &str, h: &NewHost) -> Result<()> {
    let text = fs::read_to_string(cfg).with_context(|| format!("reading {}", cfg.display()))?;
    let lines: Vec<&str> = text.lines().collect();
    match sole_block_range(&lines, original) {
        BlockFind::None => bail!("no host '{original}' in {}", cfg.display()),
        BlockFind::Shared => bail!("'{original}' shares a Host block with other aliases - edit {} by hand", cfg.display()),
        BlockFind::Range(s, e) => {
            backup(cfg)?;
            let mut out: Vec<String> = lines[..s].iter().map(|l| l.to_string()).collect();
            out.extend(render_block_lines(h));
            out.extend(lines[e..].iter().map(|l| l.to_string()));
            write_lines(cfg, &out)
        }
    }
}

fn delete_host_in(cfg: &Path, alias: &str) -> Result<()> {
    let text = fs::read_to_string(cfg).with_context(|| format!("reading {}", cfg.display()))?;
    let lines: Vec<&str> = text.lines().collect();
    match sole_block_range(&lines, alias) {
        BlockFind::None => bail!("no host '{alias}' in {}", cfg.display()),
        BlockFind::Shared => bail!("'{alias}' shares a Host block with other aliases - edit {} by hand", cfg.display()),
        BlockFind::Range(mut s, e) => {
            // Also drop the blank separator line just above the block, if any.
            if s > 0 && lines[s - 1].trim().is_empty() {
                s -= 1;
            }
            backup(cfg)?;
            let out: Vec<String> = lines[..s].iter().chain(lines[e..].iter()).map(|l| l.to_string()).collect();
            write_lines(cfg, &out)
        }
    }
}

fn write_lines(cfg: &Path, lines: &[String]) -> Result<()> {
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    fs::write(cfg, out).with_context(|| format!("writing {}", cfg.display()))
}

/// Copy the config aside as `config.bak.<epoch>` before any in-place edit.
fn backup(cfg: &Path) -> Result<()> {
    if cfg.exists() {
        let b = backup_path(cfg);
        fs::copy(cfg, &b).with_context(|| format!("backing up to {}", b.display()))?;
    }
    Ok(())
}

/// The lines of a Host block, no trailing blank - e.g. `["Host x", "    HostName y"]`.
fn render_block_lines(h: &NewHost) -> Vec<String> {
    let mut v = vec![format!("Host {}", h.alias.trim())];
    for (key, val) in [
        ("HostName", &h.hostname),
        ("User", &h.user),
        ("Port", &h.port),
        ("IdentityFile", &h.identity),
    ] {
        let val = val.trim();
        if !val.is_empty() {
            v.push(format!("    {key} {val}"));
        }
    }
    v
}

fn render_block(h: &NewHost) -> String {
    let mut s = render_block_lines(h).join("\n");
    s.push('\n');
    s
}

/// Where the sole-alias block for `alias` sits in `lines` (a half-open range).
enum BlockFind {
    None,
    Shared,
    Range(usize, usize),
}

/// Locate the `Host` block naming `alias`. Refuses (`Shared`) when the alias
/// shares its `Host` line with others, since we can't rewrite one in isolation.
/// A block runs from its `Host` line to the next `Host`/`Match` line (trailing
/// blank lines excluded).
fn sole_block_range(lines: &[&str], alias: &str) -> BlockFind {
    let mut i = 0;
    while i < lines.len() {
        let line = lines[i].trim();
        if line.is_empty() || line.starts_with('#') {
            i += 1;
            continue;
        }
        let (key, value) = split_kv(line);
        if key.eq_ignore_ascii_case("host") {
            let aliases: Vec<&str> = value.split_whitespace().collect();
            if aliases.contains(&alias) {
                if aliases.len() != 1 {
                    return BlockFind::Shared;
                }
                let mut end = i + 1;
                while end < lines.len() {
                    let l = lines[end].trim();
                    if !l.is_empty() && !l.starts_with('#') {
                        let (k, _) = split_kv(l);
                        if k.eq_ignore_ascii_case("host") || k.eq_ignore_ascii_case("match") {
                            break;
                        }
                    }
                    end += 1;
                }
                while end > i + 1 && lines[end - 1].trim().is_empty() {
                    end -= 1;
                }
                return BlockFind::Range(i, end);
            }
        }
        i += 1;
    }
    BlockFind::None
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Parse text into hosts without touching disk (no include following).
    fn parse_str(text: &str) -> Vec<Host> {
        let mut v = Vec::new();
        parse_lines(text, false, &mut v);
        v
    }

    /// A throwaway config file under the temp dir, uniquely named per test.
    fn temp_cfg(body: &str) -> PathBuf {
        let stamp = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("easyssh-test-{}-{stamp}.cfg", std::process::id()));
        fs::write(&path, body).unwrap();
        path
    }

    #[test]
    fn parses_fields_and_multi_alias() {
        let cfg = "\
Host raspi
    HostName 192.168.0.5
    User pi
    Port 22

Host a b
    HostName ex.com
";
        let hosts = parse_str(cfg);
        assert_eq!(hosts.len(), 3);
        let raspi = hosts.iter().find(|h| h.alias == "raspi").unwrap();
        assert_eq!(raspi.hostname.as_deref(), Some("192.168.0.5"));
        assert_eq!(raspi.user.as_deref(), Some("pi"));
        // A multi-alias `Host` line fans out into one entry each, sharing settings.
        let a = hosts.iter().find(|h| h.alias == "a").unwrap();
        let b = hosts.iter().find(|h| h.alias == "b").unwrap();
        assert_eq!(a.hostname.as_deref(), Some("ex.com"));
        assert_eq!(b.hostname.as_deref(), Some("ex.com"));
    }

    #[test]
    fn equals_separator_and_comments() {
        let hosts = parse_str("# a comment\nHost=x\n  HostName=1.2.3.4\n");
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "x");
        assert_eq!(hosts[0].hostname.as_deref(), Some("1.2.3.4"));
    }

    #[test]
    fn patterns_flagged() {
        assert!(is_pattern("*"));
        assert!(is_pattern("web-?"));
        assert!(!is_pattern("raspi"));
    }

    #[test]
    fn append_then_parse_roundtrip() {
        let cfg = temp_cfg("");
        let nh = NewHost {
            alias: "box".into(),
            hostname: "10.0.0.1".into(),
            user: "me".into(),
            port: "22".into(),
            identity: String::new(),
        };
        append_host_in(&cfg, &nh).unwrap();
        let hosts = parse_str(&fs::read_to_string(&cfg).unwrap());
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "box");
        assert_eq!(hosts[0].user.as_deref(), Some("me"));
        fs::remove_file(&cfg).ok();
    }

    #[test]
    fn delete_removes_only_target() {
        let cfg = temp_cfg("Host a\n    HostName 1\n\nHost b\n    HostName 2\n");
        delete_host_in(&cfg, "a").unwrap();
        let hosts = parse_str(&fs::read_to_string(&cfg).unwrap());
        assert_eq!(hosts.len(), 1);
        assert_eq!(hosts[0].alias, "b");
        assert_eq!(hosts[0].hostname.as_deref(), Some("2"));
        fs::remove_file(&cfg).ok();
    }

    #[test]
    fn delete_refuses_shared_block() {
        let cfg = temp_cfg("Host a b\n    HostName 1\n");
        assert!(delete_host_in(&cfg, "a").is_err());
        fs::remove_file(&cfg).ok();
    }

    #[test]
    fn update_replaces_block_leaving_others() {
        let cfg = temp_cfg("Host a\n    HostName old\n\nHost b\n    HostName 2\n");
        let nh = NewHost {
            alias: "a".into(),
            hostname: "new".into(),
            user: String::new(),
            port: String::new(),
            identity: String::new(),
        };
        update_host_in(&cfg, "a", &nh).unwrap();
        let hosts = parse_str(&fs::read_to_string(&cfg).unwrap());
        assert_eq!(hosts.iter().find(|h| h.alias == "a").unwrap().hostname.as_deref(), Some("new"));
        assert!(hosts.iter().any(|h| h.alias == "b"), "sibling block must survive");
        fs::remove_file(&cfg).ok();
    }
}
