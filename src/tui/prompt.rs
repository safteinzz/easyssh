//! The wizards: a list of fields, what kind each one is, how the prompt steps
//! through them, and how it draws.

use ratatui::prelude::*;
use ratatui::widgets::{Clear, Paragraph, Wrap};

use super::*;

/// One editable line in a wizard: a label in the left column, a value in the
/// right. `default` is what a blank field submits (the semantics differ per
/// action) and stands in as the dim example until something is typed.
pub(crate) struct Field {
    pub(crate) label: String,
    pub(crate) default: String,
    /// The dim example shown in the value column while the field is empty: what
    /// the field wants, not what it is called. It lives here rather than in the
    /// label so every label stays one short noun and the values line up.
    pub(crate) hint: String,
    pub(crate) value: String,
    pub(crate) kind: Kind,
    /// Which option a `Choice` field has selected. Unused by the other kinds.
    pub(crate) choice: usize,
    /// Only shown, and only reachable, while field `.0`'s choice is one of `.1`.
    /// Hidden fields keep their slot in the vec, so field indices stay stable.
    pub(crate) show_if: Option<(usize, Vec<usize>)>,
    /// Marked with a red `*`, the form convention everyone already reads. Only
    /// set it on a field the submit path actually refuses to go without, or the
    /// star is a lie: everything unmarked can be left blank.
    pub(crate) required: bool,
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
            hint: String::new(),
            value: String::new(),
            kind: Kind::Text,
            choice: 0,
            show_if: None,
            required: false,
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
    /// Builder: the submit path refuses a blank here, so it gets the red `*`.
    pub(super) fn required(mut self) -> Self {
        self.required = true;
        self
    }

    /// Builder: the dim example shown while the field is empty.
    pub(super) fn hint(mut self, hint: &str) -> Self {
        self.hint = hint.into();
        self
    }

