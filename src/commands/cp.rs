//! `essh cp <src…> <dst>` - copy files without the scp ceremony. Instead of
//! `scp -r ./thing user@host:/path` you write `essh cp ./thing raspi:Documents`:
//! the config alias is enough, and a folder copies without `-r`. rsync does the
//! copy when it is on both machines, with one progress line for the whole copy,
//! and scp does it everywhere else.

use crate::sshcfg;
use colored::Colorize;
use std::io::IsTerminal;
use std::path::Path;
use std::process::{Command, Stdio};

/// What scp hands its own ssh (`scp.c`), so a host that forces a command or
/// forwards ports, the agent or X11 copies the same way under rsync.
const SSH_OPTS: &[&str] = &[
    "-x",
    "-oPermitLocalCommand=no",
    "-oClearAllForwardings=yes",
    "-oRemoteCommand=none",
    "-oRequestTTY=no",
    "-oControlMaster=no",
    "-oForwardAgent=no",
];

#[derive(clap::Args)]
pub struct Args {
    /// One or more sources followed by the destination. A remote side is
    /// `host:path` (host = a config alias), e.g. `essh cp notes.md raspi:~`.
    #[arg(required = true, num_args = 2.., value_name = "PATH",
          add = clap_complete::ArgValueCompleter::new(crate::completion::cp_path))]
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
                format!("essh: `{alias}` isn't in your ~/.ssh/config, so ssh gets it as it is")
                    .yellow()
            );
        }
    }

    let mut cmd = if rsync_agrees(srcs, dst) {
        rsync(srcs, dst)
    } else {
        scp(srcs, dst)
    };
    let tool = cmd.get_program().to_string_lossy().into_owned();

    match cmd.status() {
        Ok(status) if status.success() => {}
        Ok(_) => std::process::exit(1),
        Err(_) => {
            eprintln!(
                "{}",
                format!("essh: could not run `{tool}`: is it installed and on PATH?").red()
            );
            std::process::exit(1);
        }
    }
}

/// Whether rsync can do this copy and land everything where scp would: rsync
/// on both machines, every source on one side and the destination on the
/// other with one remote host between them (rsync copies neither between two
/// remotes nor within one, and takes no mix of local and remote sources), and
/// a destination that is already a folder, since into a new name rsync nests
/// a folder one level deeper than scp does. Asked without a prompt, so a host
/// that wants a password gets scp and is asked once.
fn rsync_agrees(srcs: &[String], dst: &str) -> bool {
    let src_hosts: Vec<Option<String>> = srcs.iter().map(|s| remote_alias(s)).collect();
    let one_side = match remote_alias(dst) {
        Some(_) => src_hosts.iter().all(Option::is_none),
        None => src_hosts[0].is_some() && src_hosts.iter().all(|h| *h == src_hosts[0]),
    };
    if !one_side || !local_rsync() {
        return false;
    }
    // rsync reads a bare `~` as a file named `~`, where scp means the home folder.
    let bare_home = srcs
        .iter()
        .map(|s| as_a_folder(s))
        .chain([dst])
        .any(|p| p.split_once(':').is_some_and(|(_, path)| path == "~"));
    if bare_home {
        return false;
    }
    if remote_alias(dst).is_some() {
        remote_kind(dst) == Some('d')
    } else {
        Path::new(dst).is_dir() && remote_kind(as_a_folder(&srcs[0])).is_some()
    }
}

fn local_rsync() -> bool {
    Command::new("rsync")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

/// The type letter `rsync --list-only` shows for `host:path` (`d` for a folder),
/// read the same way the copy will read the path, or `None` when rsync cannot
/// reach it there.
fn remote_kind(path: &str) -> Option<char> {
    let out = Command::new("rsync")
        .arg("--list-only")
        .arg(format!(
            "--rsh=ssh -oBatchMode=yes -oConnectTimeout=5 {}",
            SSH_OPTS.join(" ")
        ))
        .arg(path)
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    out.status.success().then_some(())?;
    String::from_utf8_lossy(&out.stdout).chars().next()
}

fn rsync(srcs: &[String], dst: &str) -> Command {
    let mut cmd = Command::new("rsync");
    // `-L` copies what a symlink points to, as scp does, and nothing past `-r`
    // is kept, since scp keeps no owner or times either.
    cmd.arg("-rL")
        .arg(format!("--rsh=ssh {}", SSH_OPTS.join(" ")));
    if std::io::stdout().is_terminal() {
        // Without `--no-inc-recursive` the percentage restarts as rsync finds more files.
        cmd.args(["--info=progress2", "--human-readable", "--no-inc-recursive"]);
    }
    cmd.args(srcs.iter().map(|s| as_a_folder(s))).arg(dst);
    cmd
}

/// scp copies `dir/` as the folder itself, while rsync would copy only what
/// is inside it, so the slash goes.
fn as_a_folder(path: &str) -> &str {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() || trimmed.ends_with(':') {
        path
    } else {
        trimmed
    }
}

fn scp(srcs: &[String], dst: &str) -> Command {
    let mut cmd = Command::new("scp");
    // A remote source cannot be checked from here, and `scp -r` copies a lone file
    // exactly as plain `scp` does.
    if srcs
        .iter()
        .any(|s| remote_alias(s).is_some() || Path::new(s).is_dir())
    {
        cmd.arg("-r");
    }
    cmd.args(srcs).arg(dst);
    cmd
}

/// If `token` is a remote spec `host:path`, return the host part. We only treat
/// the piece before the first `:` as a host when it looks like an alias - a
/// Windows path (`C:\…`) or absolute path isn't a remote.
pub fn remote_alias(token: &str) -> Option<String> {
    let (head, _tail) = token.split_once(':')?;
    if head.is_empty() || head.contains('/') || head.contains('\\') || head.len() == 1 {
        return None; // paths, not host aliases
    }
    Some(head.to_string())
}
