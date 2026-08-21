//! Turning the mount wizard's fields into the `sshfs` command that runs, sudo
//! mode included.

use super::*;

/// The local mountpoint a mount wizard will use: `./sshfs/<host>` when the field
/// is blank (so leftovers stay grouped), with a typed `~` expanded because sshfs
/// is spawned without a shell and would otherwise mount onto a directory named
/// `~`. Shared by `submit_prompt` and `command_preview` so the preview cannot
/// drift from the command that actually runs.
pub(super) fn mount_point(host: &str, typed: &str) -> String {
    let raw = if typed.is_empty() {
        format!("./sshfs/{host}")
    } else {
        typed.to_string()
    };
    sshcfg::expand_tilde(&raw).to_string_lossy().into_owned()
}

/// Where OpenSSH's sftp server usually lives. Distros move it (`/usr/libexec/
/// openssh/`, `/usr/lib/ssh/`), and nothing local can see the remote layout, so
/// this is an editable default rather than a guess we hide.
pub(super) const SFTP_SERVER: &str = "/usr/lib/openssh/sftp-server";

/// Who the remote side runs the sftp server as. Mirrors the "Remote rights"
/// choice field, in the same order.
///
/// There is deliberately no "send the password" mode: sshfs's channel has no
/// terminal for sudo to prompt on, and every way of feeding one from here puts
/// the password in a command line, where `ps` shows it to every user on both
/// machines. A NOPASSWD sudoers line scoped to the sftp server is the safe
/// equivalent.
#[derive(Clone, Copy, PartialEq)]
pub(crate) enum Sudo {
    /// Plain sshfs: the login user's own rights.
    No,
    /// `sudo sftp-server`, which needs a NOPASSWD sudoers line on the server.
    NoPasswd,
}

impl Sudo {
    pub(super) fn from_choice(i: usize) -> Self {
        if i == 1 { Sudo::NoPasswd } else { Sudo::No }
    }
}

/// A mount resolved from the wizard's fields. The run path and the live preview
/// both build their command from this, so the preview cannot drift from what
/// actually runs.
pub(crate) struct MountSpec {
    pub(crate) host: String,
    pub(crate) remote: String,
    pub(crate) local: String,
    pub(crate) sudo: Sudo,
    pub(crate) server: String,
}

impl MountSpec {
    pub(super) fn from_fields(host: &str, fields: &[Field]) -> Self {
        let v = |i: usize| fields[i].value.trim();
        let server = if v(3).is_empty() {
            SFTP_SERVER.to_string()
        } else {
            v(3).to_string()
        };
        Self {
            host: host.to_string(),
            remote: v(0).to_string(),
            local: mount_point(host, v(1)),
            sudo: Sudo::from_choice(fields[2].choice),
            server,
        }
    }

    /// The `-o sftp_server=…` value, or `None` for a plain mount.
    pub(super) fn sftp_server(&self) -> Option<String> {
        match self.sudo {
            Sudo::No => None,
            Sudo::NoPasswd => Some(format!("sftp_server=sudo {}", self.server)),
        }
    }

    pub(super) fn argv(&self) -> Vec<String> {
        let mut argv = vec!["sshfs".to_string()];
        if let Some(opt) = self.sftp_server() {
            argv.push("-o".into());
            argv.push(opt);
        }
        argv.push(format!("{}:{}", self.host, self.remote));
        argv.push(self.local.clone());
        argv
    }

    /// Why this mount cannot be built, if so - checked before spawning so the
    /// user gets a sentence instead of a puzzling sshfs failure.
    pub(super) fn problem(&self) -> Option<String> {
        // sshfs splits `-o` values on commas, so one inside the option is read as
        // the start of another option and the mount fails with nothing useful.
        if self.sudo != Sudo::No && self.server.contains(',') {
            return Some("mount: sshfs splits -o on commas, so this path cannot be sent".into());
        }
        None
    }
}

/// Render an argv the way you would type it, quoting the arguments that contain
/// spaces. Only used for the preview line, never to run anything.
pub(super) fn shell_join(argv: &[String]) -> String {
    argv.iter()
        .map(|a| {
            if a.contains(' ') {
                format!("\"{a}\"")
            } else {
                a.clone()
            }
        })
        .collect::<Vec<_>>()
        .join(" ")
}