    /// What stands in the value column while the field is empty: what a blank
    /// submits, then what it wants typed. Both are dim, and both are gone the
    /// moment there is a value, so nothing here can be mistaken for one.
    pub(super) fn placeholder(&self) -> String {
        [self.default.as_str(), self.hint.as_str()]
            .iter()
            .filter(|s| !s.is_empty())
            .copied()
            .collect::<Vec<_>>()
            .join("  ")
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
            title: "add host to ~/.ssh/config".into(),
            idx: 0,
            action: Action::AddHost,
            fields: vec![
                Field::new("Alias", "")
                    .required()
                    .hint("what you type after ssh"),
                Field::new("HostName", "").hint("IP or DNS name"),
                Field::new("User", ""),
                Field::new("Port", "22"),
                Field::new("IdentityFile", "").hint("a key in ~/.ssh"),
                Field::new("ProxyJump", "").hint("a host to hop through"),
                Field::new("RemoteCommand", "").hint("runs instead of a shell, e.g. pwsh"),
            ],
        }
    }

    /// The add-host wizard, pre-filled with a host's current settings.
    pub(super) fn edit_host(h: &Host) -> Self {
        Self {
            title: format!("edit host '{}' in ~/.ssh/config", h.alias),
            idx: 0,
            action: Action::EditHost {
                original: h.alias.clone(),
            },
            fields: vec![
                Field::filled("Alias", &h.alias)
                    .required()
                    .hint("what you type after ssh"),
                Field::filled("HostName", h.hostname.as_deref().unwrap_or(""))
                    .hint("IP or DNS name"),
                Field::filled("User", h.user.as_deref().unwrap_or("")),
                Field::filled("Port", h.port.as_deref().unwrap_or("")),
                Field::filled("IdentityFile", h.identity.as_deref().unwrap_or(""))
                    .hint("a key in ~/.ssh"),
                Field::filled("ProxyJump", h.proxy_jump.as_deref().unwrap_or(""))
                    .hint("a host to hop through"),
                Field::filled("RemoteCommand", h.remote_command.as_deref().unwrap_or(""))
                    .hint("runs instead of a shell, e.g. pwsh"),
            ],
        }
    }

    pub(super) fn new_key(kind: &str) -> Self {
        Self {
            title: format!("generate a new key (ssh-keygen -t {kind})"),
            idx: 0,
            action: Action::NewKey {
                kind: kind.to_string(),
            },
            fields: vec![
                Field::new("Key name", &format!("id_{kind}")).hint("a file in ~/.ssh"),
                Field::new("Comment", "").hint("your email"),
            ],
        }
    }

    /// The mount wizard. Its two defaults come from Settings, so "where do
    /// mounts go" and "where is the sftp server" are answered once, not per mount.
    pub(super) fn mount(host: String, settings: &Settings) -> Self {
        let root = settings.mount_root.trim_end_matches('/');
        Self {
            title: format!("mount {host} on a local folder (sshfs)"),
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
                Field::new("sftp-server", &settings.sftp_server)
                    .hint("the binary on the server")
                    .shown_when(2, &[1]),
            ],
        }
    }

    pub(super) fn forward(host: String) -> Self {
        Self {
            title: format!("reach a service on {host} from this machine (ssh -L)"),
            idx: 0,
            action: Action::Forward { host: host.clone() },
            fields: vec![
                Field::new("Remote port", "").hint(&format!("the service on {host}")),
                Field::new("Local port", "= remote").hint("where you'll reach it"),
            ],
        }
    }

    /// A one-field wizard for a typed setting, pre-filled with what it is now.
    pub(super) fn edit_setting(row: &settings::Row) -> Self {
        let mut field = Field::filled(row.help, &row.value);
        field.default = row.default.clone();
        Self {
            title: format!("setting: {}", row.label),
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
            title: format!("expose a local port on {host} (ssh -R)"),
            idx: 0,
            action: Action::Reverse { host: host.clone() },
            fields: vec![
                Field::new("Local port", "").hint("yours to expose"),
                Field::new("Remote port", "= local").hint(&format!("opened on {host}")),
            ],
        }
    }

    /// The one line of guidance a form carries under its fields, dim, or empty
    /// for a form that needs none. It lives here rather than in a label because
    /// what it explains is true of the whole form and stays true once a field
    /// is filled in - a parenthetical in a pre-filled label is invisible where
    /// it is needed most.
    pub(super) fn note(&self) -> &'static str {
        match &self.action {
            Action::AddHost | Action::EditHost { .. } => {
                "ctrl-o fills IdentityFile or ProxyJump from what this machine already has"
            }
            _ => "",
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

/// The column a wizard's values start in, measured from the label's first
/// character. Fixed rather than measured off the longest label: the fields on
/// screen change as a wizard is answered, and a measured column would slide
/// every value sideways when they did. A longer label simply pushes its own
/// row rather than dragging the whole column out with it.
const LABEL_COL: usize = 14;
/// How far a wide form may push that column before the labels are the ones that
/// give way, so a single long label cannot shove every value off the box.
const LABEL_COL_MAX: usize = 22;

/// The columns a label cell eats: the label, its star, and the colon.
fn label_width(field: &Field) -> usize {
    field.label.chars().count() + usize::from(field.required) + 1
}

/// Where this form's values start.
fn value_column(fields: &[Field]) -> usize {
    let widest = fields.iter().map(label_width).max().unwrap_or(0) + 2;
    widest.clamp(LABEL_COL, LABEL_COL_MAX)
}

/// The wizard box: labels in one column, values in another, and nothing that
/// appears or disappears with the cursor. The key for a choice row used to be
/// printed on whichever row was focused, which made every row grow and shrink
/// as you moved through the form to repeat what the key line already says.
///
/// The preview line can be far wider than the box (a sudo mount wraps to
/// several rows), so the height is counted against the wrapped width and never
/// against the line count.
pub(super) fn render_prompt(f: &mut Frame, area: Rect, p: &Prompt) {
    // No leading blank: the box's own top padding is that row.
    let mut lines: Vec<Line> = Vec::new();
    // Plain text of every line, kept alongside so the box can be sized against
    // what the lines wrap to rather than how many there are.
    let mut texts: Vec<String> = Vec::new();
    let dim = Style::default().add_modifier(Modifier::DIM);
    let col = value_column(&p.fields);
    for (i, field) in p.fields.iter().enumerate() {
        // A field that does not apply to the answers so far keeps its row and
        // draws nothing: answering the question above it must not resize the
        // box under the cursor.
        if !p.visible(i) {
            lines.push(Line::raw(""));
            texts.push(String::new());
            continue;
        }
        let active = i == p.idx;
        // Required-ness is a property of the field, not of where the cursor is,
        // so the star keeps its colour while the label around it dims.
        let star = if field.required { "*" } else { "" };
        let label_style = if active {
            Style::default()
                .fg(Color::Cyan)
                .add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        let pad = " ".repeat(col.saturating_sub(label_width(field)).max(1));
        let mut spans = vec![
            Span::raw(if active { "▸ " } else { "  " }),
            Span::styled(field.label.clone(), label_style),
            Span::styled(star, Style::default().fg(Color::Red)),
            Span::styled(format!(":{pad}"), label_style),
        ];
        let value = field.display();
        let tail = if field.is_choice() {
            // A cycled answer, in the colour a form uses for the thing it will
            // submit; the guillemets are what say it can be stepped.
            let mut style = Style::default().fg(Color::Cyan);
            if active {
                style = style.add_modifier(Modifier::BOLD);
            }
            spans.push(Span::styled(value.clone(), style));
            String::new()
        } else if value.is_empty() {
            let example = field.placeholder();
            spans.push(Span::raw(if active { "█ " } else { "" }));
            spans.push(Span::styled(example.clone(), dim));
            example
        } else {
            spans.push(Span::raw(value.clone()));
            spans.push(Span::raw(if active { "█" } else { "" }));
            String::new()
        };
        texts.push(format!(
            "{}{}{}:{pad}{}{}",
            if active { "▸ " } else { "  " },
            field.label,
            star,
            value,
            tail
        ));
        lines.push(Line::from(spans));
    }
    // One line of guidance for the whole form, where a parenthetical in two
    // labels used to sit - and go missing exactly when the field was filled in.
    if !p.note().is_empty() {
        lines.push(Line::raw(""));
        texts.push(String::new());
        lines.push(Line::from(Span::styled(p.note(), dim)));
        texts.push(p.note().to_string());
    }
    // Live command preview: shows the exact command being built as you type, so
    // the wizard teaches the underlying tool instead of hiding it.
    if let Some(cmd) = p.command_preview() {
        lines.push(Line::raw(""));
        texts.push(String::new());
        texts.push(format!("  runs  {cmd}"));
        lines.push(Line::from(vec![
            Span::styled("  runs  ", dim),
            Span::styled(cmd, Style::default().fg(Color::Green)),
        ]));
    }
    let mut hint = "↑↓ tab move · ←→ choose · enter next/submit · esc cancel".to_string();
    if (0..p.fields.len()).any(|i| p.visible(i) && p.fields[i].required) {
        hint.push_str(" · * required");
    }
    let hint = hint;
    lines.push(Line::raw(""));
    lines.push(box_hint(&hint));
    texts.push(String::new());
    texts.push(hint.clone());

    // Size to the *wrapped* content: a sudo mount's preview is far wider than
    // the box, and counting lines instead of rows pushed the keys out through
    // the bottom border.
    let width = box_width(area.width);
    let rows: usize = texts
        .iter()
        .map(|t| wrapped_line_count(t, box_inner_width(width)))
        .sum();
    let rect = box_area(area, width, box_height(rows as u16, area.height));
    f.render_widget(Clear, rect);

    let para = Paragraph::new(lines)
        .block(super::widgets::box_block(Color::Cyan, &p.title))
        .wrap(Wrap { trim: false });
    f.render_widget(para, rect);
}
