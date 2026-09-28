//! sshfs mount tracking - the other "what was the command?" pain. Mounting is
//! `sshfs host: ./dir`; *un*mounting is `fusermount -u ./dir`, which nobody
//! remembers. We read the live mounts from `/proc/mounts` so the TUI can list
//! them and unmount with one key. Linux-first, like the tunnel liveness check.
//!
//! Like a tunnel, every mount you make is kept: a line in
//! `~/.config/easyssh/mounts` that outlives the mount, so it is turned off and
//! on again from the list. A mountpoint holds one mount at a time, so the local
//! folder is what identifies a line.

use anyhow::{Context, Result};
use std::fs;
use std::path::Path;
use std::process::Command;

/// Whether `sshfs` is on PATH. Mounting shells out to it, so we check first and
/// give an install hint instead of a cryptic spawn failure. We do not try to
/// install it: that needs root and per-distro package names.
pub fn sshfs_installed() -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join("sshfs").is_file()))
        .unwrap_or(false)
}

/// One row of the Mounts tab: a mount you keep, mounted or not.
pub struct Mount {
    /// The sshfs source, e.g. `pi@raspi:/home/pi`.
    pub remote: String,
    /// The local mountpoint, absolute.
    pub local: String,
    /// The kernel's mount options, verbatim from `/proc/mounts`, e.g.
    /// `rw,nosuid,nodev,relatime,user_id=1000,group_id=1000`. This is the only
    /// record of how a mount was made once the command that made it is gone.
    /// Empty while it is not mounted.
    pub options: String,
    /// Whether it is mounted right now.
    pub on: bool,
    /// The sftp server run under sudo, for a mount made as root. Only the saved
    /// line knows it, since `/proc/mounts` does not show it.
    pub sudo: Option<String>,
}

/// A mount you kept: `~/sshfs/raspi <- raspi:dotfiles` in the config file, plus
/// ` (sudo /usr/lib/openssh/sftp-server)` for one made as root.
#[derive(Clone)]
pub struct Saved {
    pub remote: String,
    /// Absolute; the file holds it home-relative.
    pub local: String,
    pub sudo: Option<String>,
}

impl Mount {
    /// One-line summary for the TUI: where it lives locally ← what it's mounting.
    /// Both paths are shown home-relative, the way they were typed; `local`
    /// itself stays absolute because that is what `fusermount` is handed.
    pub fn describe(&self) -> String {
        format!(
            "{}  ←  {}:{}",
            crate::sshcfg::collapse_tilde(&self.local),
            self.source(),
            self.home_relative()
        )
    }

    /// The remote directory with the `~` the wizard dropped put back, since sftp
    /// resolves a relative path against the login home.
    fn home_relative(&self) -> String {
        match self.remote.split_once(':').map_or("", |(_, path)| path) {
            "" => "~".into(),
            path if path.starts_with('/') => path.into(),
            path => format!("~/{path}"),
        }
    }

    /// The login half of the source: `pi@raspi` out of `pi@raspi:/home/pi`.
    /// sshfs always writes `[user@]host:path`, so the first `:` is the split.
    pub fn source(&self) -> &str {
        self.remote.split(':').next().unwrap_or(&self.remote)
    }

    /// The host we mounted - the config alias, when that is what was typed.
    pub fn host(&self) -> &str {
        let src = self.source();
        src.rsplit('@').next().unwrap_or(src)
    }

    /// The remote user, when the source names one.
    pub fn user(&self) -> Option<&str> {
        self.source().split_once('@').map(|(u, _)| u)
    }

    /// The remote directory, home-relative. Empty in the source means sshfs
    /// took the login home directory, which is worth saying in those words.
    pub fn remote_path(&self) -> String {
        match self.home_relative().as_str() {
            "~" => "~ (the login home directory)".into(),
            path => path.into(),
        }
    }

    /// Whether a given mount option is set, e.g. `ro` or `allow_other`.
    pub fn has_option(&self, name: &str) -> bool {
        self.options.split(',').any(|o| o == name)
    }

    /// The value of a `key=value` mount option, e.g. `user_id`.
    pub fn option(&self, name: &str) -> Option<&str> {
        self.options
            .split(',')
            .find_map(|o| o.strip_prefix(name)?.strip_prefix('='))
    }

    /// Options that are not `key=value` and not one of the ones we already
    /// spell out, so the panel can show what is left without repeating itself.
    pub fn other_options(&self) -> Vec<&str> {
        self.options
            .split(',')
            .filter(|o| !o.contains('=') && !matches!(*o, "rw" | "ro" | ""))
            .collect()
    }
}

