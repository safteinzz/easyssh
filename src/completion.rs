//! Tab completion, served by the binary itself through clap_complete's env
//! mode: `essh completions bash` (or `COMPLETE=bash essh`) prints the script
//! that registers it, and every Tab then runs `essh` with `COMPLETE` set. The first word completes to config aliases and
//! a `cp` path completes locally, to `alias:`, or on the far side over ssh.

use crate::commands::cp::remote_alias;
use crate::sshcfg;
use clap_complete::env::{Bash, Elvish, EnvCompleter, Fish, Powershell, Shells, Zsh};
use clap_complete::{CompletionCandidate, PathCompleter, engine::ValueCompleter};
use std::ffi::{OsStr, OsString};
use std::process::{Command, Stdio};

const SHELLS: Shells<'static> = Shells(&[
    &PastHost(ColonBash),
    &PastHost(Elvish),
    &PastHost(Fish),
    &PastHost(Powershell),
    &PastHost(Zsh),
]);

/// Answers a Tab and exits when `COMPLETE` is set; returns at once otherwise.
pub fn handle(factory: fn() -> clap::Command) {
    clap_complete::CompleteEnv::with_factory(factory)
        .shells(SHELLS)
        .complete();
}

/// The script `COMPLETE=<shell> essh` prints, for `shell` as clap names it.
/// `essh` calls itself back by the name it was run as, made absolute when
/// that name is a relative path, exactly as clap_complete does.
pub fn write_registration(
    cmd: &clap::Command,
    shell: &str,
    buf: &mut dyn std::io::Write,
) -> std::io::Result<()> {
    let completer = SHELLS
        .completer(shell)
        .ok_or_else(|| std::io::Error::other(format!("no completion for `{shell}`")))?;
    let mut me = std::path::PathBuf::from(std::env::args_os().next().unwrap_or("essh".into()));
    if me.components().count() > 1 && me.is_relative() {
        me = std::env::current_dir()?.join(me);
    }
    let bin = cmd.get_bin_name().unwrap_or_else(|| cmd.get_name());
    completer.write_registration("COMPLETE", cmd.get_name(), bin, &me.to_string_lossy(), buf)
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
    // ssh would read `-oProxyCommand=…` as an option and run it.
    if alias.starts_with('-') {
        return Vec::new();
    }
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

/// A shell's completer, silenced once the cursor is past the destination word:
/// what follows `essh raspi` belongs to ssh, while clap's engine never enters
/// an external subcommand and would offer the aliases and subcommands again.
struct PastHost<S>(S);

impl<S: EnvCompleter> EnvCompleter for PastHost<S> {
    fn name(&self) -> &'static str {
        self.0.name()
    }
    fn is(&self, name: &str) -> bool {
        self.0.is(name)
    }
    fn write_registration(
        &self,
        var: &str,
        name: &str,
        bin: &str,
        completer: &str,
        buf: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        self.0.write_registration(var, name, bin, completer, buf)
    }
    fn write_complete(
        &self,
        cmd: &mut clap::Command,
        args: Vec<OsString>,
        current_dir: Option<&std::path::Path>,
        buf: &mut dyn std::io::Write,
    ) -> Result<(), std::io::Error> {
        // Fish and PowerShell pass no index and always complete the last word.
        let index = std::env::var("_CLAP_COMPLETE_INDEX")
            .ok()
            .and_then(|i| i.parse().ok())
            .unwrap_or(args.len().saturating_sub(1));
        if past_destination(cmd, &args, index) {
            return Ok(());
        }
        self.0.write_complete(cmd, args, current_dir, buf)
    }
}

/// Whether a word before `index` is a destination rather than a subcommand.
fn past_destination(cmd: &mut clap::Command, args: &[OsString], index: usize) -> bool {
    cmd.build();
    let first = args
        .iter()
        .take(index)
        .skip(1)
        .find(|w| !w.to_string_lossy().starts_with('-'));
    first.is_some_and(|w| w.to_str().is_none_or(|w| cmd.find_subcommand(w).is_none()))
}

