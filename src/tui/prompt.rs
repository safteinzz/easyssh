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
    /// The cursor in `value`, as characters after it (`line_edit::edit`).
    pub(crate) back: usize,
    pub(crate) kind: Kind,
    /// Which option a `Choice` field has selected. Unused by the other kinds.
    pub(crate) choice: usize,
    /// Only shown, and only reachable, while field `.0`'s choice is one of `.1`.
    /// Hidden fields keep their slot in the vec, so field indices stay stable.
    pub(crate) show_if: Option<(usize, Vec<usize>)>,
    /// Answered before the form opened, by the row the wizard was launched
    /// from. It is carried and submitted, never shown and never reached: a
    /// value you cannot change is not a question, and the title says what it is.
    pub(crate) fixed: bool,
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
            back: 0,
            kind: Kind::Text,
            choice: 0,
            show_if: None,
            fixed: false,
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
            Kind::Choice(options) if options.len() == 2 => {
                format!(" {}    {} ", options[0], options[1])
            }
            Kind::Choice(options) => format!("‹ {} ›", options[self.choice]),
        }
    }

    pub(super) fn is_choice(&self) -> bool {
        matches!(self.kind, Kind::Choice(_))
    }

    /// The picked option of a choice field, or the typed value of a text one.
    pub(super) fn answer(&self) -> &str {
        match &self.kind {
            Kind::Text => self.value.trim(),
            Kind::Choice(options) => &options[self.choice],
        }
    }
}

/// `ForwardAgent` as a no/yes toggle. A socket path already in the config
/// becomes a third option, so editing the host does not quietly drop it.
fn forward_agent_field(current: Option<&str>) -> Field {
    let current = current.unwrap_or("no").trim();
    let mut options = vec!["no", "yes"];
    let choice = match options.iter().position(|o| o.eq_ignore_ascii_case(current)) {
        Some(i) => i,
        None => {
            options.push(current);
            2
        }
    };
    let mut f = Field::choice("ForwardAgent", &options);
    f.choice = choice;
    f
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
        /// Whether this host's config forces a command, and so whether the ssh
        /// client has to be wrapped. Captured when the wizard opens, because
        /// the preview cannot read `~/.ssh/config` for itself.
        forced_command: bool,
    },
    /// Change one typed setting; cycled ones never open a wizard.
    EditSetting {
        key: String,
        label: String,
    },
    /// Open or change one port forward. `editing` says what the row was
    /// before, when this wizard is rewriting one rather than making one.
    Tunnel {
        editing: Option<Edited>,
    },
}

/// The row a tunnel wizard is rewriting: the forward it was, which is the key of
/// its line, and the process it is running under when it is up.
#[derive(Clone)]
pub(crate) struct Edited {
    pub(crate) kind: char,
    pub(crate) spec: String,
    pub(crate) host: String,
    pub(crate) pid: Option<u32>,
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
                forward_agent_field(None),
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
                forward_agent_field(Some(&sshcfg::forward_agent_for(&h.alias))),
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
    pub(super) fn mount(host: String, forced_command: bool, settings: &Settings) -> Self {
        let root = settings.mount_root.trim_end_matches('/');
        Self {
            title: format!("mount {host} on a local folder (sshfs)"),
            idx: 0,
            action: Action::Mount {
                host: host.clone(),
                forced_command,
            },
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

    /// The tunnel wizard, one form for both directions and both ways in: `t`
    /// and `T` on a host pre-answer the direction and the host, `c` on the
    /// Tunnels tab answers neither. The two halves of the form are the two
    /// directions, and only the one being asked for is ever on screen.
    pub(super) fn tunnel(kind: char, host: &str) -> Self {
        let mut p = Self {
            title: "open a port forward (ssh -N)".into(),
            idx: 0,
            action: Action::Tunnel { editing: None },
            fields: tunnel_fields(),
        };
        p.fields[T_DIR].choice = usize::from(kind == 'R');
        // The host is answered by the row this was opened from, so it is carried in
        // the title instead, and the cursor starts on the first open question.
        if !host.is_empty() {
            p.title = format!("open a port forward on {host} (ssh -N)");
            p.fields[T_HOST].value = host.to_string();
            p.fields[T_HOST].fixed = true;
            p.idx = if kind == 'R' { T_R_OPEN } else { T_L_OPEN };
        }
        p
    }

    /// The same wizard over a forward that already exists.
    pub(super) fn edit_tunnel(e: &tunnels::Entry) -> Self {
        let (open, target, port) = e.ports().unwrap_or_default();
        let mut p = Self {
            title: format!("edit tunnel '{}'", e.label()),
            idx: 0,
            action: Action::Tunnel {
                editing: Some(Edited {
                    kind: e.kind,
                    spec: e.spec.clone(),
                    host: e.host.clone(),
                    pid: e.pid(),
                }),
            },
            fields: tunnel_fields(),
        };
        p.fields[T_DIR].choice = usize::from(e.kind == 'R');
        p.fields[T_HOST].value = e.host.clone();
        p.fields[T_NAME].value = e.name.clone().unwrap_or_default();
        let (a, b, c) = if e.kind == 'L' {
            (T_L_OPEN, T_L_HOST, T_L_PORT)
        } else {
            (T_R_OPEN, T_R_HOST, T_R_PORT)
        };
        p.fields[a].value = open.to_string();
        p.fields[b].value = target.to_string();
        p.fields[c].value = port.to_string();
        p
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
            // Which side a name is looked up on is the one thing about a
            // forward that is not guessable, and it stays true once typed.
            Action::Tunnel { .. } if self.fields[T_DIR].choice == 0 => {
                "the remote host is resolved over there, so localhost is the host itself"
            }
            Action::Tunnel { .. } => {
                "the local host is resolved here, so localhost is this machine"
            }
            _ => "",
        }
    }

    /// Whether field `i` applies to the answers given so far. Hidden fields keep
    /// their slot so indices stay stable, but are neither drawn nor reachable.
    pub(super) fn visible(&self, i: usize) -> bool {
        if self.fields[i].fixed {
            return false;
        }
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
            Action::Mount {
                host,
                forced_command,
            } => {
                // The same spec the run path builds, with `~` put back the way you
                // typed it: an absolute home path is unreadable in a box this wide.
                let argv: Vec<String> = MountSpec::from_fields(host, *forced_command, &self.fields)
                    .argv()
                    .into_iter()
                    .map(|a| sshcfg::collapse_tilde(&a))
                    .collect();
                Some(shell_join(&argv))
            }
            Action::Tunnel { .. } => {
                // Built by the same resolver the submit path uses, so what is
                // shown here is the command that runs, blanks and all.
                let (kind, spec, host) = tunnel_spec(&self.fields);
                Some(format!("ssh -N -{kind} {spec} {host}"))
            }
            Action::AddHost | Action::EditHost { .. } | Action::EditSetting { .. } => None,
        }
    }
}

