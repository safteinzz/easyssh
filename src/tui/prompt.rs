//! The wizards: a list of fields, what kind each one is, how the prompt steps
//! through them, and how it draws.

use ratatui::prelude::*;
use ratatui::widgets::{Block, Borders, Clear, Paragraph, Wrap};

use super::*;

/// One editable line in a wizard. `default` is shown in brackets and used when
/// the field is left blank on submit (the semantics differ per action).
pub(crate) struct Field {
    pub(crate) label: String,
    pub(crate) default: String,
    pub(crate) value: String,
    pub(crate) kind: Kind,
    /// Which option a `Choice` field has selected. Unused by the other kinds.
    pub(crate) choice: usize,
    /// Only shown, and only reachable, while field `.0`'s choice is one of `.1`.
    /// Hidden fields keep their slot in the vec, so field indices stay stable.
    pub(crate) show_if: Option<(usize, Vec<usize>)>,
}

pub(crate) enum Kind {
    Text,
    /// A fixed set of answers cycled in place with `h`/`l` or the arrows. Nothing
    /// is typed here, which is what frees up plain `h`/`l` inside a wizard.
    Choice(Vec<String>),
}

impl Field {
    pub(super) fn new(label: &str, default: &str) -> Self {
        Self {
            label: label.into(),
            default: default.into(),
            value: String::new(),
            kind: Kind::Text,
            choice: 0,
            show_if: None,
        }
    }
    /// A field that starts pre-filled with `value` - for the edit wizard.
    pub(super) fn filled(label: &str, value: &str) -> Self {
        Self {
            value: value.into(),
            ..Self::new(label, "")
        }
    }
    pub(super) fn choice(label: &str, options: &[&str]) -> Self {
        let options = options.iter().map(|o| o.to_string()).collect();
        Self {
            kind: Kind::Choice(options),
            ..Self::new(label, "")
        }
    }
    /// Builder: show this field only while field `on`'s choice is one of `values`.
    pub(super) fn shown_when(mut self, on: usize, values: &[usize]) -> Self {
        self.show_if = Some((on, values.to_vec()));
        self
    }

    /// What the wizard paints for this field's value.
    pub(super) fn display(&self) -> String {
        match &self.kind {
            Kind::Text => self.value.clone(),
            Kind::Choice(options) => format!("‹ {} ›", options[self.choice]),
        }
    }

    pub(super) fn is_choice(&self) -> bool {
        matches!(self.kind, Kind::Choice(_))
    }
}

/// What a wizard does once submitted. Cloned out before we move the prompt, so
/// each variant owns whatever it needs.
#[derive(Clone)]
pub(crate) enum Action {
    AddHost,
    EditHost {
        original: String,
    },
    NewKey {
        kind: String,
    },
    Mount {
        host: String,
    },
    /// Change one typed setting; cycled ones never open a wizard.
    EditSetting {
        key: String,
        label: String,
    },
    Forward {
        host: String,
    },
    Reverse {
        host: String,
    },
}

/// A modal wizard: a titled stack of fields plus the action to run on submit.
pub(crate) struct Prompt {
    pub(crate) title: String,
    pub(crate) fields: Vec<Field>,
    pub(crate) idx: usize,
    pub(crate) action: Action,
}

impl Prompt {
    pub(super) fn cur_mut(&mut self) -> &mut Field {
        &mut self.fields[self.idx]
    }

    pub(super) fn add_host() -> Self {
        Self {
            title: "Add host to ~/.ssh/config".into(),
            idx: 0,
            action: Action::AddHost,
            fields: vec![
                Field::new("Alias (what you type after ssh)", ""),
                Field::new("HostName (IP or DNS name)", ""),
                Field::new("User", ""),
                Field::new("Port", "22"),
                Field::new("IdentityFile (Ctrl-o to pick a key, optional)", ""),
                Field::new("ProxyJump (Ctrl-o to pick a host to hop through)", ""),
            ],
        }
    }

    /// The add-host wizard, pre-filled with a host's current settings.
    pub(super) fn edit_host(h: &Host) -> Self {
        Self {
            title: format!("Edit host '{}' in ~/.ssh/config", h.alias),
            idx: 0,
            action: Action::EditHost {
                original: h.alias.clone(),
            },
            fields: vec![
                Field::filled("Alias (what you type after ssh)", &h.alias),
                Field::filled(
                    "HostName (IP or DNS name)",
                    h.hostname.as_deref().unwrap_or(""),
                ),
                Field::filled("User", h.user.as_deref().unwrap_or("")),
                Field::filled("Port", h.port.as_deref().unwrap_or("")),
                Field::filled(
                    "IdentityFile (Ctrl-o to pick a key, optional)",
                    h.identity.as_deref().unwrap_or(""),
                ),
                Field::filled(
                    "ProxyJump (Ctrl-o to pick a host to hop through)",
                    h.proxy_jump.as_deref().unwrap_or(""),
                ),
            ],
        }
    }

