//! The toolbox - bare `essh`. Three tabs (Hosts / Keys / Tunnels) and a set of
//! wizards that stand in for the commands nobody remembers: `ssh-keygen`,
//! `ssh-copy-id`, `ssh -L/-R`, `sshfs`, and hand-editing `~/.ssh/config`.
//!
//! Anything interactive (connecting, generating a key, copying a key, mounting)
//! runs *suspended*: we drop out of the alternate screen, hand the real terminal
//! to the child process so it can prompt for passwords/passphrases, then restore
//! the UI. Tunnels are spawned detached and tracked, so they don't need that.

use crate::keys::{self};
use crate::mounts;
use crate::sshcfg::{self, Host, NewHost};
use crate::tunnels;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers},
    execute,
    terminal::{disable_raw_mode, enable_raw_mode, EnterAlternateScreen, LeaveAlternateScreen},
};
use ratatui::{
    backend::CrosstermBackend,
    layout::{Constraint, Direction, Layout, Rect},
    style::{Color, Modifier, Style},
    text::{Line, Span},
    widgets::{Block, Borders, Clear, List, ListItem, ListState, Paragraph, Tabs, Wrap},
    Frame, Terminal,
};
use std::fs;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

type Term = Terminal<CrosstermBackend<Stdout>>;

/// The tabs. Order here is the left-to-right / Tab-cycle order.
#[derive(Clone, Copy, PartialEq)]
enum View {
    Hosts,
    Keys,
    Tunnels,
    Mounts,
}

const VIEWS: [View; 4] = [View::Hosts, View::Keys, View::Tunnels, View::Mounts];

const HOSTS_HINTS: &str = "↵ connect · n new · e edit · d delete · m mount · t/T tunnel · R fix-key · h/l view · ? help · q quit";
const KEYS_HINTS: &str = "j/k ↑↓ move · h/l ←→ view · n new key · c copy to host · ? help · q quit";
const TUNNELS_HINTS: &str = "j/k ↑↓ move · h/l ←→ view · d kill · r refresh · ? help · q quit";
const MOUNTS_HINTS: &str = "j/k ↑↓ move · h/l ←→ view · d unmount · r refresh · ? help · q quit";

/// One editable line in a wizard. `default` is shown in brackets and used when
/// the field is left blank on submit (the semantics differ per action).
struct Field {
    label: String,
    default: String,
    value: String,
}

impl Field {
    fn new(label: &str, default: &str) -> Self {
        Self { label: label.into(), default: default.into(), value: String::new() }
    }
    /// A field that starts pre-filled with `value` - for the edit wizard.
    fn filled(label: &str, value: &str) -> Self {
        Self { label: label.into(), default: String::new(), value: value.into() }
    }
}

/// What a wizard does once submitted. Cloned out before we move the prompt, so
/// each variant owns whatever it needs.
#[derive(Clone)]
enum Action {
    AddHost,
    EditHost { original: String },
    NewKey,
    CopyKey { key: PathBuf },
    Mount { host: String },
    Forward { host: String },
    Reverse { host: String },
}

/// A modal wizard: a titled stack of fields plus the action to run on submit.
struct Prompt {
    title: String,
    fields: Vec<Field>,
    idx: usize,
    action: Action,
}

impl Prompt {
    fn cur_mut(&mut self) -> &mut Field {
        &mut self.fields[self.idx]
    }

    fn add_host() -> Self {
        Self {
            title: "Add host to ~/.ssh/config".into(),
            idx: 0,
            action: Action::AddHost,
            fields: vec![
                Field::new("Alias (what you type after ssh)", ""),
                Field::new("HostName (IP or DNS name)", ""),
                Field::new("User", ""),
                Field::new("Port", "22"),
                Field::new("IdentityFile (key path, optional)", ""),
            ],
        }
    }

    /// The add-host wizard, pre-filled with a host's current settings.
    fn edit_host(h: &Host) -> Self {
        Self {
            title: format!("Edit host '{}'", h.alias),
            idx: 0,
            action: Action::EditHost { original: h.alias.clone() },
            fields: vec![
                Field::filled("Alias (what you type after ssh)", &h.alias),
                Field::filled("HostName (IP or DNS name)", h.hostname.as_deref().unwrap_or("")),
                Field::filled("User", h.user.as_deref().unwrap_or("")),
                Field::filled("Port", h.port.as_deref().unwrap_or("")),
                Field::filled("IdentityFile (key path, optional)", h.identity.as_deref().unwrap_or("")),
            ],
        }
    }

    fn new_key() -> Self {
        Self {
            title: "Generate a new ed25519 key".into(),
            idx: 0,
            action: Action::NewKey,
            fields: vec![
                Field::new("Key name (file in ~/.ssh)", "id_ed25519"),
                Field::new("Comment (your email)", ""),
            ],
        }
    }

    fn copy_key(key: PathBuf) -> Self {
        let name = key.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Self {
            title: format!("Copy '{name}' to a host (ssh-copy-id)"),
            idx: 0,
            action: Action::CopyKey { key },
            fields: vec![Field::new("Host (a config alias)", "")],
        }
    }