/// The tunnel wizard's fields, by name. The form holds both directions at
/// once and hides the half it is not asking about, so the indices stay put and
/// only the visible ones ever mean anything.
const T_DIR: usize = 0;
const T_HOST: usize = 1;
/// `-L`: the port opened here, then the host and port the far side dials.
const T_L_OPEN: usize = 2;
const T_L_HOST: usize = 3;
const T_L_PORT: usize = 4;
/// `-R`: the port opened there, then the host and port this side dials.
const T_R_OPEN: usize = 5;
const T_R_HOST: usize = 6;
const T_R_PORT: usize = 7;
/// Last, because it is the only optional thing in the form and the row already
/// reads without it: a forward is listed by what it does unless you say better.
const T_NAME: usize = 8;

/// Both directions, written in the order ssh writes the spec so the form reads
/// as the command in the preview underneath it rather than as its mirror. The
/// labels swap sides between them, which is why they are two sets of fields and
/// not one: `Local port` means the port you dial in a `-L` and the port that is
/// served in a `-R`, and a form that reused the row would have to lie in one of
/// them.
fn tunnel_fields() -> Vec<Field> {
    vec![
        Field::choice(
            "Direction",
            &["reach a remote port (-L)", "expose a local port (-R)"],
        ),
        Field::new("Host", "")
            .required()
            .hint("a host in ~/.ssh/config"),
        Field::new("Local port", "= remote")
            .hint("where you'll reach it")
            .shown_when(T_DIR, &[0]),
        // The service does not have to live on the host itself: the middle of a
        // `-L` spec is resolved over there, so anything that box can reach is
        // reachable from here through it.
        Field::new("Remote host", "localhost")
            .hint("or a box the host can reach")
            .shown_when(T_DIR, &[0]),
        Field::new("Remote port", "")
            .hint("the service's port")
            .required()
            .shown_when(T_DIR, &[0]),
        Field::new("Remote port", "= local")
            .hint("opened on the host")
            .shown_when(T_DIR, &[1]),
        // The mirror of the forward's remote host: the middle of a `-R` spec is
        // resolved here, so you can hand the far side something on your LAN and
        // not only something of your own.
        Field::new("Local host", "localhost")
            .hint("or a box this machine can reach")
            .shown_when(T_DIR, &[1]),
        Field::new("Local port", "")
            .hint("the service's port")
            .required()
            .shown_when(T_DIR, &[1]),
        // The row is spelled out from the ports when this is blank, so a name
        // is only ever for saying what the ports cannot: what it is *for*.
        Field::new("Name", "").hint("a label, e.g. pihole"),
    ]
}

