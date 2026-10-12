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
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

/// One keypair on disk, with the human-facing details pulled from its `.pub`,
/// or a key only ssh-agent holds (added from elsewhere, or served by a token).
pub struct Key {
    /// Private key path (the thing you pass to `ssh -i`), `None` for a key only
    /// the agent holds.
    pub path: Option<PathBuf>,
    /// Algorithm, e.g. `ssh-ed25519`, prettified to `ED25519`.
    pub kind: String,
    /// Key size in bits, as `ssh-keygen -lf` reports it.
    pub bits: String,
    /// Trailing comment (usually the email you gave `-C`).
    pub comment: String,
    /// `SHA256:…` fingerprint from `ssh-keygen -lf`.
    pub fingerprint: String,
    /// This key's fingerprint is currently loaded in ssh-agent, so connecting
    /// with it will not ask for anything; `None` when no agent is running.
    pub agent_loaded: Option<bool>,
    /// The private key on disk is encrypted with a passphrase.
    pub encrypted: bool,
    /// Config aliases whose `IdentityFile` points at this key. Filled in by
    /// `link_hosts`, because only the caller knows the host list.
    pub used_by: Vec<String>,
}

impl Key {
    /// The filename of a private key in `~/.ssh` (what you'd recognize it by),
    /// the `~`-collapsed path of one elsewhere, or for a key only the agent
    /// holds its comment, else its fingerprint. Each is what `essh key install`
    /// takes.
    pub fn name(&self) -> String {
        match &self.path {
            Some(path) if path.parent() == Some(crate::sshcfg::ssh_dir().as_path()) => path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default(),
            Some(path) => crate::sshcfg::collapse_tilde(&path.to_string_lossy()),
            None if self.comment.is_empty() => self.fingerprint.clone(),
            None => self.comment.clone(),
        }
    }
}

/// The widest a key's name pads the name column to, so one long name pushes
/// only its own row out of line.
pub const NAME_COLUMN_MAX: usize = 24;

/// What `ssh-add -l` answered.
pub enum Agent {
    /// The keys it holds, possibly none.
    Holds(Vec<AgentKey>),
    /// There is no agent to ask.
    NotRunning,
    /// An agent, or an `ssh-add`, that could not be read, with what it said.
    Unreadable(String),
}

impl Agent {
    /// The keys it holds, or `None` when it could not be asked.
    pub fn keys(&self) -> Option<&[AgentKey]> {
        match self {
            Agent::Holds(keys) => Some(keys),
            _ => None,
        }
    }
}

/// One key ssh-agent holds, as `ssh-add -l` describes it.
pub struct AgentKey {
    pub bits: String,
    pub fingerprint: String,
    pub comment: String,
    /// The type `ssh-add -l` prints in brackets, e.g. `ED25519`.
    pub kind: String,
}

/// Every keypair in `~/.ssh`, then every key the agent holds that none of
/// them is.
pub fn list() -> Vec<Key> {
    list_with(&agent())
}

/// [`list`] against an agent already asked with [`agent`], so a caller that
/// also reports the agent asks it once. Keypairs come sorted by filename and
/// the agent's own keys after them by name. A `.pub` without its private
/// counterpart is skipped, because you can't `ssh -i` half a pair.
pub fn list_with(agent: &Agent) -> Vec<Key> {
    let dir = crate::sshcfg::ssh_dir();
    let mut keys = Vec::new();
    let held = agent.keys();
    let loaded: HashSet<&str> = held
        .unwrap_or_default()
        .iter()
        .map(|k| k.fingerprint.as_str())
        .collect();

    for entry in fs::read_dir(&dir).into_iter().flatten().flatten() {
        let pubpath = entry.path();
        // We iterate `.pub` files and derive the private path by dropping `.pub`.
        if pubpath.extension().and_then(|e| e.to_str()) != Some("pub") {
            continue;
        }
        let priv_path = pubpath.with_extension(""); // strip ".pub"
        if !priv_path.exists() {
            continue;
        }
        let mut key = read_pair(priv_path, &pubpath);
        key.agent_loaded = held.map(|_| loaded.contains(key.fingerprint.as_str()));
        keys.push(key);
    }
    keys.sort_by_key(|k| k.name());

    let on_disk: HashSet<String> = keys.iter().map(|k| k.fingerprint.clone()).collect();
    let mut agent_only: Vec<Key> = held
        .unwrap_or_default()
        .iter()
        .filter(|a| !on_disk.contains(&a.fingerprint))
        .map(|a| {
            let mut key = pair_outside(a).unwrap_or_else(|| Key {
                path: None,
                kind: a.kind.clone(),
                bits: a.bits.clone(),
                comment: a.comment.clone(),
                fingerprint: a.fingerprint.clone(),
                agent_loaded: None,
                encrypted: false,
                used_by: Vec::new(),
            });
            key.agent_loaded = Some(true);
            key
        })
        .collect();
    agent_only.sort_by_key(|k| k.name());
    // A key and its certificate are two agent keys read from one pair.
    agent_only.dedup_by(|a, b| a.path.is_some() && a.path == b.path);
    keys.extend(agent_only);
    keys
}