    fn mount(host: String) -> Self {
        Self {
            title: format!("Mount {host} locally (sshfs)"),
            idx: 0,
            action: Action::Mount { host: host.clone() },
            fields: vec![
                Field::new("Remote path", "~ (home)"),
                Field::new("Local mountpoint", &format!("./{host}")),
            ],
        }
    }

    fn forward(host: String) -> Self {
        Self {
            title: format!("Reach a service on {host} from your machine (-L)"),
            idx: 0,
            action: Action::Forward { host: host.clone() },
            fields: vec![
                Field::new(&format!("Remote port (the service on {host})"), ""),
                Field::new("Local port (where you'll reach it)", "= remote"),
            ],
        }
    }

    fn reverse(host: String) -> Self {
        Self {
            title: format!("Expose a local port on {host} (-R)"),
            idx: 0,
            action: Action::Reverse { host: host.clone() },
            fields: vec![
                Field::new("Local port (yours to expose)", ""),
                Field::new(&format!("Remote port (opened on {host})"), "= local"),
            ],
        }
    }
}

/// An external command the event loop must run *suspended* (outside the TUI) so
/// it can own the terminal - connect, keygen, copy-id, mount.
struct PendingRun {
    argv: Vec<String>,
    label: String,
}

/// A yes/no gate shown before a destructive, persistent action (deleting a host).
struct Confirm {
    message: String,
    action: ConfirmAction,
}

enum ConfirmAction {
    DeleteHost(String),
}

struct App {
    view: View,
    hosts: Vec<Host>,
    keys: Vec<keys::Key>,
    tunnels: Vec<tunnels::Tunnel>,
    mounts: Vec<mounts::Mount>,
    host_state: ListState,
    key_state: ListState,
    tunnel_state: ListState,
    mount_state: ListState,
    prompt: Option<Prompt>,
    confirm: Option<Confirm>,
    status: String,
    show_help: bool,
    should_quit: bool,
}

impl App {
    /// An app with no data loaded - the base for both `new()` and the tests
    /// (which set the lists directly, so they never touch the real ~/.ssh).
    fn empty() -> Self {
        App {
            view: View::Hosts,
            hosts: Vec::new(),
            keys: Vec::new(),
            tunnels: Vec::new(),
            mounts: Vec::new(),
            host_state: ListState::default(),
            key_state: ListState::default(),
            tunnel_state: ListState::default(),
            mount_state: ListState::default(),
            prompt: None,
            confirm: None,
            status: String::new(),
            show_help: false,
            should_quit: false,
        }
    }

    fn new() -> Self {
        let mut app = Self::empty();
        app.refresh_all();
        app
    }

    // --- data loading -----------------------------------------------------

    fn refresh_all(&mut self) {
        self.refresh_hosts();
        self.refresh_keys();
        self.refresh_tunnels();
        self.refresh_mounts();
    }

    fn refresh_hosts(&mut self) {
        self.hosts = sshcfg::list_hosts();
        Self::clamp(&mut self.host_state, self.hosts.len());
    }

    fn refresh_keys(&mut self) {
        self.keys = keys::list();
        Self::clamp(&mut self.key_state, self.keys.len());
    }

    fn refresh_tunnels(&mut self) {
        self.tunnels = tunnels::list();
        Self::clamp(&mut self.tunnel_state, self.tunnels.len());
    }

    fn refresh_mounts(&mut self) {
        self.mounts = mounts::list();
        Self::clamp(&mut self.mount_state, self.mounts.len());
    }

    /// Keep a list's selection in range (and set/clear it as the list fills/empties).
    fn clamp(state: &mut ListState, len: usize) {
        if len == 0 {
            state.select(None);
        } else {
            let sel = state.selected().unwrap_or(0).min(len - 1);
            state.select(Some(sel));
        }
    }

    fn selected_host(&self) -> Option<&Host> {
        self.host_state.selected().and_then(|i| self.hosts.get(i))
    }

    fn selected_key(&self) -> Option<&keys::Key> {
        self.key_state.selected().and_then(|i| self.keys.get(i))
    }

    fn selected_tunnel(&self) -> Option<&tunnels::Tunnel> {
        self.tunnel_state.selected().and_then(|i| self.tunnels.get(i))
    }

    fn selected_mount(&self) -> Option<&mounts::Mount> {
        self.mount_state.selected().and_then(|i| self.mounts.get(i))
    }

    // --- input ------------------------------------------------------------