/// Every active `fuse.sshfs` mount, read from `/proc/mounts`.
fn live() -> Vec<Mount> {
    let Ok(text) = fs::read_to_string("/proc/mounts") else {
        return Vec::new();
    };
    text.lines()
        .filter_map(|line| {
            // `/proc/mounts` columns: device mountpoint fstype options dump pass.
            let mut f = line.split_whitespace();
            let dev = f.next()?;
            let mp = f.next()?;
            let fstype = f.next()?;
            let options = f.next().unwrap_or("");
            (fstype == "fuse.sshfs").then(|| Mount {
                remote: unescape(dev),
                local: unescape(mp),
                options: options.to_string(),
                on: true,
                sudo: None,
            })
        })
        .collect()
}

/// The rows of the Mounts tab: every mount you keep, in the order the file has
/// it, each carrying what `/proc/mounts` says about it when it is mounted.
///
/// A mount with no line of its own is written into the file first, so "mounted"
/// and "kept" can never disagree and a row can always be mounted again.
pub fn entries() -> Vec<Mount> {
    let live = live();
    for m in &live {
        if !saved().iter().any(|s| s.local == m.local) {
            let _ = remember(&Saved {
                remote: m.remote.clone(),
                local: m.local.clone(),
                sudo: None,
            });
        }
    }
    saved()
        .into_iter()
        .map(|s| match live.iter().find(|m| m.local == s.local) {
            Some(m) => Mount {
                remote: m.remote.clone(),
                local: s.local,
                options: m.options.clone(),
                on: true,
                sudo: s.sudo,
            },
            None => Mount {
                remote: s.remote,
                local: s.local,
                options: String::new(),
                on: false,
                sudo: s.sudo,
            },
        })
        .collect()
}

/// `~/.config/easyssh/mounts`, beside `tunnels`.
pub fn saved_path() -> std::path::PathBuf {
    dirs::config_dir()
        .unwrap_or_else(|| dirs::home_dir().unwrap_or_default().join(".config"))
        .join("easyssh")
        .join("mounts")
}

/// `local <- host:path`, optionally ` (sudo server)`. A line that is not one of
/// these is skipped rather than refused, since the file is meant to be edited by
/// hand and one bad line must not cost you the others.
fn parse_saved(line: &str) -> Option<Saved> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }
    let (local, rest) = line.split_once(" <- ")?;
    let (remote, sudo) = match rest.split_once(" (sudo ") {
        Some((remote, server)) => (remote, Some(server.strip_suffix(')')?.trim().to_string())),
        None => (rest, None),
    };
    let remote = remote.trim();
    if !remote.contains(':') {
        return None;
    }
    Some(Saved {
        remote: remote.to_string(),
        local: crate::sshcfg::expand_tilde(local.trim())
            .to_string_lossy()
            .into_owned(),
        sudo,
    })
}

fn saved_line(s: &Saved) -> String {
    let line = format!(
        "{} <- {}",
        crate::sshcfg::collapse_tilde(&s.local),
        s.remote
    );
    match &s.sudo {
        Some(server) => format!("{line} (sudo {server})"),
        None => line,
    }
}

/// Every mount you keep, in the order the file has them.
pub fn saved() -> Vec<Saved> {
    saved_from(&fs::read_to_string(saved_path()).unwrap_or_default())
}

fn saved_from(text: &str) -> Vec<Saved> {
    text.lines().filter_map(parse_saved).collect()
}

const SAVED_HEADER: &str = "\
# easyssh mounts: the sshfs mounts you keep, one `local <- host:path` line
# each, and ` (sudo /path/to/sftp-server)` after one made as root. The Mounts
# tab writes this file; `enter` mounts and unmounts a line.

";

/// Add a kept mount, or replace the line for the same mountpoint. Every other
/// line survives untouched, comments included.
pub fn remember(s: &Saved) -> Result<()> {
    let path = saved_path();
    let text = fs::read_to_string(&path).unwrap_or_default();
    write_saved(&path, &with_saved(&text, s))
}

/// Drop the line for this mountpoint. Whether it is mounted is the caller's
/// business: the file only says what you keep.
pub fn forget(local: &str) -> Result<()> {
    let path = saved_path();
    let text = fs::read_to_string(&path).unwrap_or_default();
    write_saved(&path, &without_saved(&text, local))
}

