//! sshfs mount tracking - the other "what was the command?" pain. Mounting is
//! `sshfs host: ./dir`; *un*mounting is `fusermount -u ./dir`, which nobody
//! remembers. We read the live mounts from `/proc/mounts` so the TUI can list
//! them and unmount with one key. Linux-first, like the tunnel liveness check.

use anyhow::{Context, Result};
use std::fs;
use std::process::Command;

/// Whether `sshfs` is on PATH. Mounting shells out to it, so we check first and
/// give an install hint instead of a cryptic spawn failure. We do not try to
/// install it: that needs root and per-distro package names.
pub fn sshfs_installed() -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join("sshfs").is_file()))
        .unwrap_or(false)
}

/// One active sshfs mount.
pub struct Mount {
    /// The sshfs source, e.g. `pi@raspi:/home/pi`.
    pub remote: String,
    /// The local mountpoint.
    pub local: String,
}

impl Mount {
    /// One-line summary for the TUI: where it lives locally ← what it's mounting.
    pub fn describe(&self) -> String {
        format!("{}  ←  {}", self.local, self.remote)
    }
}

/// Every active `fuse.sshfs` mount, read from `/proc/mounts`.
pub fn list() -> Vec<Mount> {
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
            (fstype == "fuse.sshfs").then(|| Mount {
                remote: unescape(dev),
                local: unescape(mp),
            })
        })
        .collect()
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