    fn on_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        if self.show_help {
            if matches!(key.code, KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q')) {
                self.show_help = false;
            }
            return None;
        }
        if self.confirm.is_some() {
            return self.confirm_key(key);
        }
        if self.prompt.is_some() {
            return self.prompt_key(key);
        }
        self.nav_key(key)
    }

    /// Resolve a pending yes/no gate. Only `y`/`Enter` proceeds; anything else
    /// (including `n`/`Esc`) cancels.
    fn confirm_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        let proceed = matches!(key.code, KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter);
        let c = self.confirm.take()?;
        if !proceed {
            self.status = "cancelled".into();
            return None;
        }
        match c.action {
            ConfirmAction::DeleteHost(alias) => match sshcfg::delete_host(&alias) {
                Ok(_) => {
                    self.refresh_hosts();
                    self.status = format!("deleted '{alias}' (config backed up)");
                }
                Err(e) => self.status = format!("delete failed: {e}"),
            },
        }
        None
    }

    fn nav_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);

        // Ctrl-C always quits, even while a per-view letter (like `c` copy) is bound.
        if ctrl && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return None;
        }

        // Movement - arrows and vim keys are interchangeable, and the Ctrl-chord
        // variants navigate too (so the same fingers work everywhere). Views are
        // the horizontal axis (←→ / h l / Tab); the list is the vertical (↑↓ / k j).
        match key.code {
            KeyCode::Char('q') if !ctrl => {
                self.should_quit = true;
                return None;
            }
            KeyCode::Char('?') if !ctrl => {
                self.show_help = true;
                return None;
            }
            KeyCode::Tab | KeyCode::Right | KeyCode::Char('l') => {
                self.cycle_view(1);
                return None;
            }
            KeyCode::BackTab | KeyCode::Left | KeyCode::Char('h') => {
                self.cycle_view(-1);
                return None;
            }
            KeyCode::Down | KeyCode::Char('j') => {
                self.move_sel(1);
                return None;
            }
            KeyCode::Up | KeyCode::Char('k') => {
                self.move_sel(-1);
                return None;
            }
            _ => {}
        }

        // Any leftover Ctrl-chord is a navigation intent, never an action trigger.
        if ctrl {
            return None;
        }

        match self.view {
            View::Hosts => self.hosts_key(key),
            View::Keys => self.keys_key(key),
            View::Tunnels => self.tunnels_key(key),
            View::Mounts => self.mounts_key(key),
        }
    }

    fn hosts_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        match key.code {
            KeyCode::Enter => {
                let alias = self.selected_host()?.alias.clone();
                Some(PendingRun {
                    argv: vec!["ssh".into(), alias.clone()],
                    label: format!("session to {alias}"),
                })
            }
            // `n` = new/create, the same key in every view (the lazygit convention).
            KeyCode::Char('n') => {
                self.prompt = Some(Prompt::add_host());
                None
            }
            KeyCode::Char('e') => {
                let host = self.selected_host()?.clone();
                self.prompt = Some(Prompt::edit_host(&host));
                None
            }
            KeyCode::Char('d') => {
                let alias = self.selected_host()?.alias.clone();
                self.confirm = Some(Confirm {
                    message: format!("Delete host '{alias}' from ~/.ssh/config?"),
                    action: ConfirmAction::DeleteHost(alias),
                });
                None
            }
            // Clear a host's stale key from known_hosts (the "REMOTE HOST
            // IDENTIFICATION HAS CHANGED" fix). Non-interactive, so run it inline.
            KeyCode::Char('R') => {
                let target = {
                    let h = self.selected_host()?;
                    h.hostname.clone().unwrap_or_else(|| h.alias.clone())
                };
                self.status = match Command::new("ssh-keygen").arg("-R").arg(&target).output() {
                    Ok(o) if o.status.success() => format!("cleared known_hosts for {target}"),
                    Ok(o) => format!("ssh-keygen -R failed: {}", String::from_utf8_lossy(&o.stderr).trim()),
                    Err(e) => format!("could not run ssh-keygen: {e}"),
                };
                None
            }
            KeyCode::Char('m') => {
                let alias = self.selected_host()?.alias.clone();
                self.prompt = Some(Prompt::mount(alias));
                None
            }
            KeyCode::Char('t') => {
                let alias = self.selected_host()?.alias.clone();
                self.prompt = Some(Prompt::forward(alias));
                None
            }
            KeyCode::Char('T') => {
                let alias = self.selected_host()?.alias.clone();
                self.prompt = Some(Prompt::reverse(alias));
                None
            }
            _ => None,
        }
    }

    fn keys_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        match key.code {
            KeyCode::Char('n') => {
                self.prompt = Some(Prompt::new_key());
                None
            }
            KeyCode::Char('c') => {
                let path = self.selected_key()?.path.clone();
                self.prompt = Some(Prompt::copy_key(path));
                None
            }
            _ => None,
        }
    }

    fn tunnels_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        match key.code {
            KeyCode::Char('d') | KeyCode::Char('x') => {
                let pid = self.selected_tunnel()?.pid;
                let _ = tunnels::kill(pid);
                self.refresh_tunnels();
                self.status = format!("killed tunnel {pid}");
                None
            }
            KeyCode::Char('r') => {
                self.refresh_tunnels();
                self.status = "refreshed tunnels".into();
                None
            }
            _ => None,
        }
    }

    fn mounts_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        match key.code {
            KeyCode::Char('d') | KeyCode::Char('x') => {
                let local = self.selected_mount()?.local.clone();
                match mounts::unmount(&local) {
                    Ok(_) => {
                        self.refresh_mounts();
                        self.status = format!("unmounted {local}");
                    }
                    Err(e) => self.status = format!("unmount failed: {e}"),
                }
                None
            }
            KeyCode::Char('r') => {
                self.refresh_mounts();
                self.status = "refreshed mounts".into();
                None
            }
            _ => None,
        }
    }

    fn prompt_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let len = self.prompt.as_ref().map(|p| p.fields.len()).unwrap_or(0);
        if len == 0 {
            return None;
        }

        // Field navigation. Plain h/j/k/l are *typed* here (you're filling a text
        // field), so a vim user moves between fields with the Ctrl-chords or the
        // arrows/Tab - exactly the "submenu" rule. Ctrl-Down/Up also carry ctrl,
        // but their arm matches on the code so they land as next/prev too.
        let next = matches!(key.code, KeyCode::Tab | KeyCode::Down)
            || (ctrl && matches!(key.code, KeyCode::Char('j') | KeyCode::Char('l') | KeyCode::Right));
        let prev = matches!(key.code, KeyCode::BackTab | KeyCode::Up)
            || (ctrl && matches!(key.code, KeyCode::Char('k') | KeyCode::Left));

        if next {
            let p = self.prompt.as_mut().unwrap();
            p.idx = (p.idx + 1) % len;
            return None;
        }
        if prev {
            let p = self.prompt.as_mut().unwrap();
            p.idx = (p.idx + len - 1) % len;
            return None;
        }

        match key.code {
            KeyCode::Esc => {
                self.prompt = None;
                self.status = "cancelled".into();
            }
            // Ctrl-C bails out of the wizard rather than typing a literal 'c'.
            KeyCode::Char('c') if ctrl => {
                self.prompt = None;
                self.status = "cancelled".into();
            }
            KeyCode::Enter => {
                let idx = self.prompt.as_ref().unwrap().idx;
                if idx + 1 < len {
                    self.prompt.as_mut().unwrap().idx += 1;
                } else {
                    return self.submit_prompt();
                }
            }
            KeyCode::Backspace => {
                self.prompt.as_mut().unwrap().cur_mut().value.pop();
            }
            // Everything else with no Ctrl held is literal text - including h/j/k/l.
            KeyCode::Char(c) if !ctrl => {
                self.prompt.as_mut().unwrap().cur_mut().value.push(c);
            }
            _ => {}
        }
        None
    }

    /// Consume the active prompt and carry out its action. Returns a `PendingRun`
    /// for the interactive commands; mutates in place for the rest. On a
    /// validation error it puts the prompt back so the user can fix the field.
    fn submit_prompt(&mut self) -> Option<PendingRun> {
        let prompt = self.prompt.take()?;
        let v: Vec<String> = prompt.fields.iter().map(|f| f.value.trim().to_string()).collect();
        let action = prompt.action.clone();

        match action {
            Action::AddHost => {
                if v[0].is_empty() {
                    self.status = "add host: an alias is required".into();
                    self.prompt = Some(prompt);
                    return None;
                }
                let port = if v[3].is_empty() { "22".into() } else { v[3].clone() };
                let nh = NewHost {
                    alias: v[0].clone(),
                    hostname: v[1].clone(),
                    user: v[2].clone(),
                    port,
                    identity: v[4].clone(),
                };
                match sshcfg::add_host(&nh) {
                    Ok(_) => {
                        self.refresh_hosts();
                        self.status = format!("added host '{}' (config backed up)", v[0]);
                    }
                    Err(e) => self.status = format!("add host failed: {e}"),
                }
                None
            }

            Action::EditHost { original } => {
                if v[0].is_empty() {
                    self.status = "edit host: an alias is required".into();
                    self.prompt = Some(prompt);
                    return None;
                }
                let port = if v[3].is_empty() { "22".into() } else { v[3].clone() };
                let nh = NewHost {
                    alias: v[0].clone(),
                    hostname: v[1].clone(),
                    user: v[2].clone(),
                    port,
                    identity: v[4].clone(),
                };
                match sshcfg::update_host(&original, &nh) {
                    Ok(_) => {
                        self.refresh_hosts();
                        self.status = format!("updated host '{}' (config backed up)", v[0]);
                    }
                    Err(e) => self.status = format!("edit failed: {e}"),
                }
                None
            }

            Action::NewKey => {
                let name = if v[0].is_empty() { "id_ed25519".into() } else { v[0].clone() };
                let path = sshcfg::ssh_dir().join(name).to_string_lossy().into_owned();
                let mut argv = vec!["ssh-keygen".into(), "-t".into(), "ed25519".into(), "-f".into(), path];
                if !v[1].is_empty() {
                    argv.push("-C".into());
                    argv.push(v[1].clone());
                }
                Some(PendingRun { argv, label: "generate key".into() })
            }

            Action::CopyKey { key } => {
                if v[0].is_empty() {
                    self.status = "copy key: a host is required".into();
                    self.prompt = Some(prompt);
                    return None;
                }
                let host = v[0].clone();
                Some(PendingRun {
                    argv: vec![
                        "ssh-copy-id".into(),
                        "-i".into(),
                        key.to_string_lossy().into_owned(),
                        host.clone(),
                    ],
                    label: format!("copy key to {host}"),
                })
            }

            Action::Mount { host } => {
                // Blank remote path → sshfs mounts the login home directory.
                let remote = v[0].clone();
                let local = if v[1].is_empty() { format!("./{host}") } else { v[1].clone() };
                if let Err(e) = fs::create_dir_all(&local) {
                    self.status = format!("mount: cannot create {local}: {e}");
                    return None;
                }
                // Jump to the Mounts tab so the result (success or empty) is visible.
                self.view = View::Mounts;
                Some(PendingRun {
                    argv: vec!["sshfs".into(), format!("{host}:{remote}"), local.clone()],
                    label: format!("mount {host} → {local}"),
                })
            }

            Action::Forward { host } => {
                if v[0].is_empty() {
                    self.status = "forward: a remote port is required".into();
                    self.prompt = Some(prompt);
                    return None;
                }
                let remote = v[0].clone();
                let local = if v[1].is_empty() { remote.clone() } else { v[1].clone() };
                let spec = format!("{local}:localhost:{remote}");
                match tunnels::open('L', &spec, &host) {
                    Ok(t) => {
                        self.view = View::Tunnels;
                        self.refresh_tunnels();
                        self.status = format!("localhost:{local} → {host}:{remote}  (pid {})", t.pid);
                    }
                    Err(e) => self.status = format!("forward failed: {e}"),
                }
                None
            }

            Action::Reverse { host } => {
                if v[0].is_empty() {
                    self.status = "expose: a local port is required".into();
                    self.prompt = Some(prompt);
                    return None;
                }
                let local = v[0].clone();
                let remote = if v[1].is_empty() { local.clone() } else { v[1].clone() };
                let spec = format!("{remote}:localhost:{local}");
                match tunnels::open('R', &spec, &host) {
                    Ok(t) => {
                        self.view = View::Tunnels;
                        self.refresh_tunnels();
                        self.status = format!("{host}:{remote} → localhost:{local}  (pid {})", t.pid);
                    }
                    Err(e) => self.status = format!("expose failed: {e}"),
                }
                None
            }
        }
    }

    fn cycle_view(&mut self, delta: i32) {
        let i = VIEWS.iter().position(|v| *v == self.view).unwrap_or(0) as i32;
        let n = (i + delta).rem_euclid(VIEWS.len() as i32) as usize;
        self.view = VIEWS[n];
    }

    fn move_sel(&mut self, delta: i32) {
        match self.view {
            View::Hosts => Self::move_state(&mut self.host_state, self.hosts.len(), delta),
            View::Keys => Self::move_state(&mut self.key_state, self.keys.len(), delta),
            View::Tunnels => Self::move_state(&mut self.tunnel_state, self.tunnels.len(), delta),
            View::Mounts => Self::move_state(&mut self.mount_state, self.mounts.len(), delta),
        }
    }

    fn move_state(state: &mut ListState, len: usize, delta: i32) {
        if len == 0 {
            return;
        }
        let cur = state.selected().unwrap_or(0) as i32;
        let next = (cur + delta).rem_euclid(len as i32) as usize;
        state.select(Some(next));
    }
}