pub(crate) fn quote(s: &str) -> String {
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
        let script = String::from_utf8_lossy(&script)
            .replace(
                r#"words[COMP_CWORD]="$2""#,
                r#"[[ -n "$2" || ${words[COMP_CWORD]} != ":" ]] && words[COMP_CWORD]="$2""#,
            )
            // COMP_WORDS drops the space in `raspi: <Tab>`, so the line up to
            // the cursor is the only thing that tells it from `raspi:<Tab>`.
            .replace(
                r#"        COMPLETE="bash" \"#,
                "        _ESSH_BEFORE_CURSOR=\"${COMP_LINE:0:COMP_POINT}\" \\\n        COMPLETE=\"bash\" \\",
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
        let fresh = std::env::var("_ESSH_BEFORE_CURSOR")
            .is_ok_and(|line| line.ends_with(char::is_whitespace));
        let (args, index, typed) = glue_colons(args, index, fresh);
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
/// before the piece bash will replace. `fresh` says a space sits right before
/// the cursor, so an empty word there is a new one, never part of `raspi:`.
fn glue_colons(words: Vec<OsString>, index: usize, fresh: bool) -> (Vec<OsString>, usize, usize) {
    let mut out: Vec<OsString> = Vec::new();
    let mut new_index = index;
    let mut typed = 0;
    let mut after_colon = false;
    for (i, word) in words.into_iter().enumerate() {
        let is_colon = word == ":";
        let starts_fresh = fresh && i == index && word.is_empty();
        let glue = out.len() > 1 && (is_colon || after_colon) && !starts_fresh;
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

#[cfg(test)]
mod tests {
    use super::*;
    use clap_complete::env::Fish;

    fn words(ws: &[&str]) -> Vec<OsString> {
        ws.iter().map(OsString::from).collect()
    }

    /// What a Tab at the end of `line` offers, as fish asks for it.
    fn offered(line: &[&str]) -> String {
        let mut cmd = clap::Command::new("easyssh")
            .bin_name("essh")
            .allow_external_subcommands(true)
            .subcommand(clap::Command::new("self").subcommand(clap::Command::new("update")));
        let mut out = Vec::new();
        PastHost(Fish)
            .write_complete(&mut cmd, words(line), None, &mut out)
            .expect("completing never fails");
        String::from_utf8(out).expect("completions are text")
    }

    #[test]
    fn nothing_completes_after_the_destination_word() {
        assert_eq!(
            offered(&["essh", "alpha", ""]),
            "",
            "what follows a host belongs to ssh"
        );
        assert_eq!(
            offered(&["essh", "alpha", "-p", ""]),
            "",
            "ssh's own flags too"
        );
        assert!(
            offered(&["essh", "self", ""]).contains("update"),
            "a subcommand still completes its own words"
        );
    }

    #[test]
    fn a_path_bash_split_at_the_colon_is_glued_back_together() {
        let (glued, index, typed) =
            glue_colons(words(&["essh", "cp", "raspi", ":", "~/Do"]), 4, false);
        assert_eq!(
            glued,
            words(&["essh", "cp", "raspi:~/Do"]),
            "`raspi:~/Do` is one word"
        );
        assert_eq!(index, 2, "the cursor is on the glued word");
        assert_eq!(
            typed,
            "raspi:".len(),
            "bash already has `raspi:` on the line"
        );

        let (glued, index, typed) = glue_colons(words(&["essh", "cp", "raspi", ":"]), 3, false);
        assert_eq!(
            glued,
            words(&["essh", "cp", "raspi:"]),
            "`raspi:` is one word"
        );
        assert_eq!(
            (index, typed),
            (2, "raspi:".len()),
            "a bare `:` under the cursor stays"
        );

        let (glued, index, _) = glue_colons(words(&["essh", "cp", "raspi", ":", ""]), 4, true);
        assert_eq!(
            (glued, index),
            (words(&["essh", "cp", "raspi:", ""]), 3),
            "after `raspi: ` the next word is a new one"
        );
    }
}