fn with_saved(text: &str, s: &Saved) -> String {
    let mut body = if text.trim().is_empty() {
        SAVED_HEADER.to_string()
    } else {
        String::new()
    };
    let mut replaced = false;
    for line in text.lines() {
        match parse_saved(line) {
            Some(old) if old.local == s.local => {
                if !replaced {
                    body.push_str(&saved_line(s));
                    body.push('\n');
                    replaced = true;
                }
            }
            _ => {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    if !replaced {
        body.push_str(&saved_line(s));
        body.push('\n');
    }
    body
}

fn without_saved(text: &str, local: &str) -> String {
    text.lines()
        .filter(|l| !matches!(parse_saved(l), Some(s) if s.local == local))
        .map(|l| format!("{l}\n"))
        .collect()
}

fn write_saved(path: &Path, body: &str) -> Result<()> {
    if let Some(dir) = path.parent() {
        fs::create_dir_all(dir).with_context(|| format!("creating {}", dir.display()))?;
    }
    fs::write(path, body).with_context(|| format!("writing {}", path.display()))
}

/// Unmount an sshfs mountpoint. Prefer `fusermount -u` (needs no root); fall
/// back to `umount` if fusermount is missing or refuses.
pub fn unmount(local: &str) -> Result<()> {
    match Command::new("fusermount").args(["-u", local]).output() {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => try_umount(local).map_err(|_| anyhow_from_stderr(&o.stderr)),
        Err(_) => try_umount(local).context("fusermount not found and umount failed"),
    }
}

/// Lazy unmount: detach the mount now and let the kernel free it once nothing
/// is using it. This is the escape hatch when a normal unmount reports the mount
/// busy because a shell or program still has it open.
pub fn unmount_lazy(local: &str) -> Result<()> {
    match Command::new("fusermount")
        .args(["-u", "-z", local])
        .output()
    {
        Ok(o) if o.status.success() => Ok(()),
        Ok(o) => try_umount_lazy(local).map_err(|_| anyhow_from_stderr(&o.stderr)),
        Err(_) => try_umount_lazy(local).context("fusermount not found and umount -l failed"),
    }
}

fn try_umount_lazy(local: &str) -> Result<()> {
    let o = Command::new("umount")
        .args(["-l", local])
        .output()
        .context("running umount -l")?;
    if o.status.success() {
        Ok(())
    } else {
        Err(anyhow_from_stderr(&o.stderr))
    }
}

fn try_umount(local: &str) -> Result<()> {
    let o = Command::new("umount")
        .arg(local)
        .output()
        .context("running umount")?;
    if o.status.success() {
        Ok(())
    } else {
        Err(anyhow_from_stderr(&o.stderr))
    }
}

fn anyhow_from_stderr(stderr: &[u8]) -> anyhow::Error {
    let msg = String::from_utf8_lossy(stderr).trim().to_string();
    if msg.is_empty() {
        anyhow::anyhow!("unmount failed")
    } else {
        anyhow::anyhow!("{msg}")
    }
}

/// `/proc/mounts` octal-escapes space (`\040`), tab, newline and backslash.
/// Undo the common ones so paths display correctly.
fn unescape(s: &str) -> String {
    s.replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_saved_mounts_file_survives_a_round_trip_and_a_hand_edit() {
        let file = "# mine\n~/sshfs/raspi <- raspi:dotfiles\n/mnt/nas <- admin@nas:/srv (sudo /usr/lib/openssh/sftp-server)\n";
        let saved = saved_from(file);
        assert_eq!(saved.len(), 2, "both lines should parse");
        assert_eq!(saved[0].remote, "raspi:dotfiles");
        assert!(
            saved[0].local.starts_with('/'),
            "a `~` mountpoint must come back absolute, since fusermount gets it"
        );
        assert_eq!(
            saved[1].sudo.as_deref(),
            Some("/usr/lib/openssh/sftp-server")
        );

        // Rewriting a line keeps every other one, comment included.
        let moved = Saved {
            remote: "raspi:photos".into(),
            ..saved[0].clone()
        };
        assert_eq!(
            with_saved(file, &moved),
            "# mine\n~/sshfs/raspi <- raspi:photos\n/mnt/nas <- admin@nas:/srv (sudo /usr/lib/openssh/sftp-server)\n",
            "the line is keyed by its mountpoint and replaced where it stands"
        );
        assert_eq!(
            without_saved(file, &saved[1].local),
            "# mine\n~/sshfs/raspi <- raspi:dotfiles\n"
        );

        // A line we cannot read is skipped rather than refused.
        assert!(saved_from("nonsense\n/mnt/x <- no-colon\n<- raspi:\n").is_empty());
    }
}