// ==========================================================================
// Rendering
// ==========================================================================

fn ui(f: &mut Frame, app: &mut App) {
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(3), Constraint::Min(1), Constraint::Length(1)])
        .split(area);

    render_tabs(f, chunks[0], app);
    render_body(f, chunks[1], app);
    render_status(f, chunks[2], app);

    if app.show_help {
        render_help(f, area);
    }
    if let Some(p) = &app.prompt {
        render_prompt(f, area, p);
    }
    if let Some(c) = &app.confirm {
        render_confirm(f, area, c);
    }
}

fn render_confirm(f: &mut Frame, area: Rect, c: &Confirm) {
    let rect = centered(area, 60, 6);
    f.render_widget(Clear, rect);
    let lines = vec![
        Line::raw(""),
        Line::from(Span::raw(format!("  {}", c.message))),
        Line::raw(""),
        Line::from(Span::styled(
            "  y confirm      n / Esc cancel",
            Style::default().add_modifier(Modifier::DIM),
        )),
    ];
    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" confirm ")
            .border_style(Style::default().fg(Color::Red)),
    );
    f.render_widget(para, rect);
}

fn render_tabs(f: &mut Frame, area: Rect, app: &App) {
    let idx = VIEWS.iter().position(|v| *v == app.view).unwrap_or(0);
    let tabs = Tabs::new(vec!["Hosts", "Keys", "Tunnels", "Mounts (sshfs)"])
        .select(idx)
        .block(Block::default().borders(Borders::ALL).title(" easyssh · essh "))
        .divider("│")
        .highlight_style(Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD));
    f.render_widget(tabs, area);
}

