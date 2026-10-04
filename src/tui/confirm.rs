//! Yes/No gates in front of anything destructive, and the offered fixes that
//! follow a failure.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::*;

/// A titled yes/no modal, in two kinds that must not look alike: a **gate** in
/// front of something you cannot undo, and an **offer** the app volunteers
/// after something already failed.
pub(crate) struct Confirm {
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) action: ConfirmAction,
    /// A gate rather than an offer, which decides the colour: red when
    /// something is about to be lost, cyan when the app is proposing a fix.
    pub(crate) danger: bool,
    /// Which button is selected.
    pub(crate) yes: bool,
}

impl Confirm {
    /// A gate. Red, and it starts on **No**: it stands in front of something
    /// destructive or forceful, so a reflex Enter must never be what fires it.
    pub(super) fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        action: ConfirmAction,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            action,
            danger: true,
            yes: false,
        }
    }

    /// An offer. Cyan, and it starts on **Yes**: the client already failed and
    /// this is the app proposing the next step, which you asked for by pressing
    /// Enter in the first place. Nothing is lost by accepting it.
    pub(super) fn offer(
        title: impl Into<String>,
        message: impl Into<String>,
        action: ConfirmAction,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            action,
            danger: false,
            yes: true,
        }
    }
}

pub(crate) enum ConfirmAction {
    DeleteHost(String),
    /// Force `fusermount -u -z` on a mountpoint a normal unmount found busy.
    LazyUnmount {
        local: String,
        /// Delete its line once it is unmounted, because a `D` is what hit the busy mount.
        forget: bool,
    },
    /// Drop a mount's line from `~/.config/easyssh/mounts`, unmounting it first.
    DeleteMount {
        local: String,
        /// Mounted right now, so it is unmounted before the line goes.
        on: bool,
    },
    /// Run `ssh-keygen -R <target>` to drop a host key that no longer matches.
    ClearKnownHost {
        target: String,
    },
    /// Write `IdentitiesOnly yes` into a host's block, so ssh offers only its
    /// IdentityFile instead of every key in the agent.
    IdentitiesOnly {
        alias: String,
    },
    /// Pick a host that can already log in to `target`, to add `key` there through it.
    KeyVia {
        target: String,
        key: PathBuf,
    },
    /// Install the image-paste stand-in on a host and add it to
    /// `~/.config/easyssh/paste`.
    InstallPaste(String),
    /// Remove the image-paste stand-in from a host and drop its line from
    /// `~/.config/easyssh/paste`.
    RemovePaste(String),
    /// Drop a forward's line from `~/.config/easyssh/tunnels`, stopping it first.
    DeleteTunnel {
        kind: char,
        spec: String,
        host: String,
        label: String,
        /// The process carrying it, stopped before the line goes.
        pid: Option<u32>,
    },
}

pub(super) fn render_confirm(f: &mut Frame, area: Rect, c: &Confirm) {
    // A gate is red and starts on No; an offer is cyan and starts on Yes. The
    // colour says how much is at stake, the default focus says what Enter does.
    let accent = if c.danger { Color::Red } else { Color::Cyan };
    let width = box_width(area.width);
    let msg_rows = wrapped_line_count(&c.message, box_inner_width(width)) as u16;
    // The message, a blank, the buttons, a blank, the keys.
    let rect = box_area(area, width, box_height(msg_rows + 4, area.height));
    f.render_widget(Clear, rect);

    let mut lines: Vec<Line> = c.message.lines().map(Line::raw).collect();
    lines.extend([
        Line::raw(""),
        box_buttons(accent, c.yes),
        Line::raw(""),
        box_hint("h/l ←/→ move · enter select · y/n"),
    ]);
    let para = Paragraph::new(lines)
        .block(box_block(accent, &c.title))
        .wrap(Wrap { trim: false });
    f.render_widget(para, rect);
}

