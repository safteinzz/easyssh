//! Yes/No gates in front of anything destructive, and the offered fixes that
//! follow a failure.

use ratatui::crossterm::event::{KeyCode, KeyEvent};
use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::*;

/// A titled yes/no modal: confirm a destructive action, or offer a fix after
/// something went wrong (a busy unmount, a changed host key).
pub(crate) struct Confirm {
    pub(crate) title: String,
    pub(crate) message: String,
    pub(crate) action: ConfirmAction,
    /// Which button is selected. Starts on No: these modals gate destructive or
    /// forceful things, so a reflex Enter must never be the one that fires them.
    pub(crate) yes: bool,
}

impl Confirm {
    pub(super) fn new(
        title: impl Into<String>,
        message: impl Into<String>,
        action: ConfirmAction,
    ) -> Self {
        Self {
            title: title.into(),
            message: message.into(),
            action,
            yes: false,
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
    // Size the box to the wrapped message so short prompts stay small and long
    // ones (a host-key warning) are not cut off. Padding gives every line, wrapped
    // continuations included, a uniform margin instead of butting the border.
    let width = (area.width * 66 / 100).clamp(34, 74);
    let inner = width.saturating_sub(6) as usize; // borders (2) + horizontal padding (4)
    let msg_rows = wrapped_line_count(&c.message, inner) as u16;
    // borders (2) + vertical padding (2) + blank + buttons. Forgetting the borders
    // here clipped the buttons off every modal, leaving no visible way to answer.
    let height = (msg_rows + 6).min(area.height);

    let rect = Rect {
        x: area.x + area.width.saturating_sub(width) / 2,
        y: area.y + area.height.saturating_sub(height) / 2,
        width,
        height,
    };
    f.render_widget(Clear, rect);

    // The selected button is highlighted, so the answer is visible at a glance
    // rather than being a keystroke you had to know about.
    let button = |label: &str, selected: bool| {
        let style = if selected {
            Style::default()
                .fg(Color::Black)
                .bg(Color::Red)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        Span::styled(format!(" {label} "), style)
    };
    let lines = vec![
        Line::raw(c.message.clone()),
        Line::raw(""),
        Line::from(vec![
            button("Yes (y)", c.yes),
            Span::raw("  "),
            button("No (n)", !c.yes),
            Span::styled(
                "   ←/→ then Enter",
                Style::default().add_modifier(Modifier::DIM),
            ),
        ]),
    ];
    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", c.title))
                .border_style(Style::default().fg(Color::Red))
                .padding(Padding::new(2, 2, 1, 1)),
        )
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
        self.confirm = Some(Confirm::new(
            "WARNING: host key changed",
            format!(
                "{host}'s key no longer matches ~/.ssh/known_hosts. Usually the server was reinstalled or its IP was reused; rarely it is a man-in-the-middle. Only if you trust this change, drop the saved key (ssh-keygen -R {target}) and reconnect. Remove it now?"
            ),
            ConfirmAction::ClearKnownHost { target },
        ));
    }
}