fn render_body(f: &mut Frame, area: Rect, app: &mut App) {
    let dim = Style::default().add_modifier(Modifier::DIM);
    let bold = Style::default().add_modifier(Modifier::BOLD);
    let sel = Style::default().add_modifier(Modifier::REVERSED);

    match app.view {
        View::Hosts => {
            if app.hosts.is_empty() {
                empty(f, area, "Hosts", "No hosts in ~/.ssh/config yet.\nPress `n` to add one.");
                return;
            }
            let w = app.hosts.iter().map(|h| h.alias.len()).max().unwrap_or(0);
            let items: Vec<ListItem> = app
                .hosts
                .iter()
                .map(|h| {
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:w$}", h.alias), bold),
                        Span::raw("  "),
                        Span::styled(h.target(), dim),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(titled("Hosts", app.hosts.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.host_state);
        }

        View::Keys => {
            if app.keys.is_empty() {
                empty(f, area, "Keys", "No keypairs in ~/.ssh.\nPress `n` to generate an ed25519 key.");
                return;
            }
            let nw = app.keys.iter().map(|k| k.name().len()).max().unwrap_or(0);
            let items: Vec<ListItem> = app
                .keys
                .iter()
                .map(|k| {
                    ListItem::new(Line::from(vec![
                        Span::styled(format!("{:nw$}", k.name()), bold),
                        Span::raw("  "),
                        Span::styled(format!("{:7}", k.kind), Style::default().fg(Color::Cyan)),
                        Span::raw("  "),
                        Span::styled(k.fingerprint.clone(), dim),
                        Span::raw("  "),
                        Span::styled(k.comment.clone(), dim),
                    ]))
                })
                .collect();
            let list = List::new(items)
                .block(titled("Keys", app.keys.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.key_state);
        }

        View::Tunnels => {
            if app.tunnels.is_empty() {
                empty(
                    f,
                    area,
                    "Tunnels",
                    "No active tunnels.\nOpen one from the Hosts tab: `t` (forward) or `T` (expose).",
                );
                return;
            }
            let items: Vec<ListItem> = app
                .tunnels
                .iter()
                .map(|t| ListItem::new(Span::raw(t.describe())))
                .collect();
            let list = List::new(items)
                .block(titled("Tunnels", app.tunnels.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.tunnel_state);
        }

        View::Mounts => {
            if app.mounts.is_empty() {
                empty(f, area, "Mounts", "No sshfs mounts.\nMount one from the Hosts tab with `m`.");
                return;
            }
            let items: Vec<ListItem> = app
                .mounts
                .iter()
                .map(|m| ListItem::new(Span::raw(m.describe())))
                .collect();
            let list = List::new(items)
                .block(titled("Mounts", app.mounts.len()))
                .highlight_style(sel)
                .highlight_symbol("▸ ");
            f.render_stateful_widget(list, area, &mut app.mount_state);
        }
    }
}

fn render_status(f: &mut Frame, area: Rect, app: &App) {
    // Show the last action's result if there is one; otherwise the key hints.
    let text = if app.status.is_empty() {
        match app.view {
            View::Hosts => HOSTS_HINTS,
            View::Keys => KEYS_HINTS,
            View::Tunnels => TUNNELS_HINTS,
            View::Mounts => MOUNTS_HINTS,
        }
    } else {
        &app.status
    };
    let style = if app.status.is_empty() {
        Style::default().add_modifier(Modifier::DIM)
    } else {
        Style::default().fg(Color::Green)
    };
    f.render_widget(Paragraph::new(Line::from(Span::styled(format!(" {text}"), style))), area);
}

fn render_prompt(f: &mut Frame, area: Rect, p: &Prompt) {
    let height = p.fields.len() as u16 + 6;
    let rect = centered(area, 72, height);
    f.render_widget(Clear, rect);

    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (i, field) in p.fields.iter().enumerate() {
        let active = i == p.idx;
        let head = if field.default.is_empty() {
            format!("{}: ", field.label)
        } else {
            format!("{} [{}]: ", field.label, field.default)
        };
        let label_style = if active {
            Style::default().fg(Color::Cyan).add_modifier(Modifier::BOLD)
        } else {
            Style::default().add_modifier(Modifier::DIM)
        };
        let cursor = if active { "█" } else { "" };
        lines.push(Line::from(vec![
            Span::raw(if active { "▸ " } else { "  " }),
            Span::styled(head, label_style),
            Span::raw(format!("{}{}", field.value, cursor)),
        ]));
    }
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "  Enter next/submit   Ctrl-j/k · Ctrl-↑↓ · Tab move field   Esc cancel",
        Style::default().add_modifier(Modifier::DIM),
    )));

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

fn render_help(f: &mut Frame, area: Rect) {
    let rect = centered(area, 76, 21);
    f.render_widget(Clear, rect);
    let dim = Style::default().add_modifier(Modifier::DIM);
    let lines = vec![
        Line::from(Span::styled("easyssh - one tool for ssh, keys, tunnels, mounts", Style::default().add_modifier(Modifier::BOLD))),
        Line::raw(""),
        Line::raw("Move      j/k or ↑↓ list     h/l or ←→ switch view     (Ctrl+ works too)"),
        Line::raw("Global    ? help      q or Ctrl-c quit"),
        Line::raw(""),
        Line::raw("Hosts     ↵ connect   n new   e edit   d delete   m mount (sshfs)"),
        Line::raw("          t forward a remote port to you   T expose a local port"),
        Line::raw("          R clear this host from known_hosts (stale-key fix)"),
        Line::raw(""),
        Line::raw("Keys      n new ed25519 key      c copy key to a host"),
        Line::raw("Tunnels   d kill      r refresh"),
        Line::raw("Mounts    d unmount   r refresh"),
        Line::raw(""),
        Line::raw("In a form  type to fill (h/j/k/l are text!)"),
        Line::raw("           Ctrl-j/k · Ctrl-↑↓ · Tab move between fields   Esc cancel"),
        Line::raw(""),
        Line::from(Span::styled("press ? or Esc to close", dim)),
    ];
    let para = Paragraph::new(lines).block(
        Block::default()
            .borders(Borders::ALL)
            .title(" help ")
            .border_style(Style::default().fg(Color::Cyan)),
    );
    f.render_widget(para, rect);
}

/// A bordered block titled `Name (count)`.
fn titled(name: &str, count: usize) -> Block<'static> {
    Block::default().borders(Borders::ALL).title(format!(" {name} ({count}) "))
}

/// Render a friendly empty-state message inside the body block.
fn empty(f: &mut Frame, area: Rect, name: &str, msg: &str) {
    let para = Paragraph::new(msg)
        .block(titled(name, 0))
        .style(Style::default().add_modifier(Modifier::DIM))
        .wrap(Wrap { trim: false });
    f.render_widget(para, area);
}

/// A rect centered in `area`, `pct_x`% wide and `height` rows tall.
fn centered(area: Rect, pct_x: u16, height: u16) -> Rect {
    let w = area.width * pct_x / 100;
    let h = height.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    }
}