/// A keypair read from disk, with `agent_loaded` left for the caller.
fn read_pair(priv_path: PathBuf, pubpath: &PathBuf) -> Key {
    let (kind, comment) = parse_pub(pubpath);
    let (bits, fingerprint) = fingerprint(pubpath);
    Key {
        agent_loaded: None,
        encrypted: is_encrypted(&priv_path),
        fingerprint,
        bits,
        path: Some(priv_path),
        kind,
        comment,
        used_by: Vec::new(),
    }
}

/// The pair outside `~/.ssh` an agent key was added from, when its comment is
/// that path: `ssh-add` comments a key that has no comment of its own with the
/// file it read. Only a pair with the agent key's own fingerprint counts.
fn pair_outside(held: &AgentKey) -> Option<Key> {
    let priv_path = PathBuf::from(&held.comment);
    let pubpath = pub_path(&priv_path);
    if !priv_path.is_absolute() || !priv_path.is_file() || !pubpath.is_file() {
        return None;
    }
    let key = read_pair(priv_path, &pubpath);
    (key.fingerprint == held.fingerprint).then_some(key)
}

/// Fill in `used_by`: which config aliases pin this key with `IdentityFile`.
/// Paths are compared after `~` expansion, so `~/.ssh/id_rsa` and the absolute
/// form are the same key; a bare filename is matched against `~/.ssh` too.
pub fn link_hosts(keys: &mut [Key], hosts: &[crate::sshcfg::Host]) {
    for key in keys.iter_mut() {
        key.used_by.clear();
        let Some(path) = &key.path else {
            continue;
        };
        for host in hosts {
            let Some(identity) = &host.identity else {
                continue;
            };
            if same_key(path, identity) {
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

/// Ask ssh-agent what it holds. `ssh-add -l` exits 2 when it cannot reach an
/// agent, and 1 both for an empty one, which it says on stdout, and for a
/// failed fetch, which it explains on stderr.
pub fn agent() -> Agent {
    let Ok(out) = Command::new("ssh-add").arg("-l").output() else {
        return Agent::Unreadable("could not run `ssh-add`".into());
    };
    let said: Vec<&str> = std::str::from_utf8(&out.stderr)
        .unwrap_or_default()
        .lines()
        .filter(|l| !l.trim().is_empty())
        .take(5)
        .collect();
    match out.status.code() {
        Some(0) => {
            let text = String::from_utf8_lossy(&out.stdout);
            Agent::Holds(text.lines().filter_map(parse_agent_line).collect())
        }
        Some(1) if said.is_empty() => Agent::Holds(Vec::new()),
        Some(2) => Agent::NotRunning,
        _ if said.is_empty() => Agent::Unreadable("`ssh-add -l` failed".into()),
        _ => Agent::Unreadable(said.join(" ")),
    }
}

/// One line of `ssh-add -l`: `<bits> SHA256:<hash> <comment...> (<TYPE>)`,
/// where the comment may hold spaces.
fn parse_agent_line(line: &str) -> Option<AgentKey> {
    let (bits, rest) = line.trim().split_once(' ')?;
    let (fingerprint, rest) = rest.split_once(' ')?;
    let (comment, kind) = rest.rsplit_once(' ')?;
    let kind = kind.strip_prefix('(')?.strip_suffix(')')?;
    Some(AgentKey {
        bits: bits.to_string(),
        fingerprint: fingerprint.to_string(),
        comment: comment.to_string(),
        kind: kind.to_string(),
    })
}

/// The public key line the agent holds for this fingerprint and `ssh-add -l`
/// type (a key and its certificate share a fingerprint), for a key with no
/// `.pub` on disk to read it from; `None` once the agent no longer holds it.
pub fn agent_public(fingerprint: &str, kind: &str) -> Option<String> {
    let listed = Command::new("ssh-add").arg("-L").output().ok()?;
    if !listed.status.success() {
        return None;
    }
    // `ssh-keygen -lf -` prints one fingerprint per line it reads, in order,
    // which is what pairs each `ssh-add -L` line with its fingerprint.
    let mut keygen = Command::new("ssh-keygen")
        .args(["-l", "-f", "-"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    keygen.stdin.take()?.write_all(&listed.stdout).ok()?;
    let printed = keygen.wait_with_output().ok()?;
    let publics = String::from_utf8_lossy(&listed.stdout);
    let prints = String::from_utf8_lossy(&printed.stdout);
    let publics: Vec<&str> = publics.lines().collect();
    let prints: Vec<&str> = prints.lines().collect();
    if publics.len() != prints.len() {
        return None;
    }
    let kind = format!("({kind})");
    publics
        .into_iter()
        .zip(prints)
        .find(|(_, p)| {
            p.split_whitespace().nth(1) == Some(fingerprint)
                && p.split_whitespace().last() == Some(kind.as_str())
        })
        .map(|(public, _)| public.to_string())
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

/// Whether `path` holds a public key: named `*.pub`, or a one-line
/// `<type> <base64>` body, which is what ssh cannot log in with.
pub fn is_public_key(path: &Path) -> bool {
    if path.extension().is_some_and(|e| e == "pub") {
        return true;
    }
    // A key is a few kilobytes; anything bigger is not one, so it is not read.
    if fs::metadata(path).map_or(true, |m| m.len() > 64 * 1024) {
        return false;
    }
    let text = fs::read_to_string(path).unwrap_or_default();
    let first = text.split_whitespace().next().unwrap_or("");
    ["ssh-", "ecdsa-sha2-", "sk-ssh-", "sk-ecdsa-"]
        .iter()
        .any(|p| first.starts_with(p))
}

/// What `install_via_argv` exits with when the key was already listed there.
pub const ALREADY_AUTHORIZED: i32 = 3;
/// What it exits with once the key is added. Not 0, because `sh` fed nothing
/// by a `base64 -d` that failed also exits 0.
pub const ADDED: i32 = 4;

/// The `.pub` beside a private key, appended rather than swapped in, since
/// `work.ed25519` pairs with `work.ed25519.pub`.
pub fn pub_path(private: &Path) -> PathBuf {
    let mut name = private.as_os_str().to_owned();
    name.push(".pub");
    PathBuf::from(name)
}

/// The command that logs in to `via` and from there to `target`, and appends
/// `pubkey` (one `.pub` line) to `login`'s authorized_keys there. It is run
/// with a terminal, so either hop can ask for a password; it exits
/// `ALREADY_AUTHORIZED` when the key was already listed and `ADDED` once it is
/// added, so a 0 means the script never ran.
///
/// `via` reaches `target` by `target`'s alias when its own config defines it,
/// and by `login`'s HostName and port otherwise, but always as `login.user`,
/// since that user's authorized_keys is the one this machine logs in to.
pub fn install_via_argv(
    via: &str,
    target: &str,
    login: &crate::sshcfg::Login,
    pubkey: &str,
) -> Option<Vec<String>> {
    let blob = pubkey.split_whitespace().nth(1)?;
    let append = format!(
        r#"umask 077
mkdir -p "$HOME/.ssh" || exit 1
f="$HOME/.ssh/authorized_keys"
if [ -f "$f" ] && grep -v '^[[:space:]]*#' "$f" | grep -qF {blob}; then exit {ALREADY_AUTHORIZED}; fi
if [ -s "$f" ] && [ -n "$(tail -c 1 "$f")" ]; then echo >> "$f"; fi
printf '%s\n' {key} >> "$f" && exit {ADDED}
"#,
        blob = crate::completion::quote(blob),
        key = crate::completion::quote(pubkey),
    );
    // `ssh -G` prints the HostName lowercased, and the alias itself when the
    // config has no block for it.
    let hop = format!(
        r#"dest={alias}
case "$(ssh -G "$dest" 2>/dev/null | awk '$1 == "hostname" {{ print $2; exit }}')" in
  {lower} | "") set -- -p {port} {hostname} ;;
  *) set -- "$dest" ;;
esac
ssh -o RemoteCommand=none -o RequestTTY=no -o ClearAllForwardings=yes -l {user} "$@" {remote} </dev/null
"#,
        alias = crate::completion::quote(target),
        lower = crate::completion::quote(&target.to_lowercase()),
        port = crate::completion::quote(&login.port),
        hostname = crate::completion::quote(&login.hostname),
        user = crate::completion::quote(&login.user),
        remote = crate::completion::quote(&crate::paste::sh_line(&append)),
    );
    Some(vec![
        "ssh".into(),
        "-t".into(),
        "-o".into(),
        "RemoteCommand=none".into(),
        "-o".into(),
        "ClearAllForwardings=yes".into(),
        via.into(),
        crate::paste::sh_line(&hop),
    ])
}