    pub(super) fn new_key(kind: &str) -> Self {
        Self {
            title: format!("Generate a new key (ssh-keygen -t {kind})"),
            idx: 0,
            action: Action::NewKey {
                kind: kind.to_string(),
            },
            fields: vec![
                Field::new("Key name (file in ~/.ssh)", &format!("id_{kind}")),
                Field::new("Comment (your email)", ""),
            ],
        }
    }

    /// The mount wizard. Its two defaults come from Settings, so "where do
    /// mounts go" and "where is the sftp server" are answered once, not per mount.
    pub(super) fn mount(host: String, settings: &Settings) -> Self {
        let root = settings.mount_root.trim_end_matches('/');
        Self {
            title: format!("Mount {host} on a local folder (sshfs)"),
            idx: 0,
            action: Action::Mount { host: host.clone() },
            fields: vec![
                Field::new("Remote path", "~ (home)"),
                Field::new("Local mountpoint", &format!("{root}/{host}")),
                Field::choice(
                    "Remote rights",
                    &["your user (no sudo)", "root (requires NOPASSWD sudo)"],
                ),
                // Only the sudo mode needs the server-side binary, so it stays
                // hidden for a plain mount.
                Field::new("sftp-server on the server", &settings.sftp_server).shown_when(2, &[1]),
            ],
        }
    }

    pub(super) fn forward(host: String) -> Self {
        Self {
            title: format!("Reach a service on {host} from this machine (ssh -L)"),
            idx: 0,
            action: Action::Forward { host: host.clone() },
            fields: vec![
                Field::new(&format!("Remote port (the service on {host})"), ""),
                Field::new("Local port (where you'll reach it)", "= remote"),
            ],
        }
    }

    /// A one-field wizard for a typed setting, pre-filled with what it is now.
    pub(super) fn edit_setting(row: &settings::Row) -> Self {
        let mut field = Field::filled(row.help, &row.value);
        field.default = row.default.clone();
        Self {
            title: format!("Setting: {}", row.label),
            idx: 0,
            action: Action::EditSetting {
                key: row.key.to_string(),
                label: row.label.to_string(),
            },
            fields: vec![field],
        }
    }

    pub(super) fn reverse(host: String) -> Self {
        Self {
            title: format!("Expose a local port on {host} (ssh -R)"),
            idx: 0,
            action: Action::Reverse { host: host.clone() },
            fields: vec![
                Field::new("Local port (yours to expose)", ""),
                Field::new(&format!("Remote port (opened on {host})"), "= local"),
            ],
        }
    }

    /// Whether field `i` applies to the answers given so far. Hidden fields keep
    /// their slot so indices stay stable, but are neither drawn nor reachable.
    pub(super) fn visible(&self, i: usize) -> bool {
        match &self.fields[i].show_if {
            None => true,
            Some((on, values)) => values.contains(&self.fields[*on].choice),
        }
    }

    /// The next visible field in `dir` (+1/-1), wrapping. Falls back to the
    /// current index, so a wizard whose fields all vanished cannot spin forever.
    pub(super) fn step(&self, dir: isize) -> usize {
        let len = self.fields.len();
        let mut i = self.idx;
        for _ in 0..len {
            i = (i as isize + dir).rem_euclid(len as isize) as usize;
            if self.visible(i) {
                return i;
            }
        }
        self.idx
    }

    /// True when there is no visible field after this one, so Enter submits.
    pub(super) fn on_last_field(&self) -> bool {
        !(self.idx + 1..self.fields.len()).any(|i| self.visible(i))
    }