// ==========================================================================
// Terminal lifecycle + event loop
// ==========================================================================

/// Entry point for bare `essh`.
pub fn run() -> Result<()> {
    install_panic_hook();
    let mut terminal = setup()?;
    let mut app = App::new();
    let res = event_loop(&mut terminal, &mut app);
    teardown(&mut terminal)?;
    res
}

fn event_loop(terminal: &mut Term, app: &mut App) -> Result<()> {
    while !app.should_quit {
        terminal.draw(|f| ui(f, app))?;

        let Event::Key(key) = event::read()? else {
            continue;
        };
        // crossterm emits Press+Release on some platforms; act on Press only.
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if let Some(run) = app.on_key(key) {
            let status = run_suspended(terminal, &run.argv)?;
            app.status = match status {
                Some(s) if s.success() => format!("{} ✓", run.label),
                Some(s) => format!("{} failed (exit {})", run.label, s.code().unwrap_or(-1)),
                None => format!("{}: could not run '{}'", run.label, run.argv[0]),
            };
            app.refresh_all();
        }
    }
    Ok(())
}

/// Leave the TUI, run an interactive command with the real terminal, then
/// restore the TUI. This is what lets ssh/ssh-keygen/ssh-copy-id/sshfs prompt.
fn run_suspended(terminal: &mut Term, argv: &[String]) -> Result<Option<ExitStatus>> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    // ratatui hides the cursor while drawing and leaving the alt-screen does not
    // restore it, so without this the child (ssh, keygen, copy-id, sshfs) runs
    // with an invisible cursor and you cannot see where you are typing.
    terminal.show_cursor()?;

    let status = Command::new(&argv[0]).args(&argv[1..]).status().ok();

    enable_raw_mode()?;
    execute!(terminal.backend_mut(), EnterAlternateScreen)?;
    terminal.hide_cursor()?;
    terminal.clear()?;
    Ok(status)
}

