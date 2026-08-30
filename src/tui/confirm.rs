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
    },
    /// Run `ssh-keygen -R <target>` to drop a host key that no longer matches.
    ClearKnownHost {
        target: String,
    },
}

pub(super) fn render_confirm(f: &mut Frame, area: Rect, c: &Confirm) {
    // A gate is red and starts on No; an offer is cyan and starts on Yes. The
    // colour says how much is at stake, the default focus says what Enter does.
    let accent = if c.danger { Color::Red } else { Color::Cyan };
    let width = box_width(area.width);
    let msg_rows = wrapped_line_count(&c.message, box_inner_width(width)) as u16;
    // The message, a blank, and the button row.
    // The message, a blank, the buttons, a blank, the keys.
    let rect = box_area(area, width, box_height(msg_rows + 4, area.height));
    f.render_widget(Clear, rect);

    let para = Paragraph::new(vec![
        Line::raw(c.message.clone()),
        Line::raw(""),
        box_buttons(accent, c.yes),
        Line::raw(""),
        box_hint("h/l ←/→ move · enter select · y/n"),
    ])
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
                Err(e) => self.set_status(format!("delete failed: {e}")),
            },
            ConfirmAction::LazyUnmount { local } => match mounts::unmount_lazy(&local) {
                Ok(_) => {
                    let _ = fs::remove_dir(&local);
                    self.refresh_mounts();
                    self.set_status(format!("fusermount -u -z {local}: lazy-unmounted"));
                }
                Err(e) => self.set_status(format!("lazy unmount failed: {e}")),
            },
            ConfirmAction::ClearKnownHost { target } => {
                let out = Command::new("ssh-keygen").arg("-R").arg(&target).output();
                self.set_status(match out {
                    Ok(o) if o.status.success() => {
                        format!("removed old key for {target}; press Enter to reconnect")
                    }
                    Ok(o) => format!(
                        "ssh-keygen -R failed: {}",
                        String::from_utf8_lossy(&o.stderr).trim()
                    ),
                    Err(e) => format!("could not run ssh-keygen: {e}"),
                });
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
}