    /// The exact command this wizard will run, rebuilt from the current field
    /// values so it updates live as you type. `None` for wizards that write the
    /// config rather than run one command. Resolution mirrors `submit_prompt`.
    pub(super) fn command_preview(&self) -> Option<String> {
        let v = |i: usize| self.fields[i].value.trim();
        match &self.action {
            Action::NewKey { kind } => {
                let name = if v(0).is_empty() {
                    format!("id_{kind}")
                } else {
                    v(0).to_string()
                };
                let mut cmd = format!("ssh-keygen -t {kind} -f ~/.ssh/{name}");
                if kind == "rsa" {
                    cmd.push_str(" -b 4096");
                }
                if !v(1).is_empty() {
                    cmd.push_str(&format!(" -C {}", v(1)));
                }
                Some(cmd)
            }
            Action::Mount { host } => {
                // Built from the same spec the run path uses, so the mountpoint
                // and the sudo wrapping shown here are exactly what gets executed.
                // The one difference is cosmetic: a path under home is shown the
                // way you would type it, since a shell expands `~` itself and an
                // absolute home path is unreadable in a box this wide.
                let argv: Vec<String> = MountSpec::from_fields(host, &self.fields)
                    .argv()
                    .into_iter()
                    .map(|a| sshcfg::collapse_tilde(&a))
                    .collect();
                Some(shell_join(&argv))
            }
            Action::Forward { host } => {
                let remote = v(0);
                let local = if v(1).is_empty() { remote } else { v(1) };
                Some(format!("ssh -N -L {local}:localhost:{remote} {host}"))
            }
            Action::Reverse { host } => {
                let local = v(0);
                let remote = if v(1).is_empty() { local } else { v(1) };
                Some(format!("ssh -N -R {remote}:localhost:{local} {host}"))
            }
            Action::AddHost | Action::EditHost { .. } | Action::EditSetting { .. } => None,
        }
    }
}

/// Wizard box width, as the percentage of the area `centered` takes. The preview
/// line can be far wider than the box (a sudo mount wraps to several rows), so
/// the height has to be counted against the wrapped width, not the line count.
pub(super) const PROMPT_PCT: u16 = 72;

pub(super) fn render_prompt(f: &mut Frame, area: Rect, p: &Prompt) {
    let mut lines: Vec<Line> = vec![Line::raw("")];
    // Plain text of every line, kept alongside so the box can be sized against
    // what the lines wrap to rather than how many there are.
    let mut texts: Vec<String> = vec![String::new()];
    for (i, field) in p.fields.iter().enumerate() {
        // A field that does not apply to the answers so far is not drawn at all.
        if !p.visible(i) {
            continue;
        }
        let active = i == p.idx;
        let head = if field.default.is_empty() {
            format!("{}: ", field.label)
        } else {
            format!("{} [{}]: ", field.label, field.default)
        };
        let label_style = if active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        // A choice has no text cursor; it shows its options key instead, so the
        // way to change it is on screen rather than something you must know.
        let (value_style, tail) = if field.is_choice() {
            let style = if active {
                Style::default()
                    .fg(Color::Yellow)
                    .add_modifier(Modifier::BOLD)
            } else {
                Style::default()
            };
            (style, if active { "   h/l or ←/→" } else { "" })
        } else {
            (Style::default(), if active { "█" } else { "" })
        };
        texts.push(format!(
            "{}{}{}{}",
            if active { "▸ " } else { "  " },
            head,
            field.display(),
            tail
        ));
        lines.push(Line::from(vec![
            Span::raw(if active { "▸ " } else { "  " }),
            Span::styled(head, label_style),
            Span::styled(field.display(), value_style),
            Span::styled(
                tail.to_string(),
                Style::default().add_modifier(Modifier::DIM),
            ),
        ]));
    }
    // Live command preview: shows the exact command being built as you type, so
    // the wizard teaches the underlying tool instead of hiding it.
    if let Some(cmd) = p.command_preview() {
        lines.push(Line::raw(""));
        texts.push(String::new());
        texts.push(format!("  runs  {cmd}"));
        lines.push(Line::from(vec![
            Span::styled("  runs  ", Style::default().add_modifier(Modifier::DIM)),
            Span::styled(cmd, Style::default().fg(Color::Green)),
        ]));
    }
    let hint = "  Enter next/submit   Ctrl-j/k · Ctrl-↑↓ · Tab move field · Esc cancel";
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        hint,
        Style::default().add_modifier(Modifier::DIM),
    )));
    texts.push(String::new());
    texts.push(hint.to_string());

    // Size to the *wrapped* content: a sudo mount's preview is far wider than the
    // box, and counting lines instead of rows pushed the hint out of the border.
    let inner = (area.width * PROMPT_PCT / 100).saturating_sub(2) as usize;
    let rows: usize = texts.iter().map(|t| wrapped_line_count(t, inner)).sum();
    let height = ((rows + 2) as u16).min(area.height);
    let rect = centered(area, PROMPT_PCT, height);
    f.render_widget(Clear, rect);

    let para = Paragraph::new(lines)
        .block(
            Block::default()
                .borders(Borders::ALL)
                .title(format!(" {} ", p.title))
                .border_style(Style::default().fg(Color::Cyan)),
        )
        .wrap(Wrap { trim: false });
    f.render_widget(para, rect);
}