impl App {
    /// Resolve a pending yes/no gate. `y` proceeds and `n`/`Esc` cancels outright,
    /// or move between the buttons (`h`/`l`, the arrows, Tab) and press Enter.
    /// Any other key is ignored so a stray keypress cannot dismiss the modal.
    pub(super) fn confirm_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        use KeyCode::*;
        match key.code {
            Left | Right | Char('h') | Char('l') | Tab | BackTab => {
                if let Some(c) = self.confirm.as_mut() {
                    c.yes = !c.yes;
                }
                return None;
            }
            Char('n') | Char('N') | Esc => {
                self.confirm = None;
                self.set_status("cancelled");
                return None;
            }
            Char('y') | Char('Y') => {}
            Enter => {
                if !self.confirm.as_ref().is_some_and(|c| c.yes) {
                    self.confirm = None;
                    self.set_status("cancelled");
                    return None;
                }
            }
            _ => return None,
        }
        let c = self.confirm.take()?;
        match c.action {
            ConfirmAction::DeleteHost(alias) => match sshcfg::delete_host(&alias) {
                Ok(_) => {
                    self.refresh_hosts();
                    self.set_status(format!("deleted '{alias}' (config backed up)"));
                }
                Err(e) => self.set_failed(format!("delete failed: {e}")),
            },
            ConfirmAction::LazyUnmount { local, forget } => match mounts::unmount_lazy(&local) {
                Ok(_) => {
                    let _ = fs::remove_dir(&local);
                    let shown = crate::sshcfg::collapse_tilde(&local);
                    match forget.then(|| mounts::forget(&local)) {
                        Some(Err(e)) => self.set_failed(format!("delete failed: {e:#}")),
                        Some(Ok(_)) => self.set_status(format!(
                            "fusermount -u -z {shown}: lazy-unmounted and deleted"
                        )),
                        None => {
                            self.set_status(format!("fusermount -u -z {shown}: lazy-unmounted"))
                        }
                    }
                    self.refresh_mounts();
                }
                Err(e) => self.set_failed(format!("lazy unmount failed: {e}")),
            },
            // A busy mount leaves its line, and the lazy-unmount offer in its place.
            ConfirmAction::DeleteMount { local, on }
                if on && !self.unmount(local.clone(), true) => {}
            ConfirmAction::DeleteMount { local, .. } => match mounts::forget(&local) {
                Ok(_) => {
                    self.refresh_mounts();
                    self.set_status(format!(
                        "deleted {} from the mounts you keep",
                        crate::sshcfg::collapse_tilde(&local)
                    ));
                }
                Err(e) => self.set_failed(format!("delete failed: {e}")),
            },
            ConfirmAction::DeleteTunnel {
                kind,
                spec,
                host,
                label,
                pid,
            } => {
                if let Some(pid) = pid {
                    let _ = tunnels::kill(pid);
                }
                match tunnels::forget(kind, &spec, &host) {
                    Ok(_) => {
                        self.refresh_tunnels();
                        self.set_status(format!("deleted tunnel '{label}'"));
                    }
                    Err(e) => self.set_failed(format!("delete failed: {e}")),
                }
            }
            ConfirmAction::IdentitiesOnly { alias } => {
                match sshcfg::add_option(&alias, "IdentitiesOnly", "yes") {
                    Ok(false) => self.set_failed(format!(
                        "{alias} already sets IdentitiesOnly, so something else is offering too many keys"
                    )),
                    Ok(true) => {
                        self.refresh_hosts();
                        self.set_status(format!(
                            "{alias} now offers only its IdentityFile; press Enter to reconnect"
                        ));
                    }
                    Err(e) => self.set_failed(format!("could not edit {alias}: {e}")),
                }
            }
            ConfirmAction::KeyVia { target, key } => self.pick_key_via(target, key),
            ConfirmAction::InstallPaste(alias) => return Some(self.install_paste(alias)),
            ConfirmAction::RemovePaste(alias) => return self.remove_paste(alias),
            ConfirmAction::ClearKnownHost { target } => {
                let out = Command::new("ssh-keygen").arg("-R").arg(&target).output();
                let (ok, msg) = match out {
                    Ok(o) if o.status.success() => (
                        true,
                        format!("removed old key for {target}; press Enter to reconnect"),
                    ),
                    Ok(o) => (
                        false,
                        format!(
                            "ssh-keygen -R failed: {}",
                            String::from_utf8_lossy(&o.stderr).trim()
                        ),
                    ),
                    Err(e) => (false, format!("could not run ssh-keygen: {e}")),
                };
                self.set_result(ok, msg);
            }
        }
        None
    }

    /// Show the "host key changed" modal offering the `ssh-keygen -R` fix.
    pub(super) fn offer_known_hosts_fix(&mut self, host: &str, target: String) {
        self.confirm = Some(Confirm::offer(
            "host key changed",
            format!(
                "{host}'s key no longer matches ~/.ssh/known_hosts. Usually the server was reinstalled or its IP was reused; rarely it is a man-in-the-middle. Only if you trust this change, drop the saved key (ssh-keygen -R {target}) and reconnect. Remove it now?"
            ),
            ConfirmAction::ClearKnownHost { target },
        ));
    }

    /// Offer to stop the agent drowning a host's IdentityFile in other keys.
    pub(super) fn offer_identities_only(&mut self, alias: &str) {
        self.confirm = Some(Confirm::offer(
            "too many keys offered",
            format!(
                "{alias} gave up before its IdentityFile was tried, because your agent offered every key it holds first. Make ssh offer only that key (IdentitiesOnly yes in its block)?"
            ),
            ConfirmAction::IdentitiesOnly {
                alias: alias.to_string(),
            },
        ));
    }

    /// Offer to add `key` to a host that takes keys only and none of ours,
    /// through a host that can already log in there.
    pub(super) fn offer_key_via(&mut self, host: &str, key: PathBuf) {
        let name = key.file_name().unwrap_or_default().to_string_lossy();
        self.confirm = Some(Confirm::offer(
            "no way in",
            format!(
                "{host} accepts keys only and none of yours, so ssh-copy-id cannot log in. Add {name}.pub there through another host that can (ssh -t <that host> ssh {host})?"
            ),
            ConfirmAction::KeyVia {
                target: host.to_string(),
                key,
            },
        ));
    }
}