/// The forward these fields describe: the flag, the spec and the host. The
/// preview and the command that runs both come through here, so they cannot
/// disagree about what a blank field means.
pub(super) fn tunnel_spec(fields: &[Field]) -> (char, String, String) {
    let v = |i: usize| fields[i].value.trim();
    let host = v(T_HOST).to_string();
    if fields[T_DIR].choice == 0 {
        let target = non_blank(v(T_L_HOST), "localhost");
        let remote = v(T_L_PORT);
        let local = non_blank(v(T_L_OPEN), remote);
        ('L', format!("{local}:{target}:{remote}"), host)
    } else {
        let target = non_blank(v(T_R_HOST), "localhost");
        let local = v(T_R_PORT);
        let remote = non_blank(v(T_R_OPEN), local);
        ('R', format!("{remote}:{target}:{local}"), host)
    }
}

/// The label typed over this forward, or `None` when it is left to speak for
/// itself.
pub(super) fn tunnel_name(fields: &[Field]) -> Option<String> {
    let name = fields[T_NAME].value.trim();
    (!name.is_empty()).then(|| name.to_string())
}

/// Why these fields are not a forward yet, in the words of what to do about it.
/// `None` when they are one.
pub(super) fn tunnel_problem(fields: &[Field]) -> Option<String> {
    let v = |i: usize| fields[i].value.trim();
    let forward = fields[T_DIR].choice == 0;
    if v(T_HOST).is_empty() {
        return Some("a host is required - ctrl-o picks one from ~/.ssh/config".into());
    }
    let (port, side) = if forward {
        (v(T_L_PORT), "a remote port")
    } else {
        (v(T_R_PORT), "a local port")
    };
    if port.is_empty() {
        return Some(format!("{side} is required"));
    }
    // A `host:port` here would make the spec four fields, which is one we can
    // no longer read back; catch it rather than letting ssh answer for us.
    let (target, label) = if forward {
        (v(T_L_HOST), "the remote host")
    } else {
        (v(T_R_HOST), "the local host")
    };
    if target.contains(':') {
        return Some(format!("{label} takes no `:` port - use the port field"));
    }
    None
}

/// The most rows any one answer to field `on` can add to the form. A lone
/// optional field reserves its row, so answering the question above it does not
/// resize the frame under the cursor - but two branches of the same size swap
/// row for row and need nothing spare, which is what keeps the tunnel wizard
/// from carrying three blank rows for the direction it is not asking about.
fn reserved_rows(fields: &[Field], on: usize) -> usize {
    let answers = match &fields[on].kind {
        Kind::Choice(options) => options.len(),
        _ => 1,
    };
    (0..answers)
        .map(|answer| {
            fields
                .iter()
                .filter(|f| {
                    matches!(&f.show_if, Some((c, values)) if *c == on && values.contains(&answer))
                })
                .count()
        })
        .max()
        .unwrap_or(0)
}

/// Where each hidden field's reserved rows are made up: at the end of the block
/// it belongs to, so the fields on screen stay together and the gap is always in
/// the same place.
fn reserved_after(fields: &[Field], visible: impl Fn(usize) -> bool) -> Vec<usize> {
    let mut pad = vec![0usize; fields.len()];
    for on in 0..fields.len() {
        let block: Vec<usize> = (0..fields.len())
            .filter(|&i| matches!(&fields[i].show_if, Some((c, _)) if *c == on))
            .collect();
        let Some(&last) = block.last() else {
            continue;
        };
        let shown = block.iter().filter(|&&i| visible(i)).count();
        pad[last] += reserved_rows(fields, on).saturating_sub(shown);
    }
    pad
}

fn non_blank<'a>(value: &'a str, fallback: &'a str) -> &'a str {
    if value.is_empty() { fallback } else { value }
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
    // Rows held clear for the fields an answer hides, so the box never resizes
    // under the cursor. Made up at the end of each block, since two branches of
    // the same size need none.
    let pad = reserved_after(&p.fields, |i| p.visible(i));
    for (i, field) in p.fields.iter().enumerate() {
        let blanks = pad[i];
        if !p.visible(i) {
            for _ in 0..blanks {
                lines.push(Line::raw(""));
                texts.push(String::new());
            }
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
        let tail = if let Kind::Choice(options) = &field.kind
            && options.len() == 2
        {
            // A toggle is the house buttons, the picked one filled.
            for (i, option) in options.iter().enumerate() {
                if i > 0 {
                    spans.push(Span::raw("  "));
                }
                let style = if i == field.choice {
                    Style::default()
                        .fg(Color::Black)
                        .bg(Color::Cyan)
                        .add_modifier(Modifier::BOLD)
                } else {
                    dim
                };
                spans.push(Span::styled(format!(" {option} "), style));
            }
            String::new()
        } else if field.is_choice() {
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
        } else if active {
            spans.extend(line_edit::with_cursor(&value, field.back, Style::default()));
            String::new()
        } else {
            spans.push(Span::raw(value.clone()));
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
        for _ in 0..blanks {
            lines.push(Line::raw(""));
            texts.push(String::new());
        }
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
    let mut hint = "enter next/submit · esc cancel".to_string();
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