fn setup() -> Result<Term> {
    enable_raw_mode()?;
    let mut stdout = io::stdout();
    execute!(stdout, EnterAlternateScreen)?;
    Ok(Terminal::new(CrosstermBackend::new(stdout))?)
}

fn teardown(terminal: &mut Term) -> Result<()> {
    disable_raw_mode()?;
    execute!(terminal.backend_mut(), LeaveAlternateScreen)?;
    terminal.show_cursor()?;
    Ok(())
}

/// Make sure a panic doesn't leave the user's terminal in raw/alt-screen mode.
fn install_panic_hook() {
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let _ = disable_raw_mode();
        let _ = execute!(io::stdout(), LeaveAlternateScreen);
        hook(info);
    }));
}

#[cfg(test)]
mod tests {
    use super::*;
    use ratatui::backend::TestBackend;

    /// Render each view + a wizard against a headless backend and dump the
    /// buffer to a string, so we can assert the layout draws real content
    /// without needing an actual terminal.
    fn render(app: &mut App) -> String {
        let mut terminal = Terminal::new(TestBackend::new(120, 30)).unwrap();
        terminal.draw(|f| ui(f, app)).unwrap();
        let buf = terminal.backend().buffer().clone();
        buf.content().iter().map(|c| c.symbol()).collect()
    }

