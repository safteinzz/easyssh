//! `essh cp <src…> <dst>` - copy files without the scp ceremony. Instead of
//! `scp -r ./thing user@host:/path` you write `essh cp ./thing raspi:Documents`:
//! the config alias is enough, and we add `-r` automatically when a source is a
//! directory (the flag everyone forgets until it errors).

use crate::sshcfg;
use colored::Colorize;
use std::path::Path;

#[derive(clap::Args)]
pub struct Args {
    /// One or more sources followed by the destination. A remote side is
    /// `host:path` (host = a config alias), e.g. `essh cp notes.md raspi:~`.
    #[arg(required = true, num_args = 2..)]
    pub paths: Vec<String>,
}

pub fn run(args: Args) {
    // clap guarantees ≥2, so split is safe: everything but the last is a source.
    let (dst, srcs) = args.paths.split_last().unwrap();

    // Warn early if the remote alias isn't in the config - a typo here otherwise
    // shows up as a confusing scp "Could not resolve hostname".
    let known: Vec<String> = sshcfg::list_hosts().into_iter().map(|h| h.alias).collect();
    for token in &args.paths {
        if let Some(alias) = remote_alias(token)
            && !known.contains(&alias)
        {
            eprintln!(
                "{}",
                format!("essh: '{alias}' isn't in your ~/.ssh/config - passing it to scp as-is.").yellow()
            );
        }
    }

    // Auto-recursive if any *local* source is a directory.
    let recursive = srcs.iter().any(|s| remote_alias(s).is_none() && Path::new(s).is_dir());

    let mut cmd = std::process::Command::new("scp");
    if recursive {
        cmd.arg("-r");
    }
    cmd.args(srcs).arg(dst);

    match cmd.status() {
        Ok(status) if status.success() => {}
        Ok(status) => std::process::exit(status.code().unwrap_or(1)),
        Err(e) => {
            eprintln!("{}", format!("essh: could not run scp: {e}").red());
            std::process::exit(127);
        }
    }
}

/// If `token` is a remote spec `host:path`, return the host part. We only treat
/// the piece before the first `:` as a host when it looks like an alias - a
/// Windows path (`C:\…`) or absolute path isn't a remote.
fn remote_alias(token: &str) -> Option<String> {
    let (head, _tail) = token.split_once(':')?;
    if head.is_empty() || head.contains('/') || head.contains('\\') || head.len() == 1 {
        return None; // paths, not host aliases
    }
    Some(head.to_string())
}
