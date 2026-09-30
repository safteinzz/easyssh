//! Tab completion, served by the binary itself through clap_complete's env
//! mode: `source <(COMPLETE=bash essh)` registers it, and every Tab then runs
//! `essh` with `COMPLETE` set. The first word completes to config aliases and
//! a `cp` path completes locally, to `alias:`, or on the far side over ssh.

use crate::commands::cp::remote_alias;
use crate::sshcfg;
use clap_complete::env::{Bash, Elvish, EnvCompleter, Fish, Powershell, Shells, Zsh};
use clap_complete::{CompletionCandidate, PathCompleter, engine::ValueCompleter};
use std::ffi::{OsStr, OsString};
use std::process::{Command, Stdio};

/// Answers a Tab and exits when `COMPLETE` is set; returns at once otherwise.
pub fn handle(factory: fn() -> clap::Command) {
    clap_complete::CompleteEnv::with_factory(factory)
        .shells(Shells(&[&ColonBash, &Elvish, &Fish, &Powershell, &Zsh]))
        .complete();
}

/// Every alias in the config, for the destination word.
pub fn hosts() -> Vec<CompletionCandidate> {
    sshcfg::list_hosts()
        .into_iter()
        .map(|h| CompletionCandidate::new(&h.alias).help(Some(h.target().into())))
        .collect()
}

/// A `cp` path: `alias:path` lists the remote side, anything else is a local
/// path or the start of an alias.
pub fn cp_path(current: &OsStr) -> Vec<CompletionCandidate> {
    let Some(current) = current.to_str() else {
        return Vec::new();
    };
    if let Some(alias) = remote_alias(current) {
        let path = &current[alias.len() + 1..];
        return remote_paths(&alias, path)
            .into_iter()
            .map(|p| CompletionCandidate::new(format!("{alias}:{p}")))
            .collect();
    }
    let mut out = PathCompleter::any().complete(OsStr::new(current));
    out.extend(
        hosts()
            .into_iter()
            .filter(|c| c.get_value().to_string_lossy().starts_with(current))
            .map(|c| {
                let value = format!("{}:", c.get_value().to_string_lossy());
                CompletionCandidate::new(value).help(c.get_help().cloned())
            }),
    );
    out
}

/// What `path*` matches on `alias`, spelled the way it was typed (`~/Do` gives
/// `~/Documents/`). Empty when the host cannot answer without a prompt.
fn remote_paths(alias: &str, path: &str) -> Vec<String> {
    let (dir, base) = match path.rfind('/') {
        Some(i) => path.split_at(i + 1),
        None => ("", path),
    };
    let list = format!("command ls -1dLp -- {}* 2>/dev/null", quote(base));
    let script = match remote_dir(dir) {
        Some(dir) => format!("cd {dir} 2>/dev/null && {list}"),
        None if dir.is_empty() => list,
        None => return Vec::new(),
    };
    let Ok(out) = Command::new("ssh")
        .args(["-o", "BatchMode=yes", "-o", "ConnectTimeout=5"])
        .args(["-o", "RemoteCommand=none", "-o", "RequestTTY=no"])
        .args([alias, "--", &script])
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
    else {
        return Vec::new();
    };
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(|name| format!("{dir}{name}"))
        .collect()
}

/// `dir` quoted for the remote shell, a leading `~` or `~user` left bare so
/// that shell expands it. `None` for an empty dir or an unsafe `~user`.
fn remote_dir(dir: &str) -> Option<String> {
    if dir.is_empty() {
        return None;
    }
    if !dir.starts_with('~') {
        return Some(quote(dir));
    }
    let end = dir.find('/').unwrap_or(dir.len());
    let (tilde, rest) = dir.split_at(end);
    let user_ok = tilde[1..]
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || "._-".contains(c));
    // The `/` stays bare too, or bash does not expand the `~` in front of it.
    let rest = rest.strip_prefix('/').map(|r| format!("/{}", quote(r)));
    user_ok.then(|| format!("{tilde}{}", rest.unwrap_or_default()))
}

fn quote(s: &str) -> String {
    format!("'{}'", s.replace('\'', r"'\''"))
}

/// clap's bash adapter, told about the colon: bash's `COMP_WORDBREAKS` splits
/// `raspi:~/Do` into `raspi`, `:` and `~/Do`, and replaces only the last piece
/// with what we answer. So the pieces are glued back before clap parses them,
/// and each answer loses the part bash already has on the line.
struct ColonBash;

impl EnvCompleter for ColonBash {
    fn name(&self) -> &'static str {
        Bash.name()
    }
    fn is(&self, name: &str) -> bool {
        Bash.is(name)
    }
    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        let mut script = Vec::new();
        Bash.write_registration(var, name, bin, completer, &mut script)?;
        // With the cursor just past `raspi:`, bash's word is `:` but its `$2` is
        // empty, and clap's copy of `$2` over the word would lose the colon.
        let script = String::from_utf8_lossy(&script).replace(
            r#"words[COMP_CWORD]="$2""#,
            r#"[[ -n "$2" || ${words[COMP_CWORD]} != ":" ]] && words[COMP_CWORD]="$2""#,
        );
        buf.write_all(script.as_bytes())
    }
    fn write_complete(
        &self,
        cmd: &mut clap::Command,
        args: Vec<OsString>,
        current_dir: Option<&std::path::Path>,
        buf: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        let index: usize = std::env::var("_CLAP_COMPLETE_INDEX")
            .ok()
            .and_then(|i| i.parse().ok())
            .unwrap_or_default();
        let ifs = std::env::var("_CLAP_IFS").unwrap_or_else(|_| "\n".into());
        let (args, index, typed) = glue_colons(args, index);
        let completions = clap_complete::engine::complete(cmd, args, index, current_dir)?;
        let answers: Vec<String> = completions
            .iter()
            .filter_map(|c| {
                let value = c.get_value().to_string_lossy();
                value.get(typed..).map(str::to_string)
            })
            .collect();
        write!(buf, "{}", answers.join(&ifs))
    }
}

/// Joins the words bash split at a `:` back together. Returns the words, the
/// new index of the one under the cursor, and how many bytes of that word sit
/// before the piece bash will replace.
fn glue_colons(words: Vec<OsString>, index: usize) -> (Vec<OsString>, usize, usize) {
    let mut out: Vec<OsString> = Vec::new();
    let mut new_index = index;
    let mut typed = 0;
    let mut after_colon = false;
    for (i, word) in words.into_iter().enumerate() {
        let is_colon = word == ":";
        let glue = out.len() > 1 && (is_colon || after_colon);
        if glue {
            let last = out.last_mut().expect("out holds at least two words");
            if i == index && !is_colon {
                typed = last.len();
            }
            last.push(&word);
            // A bare `:` under the cursor is kept on the line, so bash inserts after it.
            if i == index && is_colon {
                typed = last.len();
            }
        } else {
            out.push(word);
        }
        if i == index {
            new_index = out.len() - 1;
        }
        after_colon = is_colon && glue;
    }
    (out, new_index, typed)
}
