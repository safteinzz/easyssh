//! `P` on a host: turning image paste into Claude Code on and off there, and
//! reading what the remote script said when it is done.

use super::*;
use confirm::ConfirmAction;

impl App {
    /// Both ways go behind a gate: on lets the host read your clipboard, and off
    /// deletes the stand-in there, which only that gate brings back.
    pub(super) fn toggle_paste(&mut self, alias: String) -> Option<PendingRun> {
        if !self.paste.contains(&alias) {
            self.confirm = Some(Confirm::new(
                "image paste",
                format!(
                    "Let {alias} read the image on your clipboard whenever you are connected with essh? A stand-in xclip goes into ~/.local/bin there, so Ctrl+V in Claude Code on {alias} pastes the image from this machine through the login (ssh -R). Root on {alias} can read it too while you are connected."
                ),
                ConfirmAction::InstallPaste(alias),
            ));
            return None;
        }
        self.confirm = Some(Confirm::new(
            "image paste",
            format!(
                "Turn off image paste for {alias}? Its stand-in ~/.local/bin/xclip is removed there, and logins stop carrying your clipboard."
            ),
            ConfirmAction::RemovePaste(alias),
        ));
        None
    }

    /// The line goes first, because that alone stops the forwarding.
    pub(super) fn remove_paste(&mut self, alias: String) -> Option<PendingRun> {
        if let Err(e) = paste::forget(&alias) {
            self.set_failed(format!("image paste is still on for {alias}: {e:#}"));
            return None;
        }
        self.paste.retain(|a| *a != alias);
        Some(PendingRun {
            argv: paste::remove_argv(&alias),
            label: format!("removing the xclip stand-in from {alias} (ssh {alias})"),
            paste: Some(PasteJob::Remove(alias)),
            ..Default::default()
        })
    }

    pub(super) fn install_paste(&mut self, alias: String) -> PendingRun {
        PendingRun {
            argv: paste::install_argv(&alias),
            label: format!("installing the xclip stand-in on {alias} (ssh {alias})"),
            paste: Some(PasteJob::Install(alias)),
            ..Default::default()
        }
    }

    /// Report a finished install or removal. `out` is `None` when ssh could not
    /// be started at all.
    pub(super) fn finish_paste(&mut self, job: PasteJob, out: Option<&Output>) {
        let Some(out) = out else {
            self.alert(
                "could not run it",
                "`ssh` could not be started, so nothing changed on the host.\n\nCheck that it is installed and on your PATH.",
            );
            return;
        };
        let stdout = String::from_utf8_lossy(&out.stdout);
        let stderr = String::from_utf8_lossy(&out.stderr);
        match job {
            PasteJob::Install(alias) => self.finish_install(&alias, out, &stdout, &stderr),
            PasteJob::Remove(alias) => {
                let off = format!("image paste off for {alias}");
                match paste::read_remove(&stdout) {
                    Some(paste::Removal::Done) => {
                        self.set_status(format!("{off}: removed ~/.local/bin/xclip there"))
                    }
                    Some(paste::Removal::Absent) => {
                        self.set_status(format!("{off}: there was no stand-in there to remove"))
                    }
                    Some(paste::Removal::NotOurs) => self.set_status(format!(
                        "{off}: ~/.local/bin/xclip there is not essh's, so it stays"
                    )),
                    // Nothing forwards to it any more, so a stand-in left there
                    // only hands every call to the real xclip.
                    None => self.report_failure(
                        "stand-in not removed",
                        &format!("{off}, but its stand-in is still there"),
                        &format!("ssh {alias}"),
                        &connect::said(&stderr),
                    ),
                }
            }
        }
    }

    fn finish_install(&mut self, alias: &str, out: &Output, stdout: &str, stderr: &str) {
        let cmd = format!("ssh {alias}");
        if !out.status.success() && !stdout.contains("essh-paste: ") {
            self.report_failure(
                "image paste failed",
                &format!("could not install on {alias}"),
                &cmd,
                &connect::said(stderr),
            );
            return;
        }
        let installed = match paste::read_install(alias, stdout) {
            Ok(installed) if out.status.success() => installed,
            Ok(_) => {
                self.alert(
                    "image paste failed",
                    format!(
                        "{cmd}\n\nThe stand-in could not be written to ~/.local/bin/xclip there.\n\n{}",
                        connect::said(stderr)
                    ),
                );
                return;
            }
            Err(why) => {
                self.alert("image paste failed", why);
                return;
            }
        };
        if let Err(e) = paste::remember(alias, &installed.dir) {
            self.alert(
                "image paste failed",
                format!(
                    "The stand-in is on {alias}, but {e:#}, so essh will not forward your clipboard there."
                ),
            );
            return;
        }
        self.paste.push(alias.to_string());
        if installed.warnings.is_empty() {
            self.set_status(format!(
                "image paste on for {alias}: connect, then Ctrl+V in Claude Code there (xclip over ssh -R)"
            ));
        } else {
            self.alert(
                "image paste on, with a catch",
                installed.warnings.join("\n\n"),
            );
        }
    }
}