    #[test]
    fn renders_chrome_and_wizards() {
        let mut app = App::empty();

        // Tab chrome is always present.
        let hosts = render(&mut app);
        assert!(hosts.contains("easyssh"), "title bar missing");
        assert!(hosts.contains("Hosts") && hosts.contains("Mounts"), "tabs missing");

        // The add-host wizard overlays its fields.
        app.prompt = Some(Prompt::add_host());
        let add = render(&mut app);
        assert!(add.contains("Add host"), "add-host title missing");
        assert!(add.contains("Alias"), "alias field missing");
        app.prompt = None;

        // The forward wizard builds off a host name.
        app.prompt = Some(Prompt::forward("box".into()));
        let fwd = render(&mut app);
        assert!(fwd.contains("Remote port"), "forward field missing");

        // Switching to the Tunnels view renders its (possibly empty) body.
        app.view = View::Tunnels;
        app.prompt = None;
        let tun = render(&mut app);
        assert!(tun.contains("Tunnels"), "tunnels view missing");
    }

    #[test]
    fn tab_cycles_views() {
        let mut app = App::empty();
        assert!(matches!(app.view, View::Hosts));
        app.cycle_view(1);
        assert!(matches!(app.view, View::Keys));
        app.cycle_view(-1);
        assert!(matches!(app.view, View::Hosts));
    }

    fn press(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::NONE)
    }
    fn ctrl(code: KeyCode) -> KeyEvent {
        KeyEvent::new(code, KeyModifiers::CONTROL)
    }

    #[test]
    fn vim_keys_switch_views() {
        let mut app = App::empty();
        app.on_key(press(KeyCode::Char('l'))); // → next view
        assert!(matches!(app.view, View::Keys));
        app.on_key(press(KeyCode::Char('h'))); // ← prev view
        assert!(matches!(app.view, View::Hosts));
    }

    #[test]
    fn n_creates_in_every_view() {
        let mut app = App::empty();
        // Hosts: `n` opens the add-host wizard.
        app.on_key(press(KeyCode::Char('n')));
        assert!(matches!(app.prompt.as_ref().map(|p| &p.action), Some(Action::AddHost)));
        app.prompt = None;

        // Keys: the same `n` opens the new-key wizard.
        app.view = View::Keys;
        app.on_key(press(KeyCode::Char('n')));
        assert!(matches!(app.prompt.as_ref().map(|p| &p.action), Some(Action::NewKey)));
    }

    #[test]
    fn form_typing_vs_field_nav() {
        let mut app = App::empty();
        app.prompt = Some(Prompt::add_host()); // 5 fields, idx 0

        // Plain h/j/k/l are literal text in a form.
        for c in ['h', 'j', 'k', 'l'] {
            app.on_key(press(KeyCode::Char(c)));
        }
        assert_eq!(app.prompt.as_ref().unwrap().fields[0].value, "hjkl");
        assert_eq!(app.prompt.as_ref().unwrap().idx, 0, "typing must not move fields");

        // Ctrl-j / Ctrl-k move between fields.
        app.on_key(ctrl(KeyCode::Char('j')));
        assert_eq!(app.prompt.as_ref().unwrap().idx, 1);
        app.on_key(ctrl(KeyCode::Char('j')));
        assert_eq!(app.prompt.as_ref().unwrap().idx, 2);
        app.on_key(ctrl(KeyCode::Char('k')));
        assert_eq!(app.prompt.as_ref().unwrap().idx, 1);

        // Ctrl-c bails out of the wizard.
        app.on_key(ctrl(KeyCode::Char('c')));
        assert!(app.prompt.is_none());
    }

    /// An app with one selected host, built without touching disk.
    fn app_with_host() -> App {
        let mut app = App::empty();
        app.hosts = vec![Host {
            alias: "raspi".into(),
            hostname: Some("10.0.0.1".into()),
            user: Some("pi".into()),
            port: None,
            identity: None,
        }];
        app.host_state.select(Some(0));
        app
    }

    #[test]
    fn e_opens_edit_prefilled() {
        let mut app = app_with_host();
        app.on_key(press(KeyCode::Char('e')));
        let p = app.prompt.as_ref().expect("edit wizard should open");
        assert!(matches!(p.action, Action::EditHost { .. }));
        assert_eq!(p.fields[0].value, "raspi", "alias pre-filled");
        assert_eq!(p.fields[1].value, "10.0.0.1", "hostname pre-filled");
    }

    #[test]
    fn d_gates_delete_behind_confirm() {
        let mut app = app_with_host();
        app.on_key(press(KeyCode::Char('d')));
        assert!(app.confirm.is_some(), "delete must ask first");
        app.on_key(press(KeyCode::Char('n'))); // decline
        assert!(app.confirm.is_none(), "declining clears the gate");
        // The host is still there (we never confirmed).
        assert_eq!(app.hosts.len(), 1);
    }

    #[test]
    fn mounts_tab_renders() {
        let mut app = App::empty();
        app.view = View::Mounts;
        assert!(render(&mut app).contains("Mounts"));
    }
}
