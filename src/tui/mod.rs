//! The toolbox - bare `essh`. Four tabs (Hosts / Keys / Tunnels / Mounts) and a
//! set of wizards that stand in for the commands nobody remembers: `ssh-keygen`,
//! `ssh-copy-id`, `ssh -L/-R`, `sshfs`, and hand-editing `~/.ssh/config`.
//!
//! Anything interactive (connecting, generating a key, copying a key, mounting)
//! runs *suspended*: we drop out of the alternate screen, hand the real terminal
//! to the child process so it can prompt for passwords/passphrases, then restore
//! the UI. Tunnels are spawned detached and tracked, so they don't need that.

use crate::history;
use crate::keys::{self};
use crate::mounts;
use crate::reach::{self, Reach};
use crate::settings::{self, HostOrder, Settings};
use crate::sshcfg::{self, Host};
use crate::tunnels;
use anyhow::Result;
use crossterm::{
    event::{self, Event, KeyEventKind},
    execute,
    terminal::{EnterAlternateScreen, LeaveAlternateScreen, disable_raw_mode, enable_raw_mode},
};
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::widgets::{ListState, Padding};
use std::collections::HashMap;
use std::fs;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::time::{Duration, Instant};

type Term = Terminal<CrosstermBackend<Stdout>>;

/// The tabs. Order here is the left-to-right / Tab-cycle order.
mod alert;
mod confirm;
mod connect;
mod detail;
mod filter;
mod input;
mod mount_spec;
mod picker;
mod prompt;
mod render;
#[cfg(test)]
mod tests;
mod widgets;
mod wizard;

use confirm::Confirm;
use connect::changed_host_key;
use detail::render_detail;
use mount_spec::{MountSpec, shell_join};
use picker::Picker;
use prompt::{Action, Field, Kind, Prompt, render_prompt};
use render::ui;
use widgets::{
    box_area, box_block, box_buttons, box_height, box_hint, box_inner_width, box_width, empty,
    titled, wrapped_line_count,
};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum View {
    Hosts,
    Keys,
    Tunnels,
    Mounts,
    Settings,
}

const VIEWS: [View; 5] = [
    View::Hosts,
    View::Keys,
    View::Tunnels,
    View::Mounts,
    View::Settings,
];

// The bottom bar is a terse reminder of this view's actions only; `?` opens the
// full cheat-sheet (navigation keys and the real command behind each action), so
// the bar stays short instead of restating everything and overflowing.
const HOSTS_HINTS: &str =
    "↵ connect · c new · e edit · d del · m mount · t/T tunnel · r refresh · / find · ? help";
const KEYS_HINTS: &str = "c new · y copy pubkey · Y install on host · r refresh · / find · ? help";
const TUNNELS_HINTS: &str = "d kill · r refresh · / find · ? help";
const MOUNTS_HINTS: &str = "d unmount · r refresh · / find · ? help";
const SETTINGS_HINTS: &str = "↵ change · d back to default · r reload · ? help";

/// How long a status message stays on screen before the hints return.
const STATUS_TTL: Duration = Duration::from_millis(1500);

/// An external command the event loop must run *suspended* (outside the TUI) so
/// it can own the terminal - connect, keygen, copy-id, mount.
pub(super) struct PendingRun {
    pub(super) argv: Vec<String>,
    pub(super) label: String,
    /// The alias, when this run is an interactive login. Set here rather than
    /// sniffed from `argv[0]`, which stopped being `ssh` the moment Settings
    /// could point at a wrapper.
    pub(super) connect: Option<String>,
}

pub(super) struct App {
    pub(super) view: View,
    pub(super) hosts: Vec<Host>,
    pub(super) keys: Vec<keys::Key>,
    pub(super) tunnels: Vec<tunnels::Tunnel>,
    pub(super) mounts: Vec<mounts::Mount>,
    pub(super) host_state: ListState,
    pub(super) key_state: ListState,
    pub(super) tunnel_state: ListState,
    pub(super) mount_state: ListState,
    pub(super) prompt: Option<Prompt>,
    pub(super) picker: Option<Picker>,
    pub(super) confirm: Option<Confirm>,
    /// A failure that has to be read. It owns every key until dismissed.
    pub(super) alert: Option<alert::Alert>,
    pub(super) status: String,
    /// When `status` was set; it stops showing after `STATUS_TTL` so an old
    /// message never sits there looking like it is still current.
    pub(super) status_at: Option<Instant>,
    pub(super) show_help: bool,
    pub(super) should_quit: bool,
    /// What `/` is filtering the current list by. Empty means "show all".
    pub(super) query: String,
    /// True while the query is being typed, so keys go into it instead of
    /// triggering actions.
    pub(super) searching: bool,
    /// When you last connected to each alias, and how often.
    pub(super) history: history::History,
    /// Last known state of each host's ssh port, keyed by alias.
    pub(super) reach: HashMap<String, Reach>,
    /// Which probe round we are showing; answers from an older one are dropped.
    pub(super) reach_gen: u64,
    pub(super) reach_tx: Sender<reach::Msg>,
    pub(super) reach_rx: Receiver<reach::Msg>,
    /// A mount we just spawned, waiting to be selected once `/proc/mounts` shows
    /// it. The mount itself runs suspended, so it does not exist yet when the
    /// wizard hands the command over.
    pub(super) new_mount: Option<String>,
    /// The choices that are yours rather than ssh's.
    pub(super) settings: Settings,
    pub(super) settings_state: ListState,
}

impl App {
    /// An app with no data loaded - the base for both `new()` and the tests
    /// (which set the lists directly, so they never touch the real ~/.ssh).
    pub(super) fn empty() -> Self {
        let (tx, rx) = channel();
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
            picker: None,
            confirm: None,
            alert: None,
            status: String::new(),
            status_at: None,
            show_help: false,
            should_quit: false,
            query: String::new(),
            searching: false,
            history: history::History::default(),
            reach: HashMap::new(),
            reach_gen: 0,
            reach_tx: tx,
            reach_rx: rx,
            new_mount: None,
            settings: Settings::default(),
            settings_state: ListState::default(),
        }
    }

    /// Set the transient status line. It fades on its own after `STATUS_TTL`.
    pub(super) fn set_status(&mut self, msg: impl Into<String>) {
        self.status = msg.into();
        self.status_at = Some(Instant::now());
    }

    /// The status message while it is still fresh; `None` once it has expired.
    pub(super) fn live_status(&self) -> Option<&str> {
        let at = self.status_at?;
        (at.elapsed() < STATUS_TTL && !self.status.is_empty()).then_some(self.status.as_str())
    }

    pub(super) fn new() -> Self {
        let mut app = Self::empty();
        app.settings = settings::load();
        app.history = history::load();
        app.refresh_all();
        app.start_probes();
        app
    }

    // --- data loading -----------------------------------------------------

    pub(super) fn refresh_all(&mut self) {
        self.refresh_hosts();
        self.refresh_keys();
        self.refresh_tunnels();
        self.refresh_mounts();
    }

    pub(super) fn refresh_hosts(&mut self) {
        self.hosts = sshcfg::list_hosts();
        self.sort_hosts();
        let n = self.host_rows().len();
        Self::clamp(&mut self.host_state, n);
    }

    /// Most recently connected first, then everything you have never opened,
    /// alphabetically. The box you were on ten minutes ago is the one you are
    /// most likely reaching for, and it should not be a scroll away - unless you
    /// asked for plain alphabetical in Settings.
    pub(super) fn sort_hosts(&mut self) {
        match self.settings.host_order {
            HostOrder::Recent => {
                let history = &self.history;
                self.hosts.sort_by_key(|h| history.rank(&h.alias));
            }
            HostOrder::Alpha => self.hosts.sort_by(|a, b| a.alias.cmp(&b.alias)),
        }
    }

    pub(super) fn refresh_keys(&mut self) {
        self.keys = keys::list();
        // Which hosts pin each key: only we know both halves.
        keys::link_hosts(&mut self.keys, &self.hosts);
        let n = self.key_rows().len();
        Self::clamp(&mut self.key_state, n);
    }

    pub(super) fn refresh_tunnels(&mut self) {
        self.tunnels = tunnels::list();
        let n = self.tunnel_rows().len();
        Self::clamp(&mut self.tunnel_state, n);
    }

    pub(super) fn refresh_mounts(&mut self) {
        self.mounts = mounts::list();
        let n = self.mount_rows().len();
        Self::clamp(&mut self.mount_state, n);
    }

    // --- reachability -----------------------------------------------------

    /// Start a fresh round of port probes for every host. Older rounds are
    /// invalidated by the generation bump, so a slow answer from the last round
    /// cannot repaint a host we have since re-probed.
    pub(super) fn start_probes(&mut self) {
        self.reach_gen += 1;
        // Turned off in Settings: forget what we knew rather than leave dots
        // that stop being true the moment anything moves.
        if !self.settings.probe {
            self.reach.clear();
            return;
        }
        let targets: Vec<reach::Target> = self
            .hosts
            .iter()
            .map(|h| reach::Target {
                alias: h.alias.clone(),
                host: h.hostname.clone().unwrap_or_else(|| h.alias.clone()),
                // A config without a Port means ssh's default, which is 22.
                port: h.port.as_deref().and_then(|p| p.parse().ok()).unwrap_or(22),
            })
            .collect();
        for t in &targets {
            self.reach.insert(t.alias.clone(), Reach::Probing);
        }
        reach::probe_all(
            targets,
            self.reach_gen,
            self.reach_tx.clone(),
            self.settings.probe_timeout,
        );
    }

    /// Take whatever the probe threads have reported since the last frame.
    pub(super) fn drain_probes(&mut self) -> bool {
        let mut changed = false;
        while let Ok(msg) = self.reach_rx.try_recv() {
            if msg.generation != self.reach_gen {
                continue; // an answer to a question we no longer ask
            }
            self.reach.insert(msg.alias, msg.reach);
            changed = true;
        }
        changed
    }

    /// True while any probe is still outstanding, so the event loop knows to
    /// keep waking up instead of blocking on a keypress.
    pub(super) fn probing(&self) -> bool {
        self.reach.values().any(|r| *r == Reach::Probing)
    }

    // --- filtering --------------------------------------------------------

    /// Indices into `hosts` that survive the current query.
    pub(super) fn host_rows(&self) -> Vec<usize> {
        let target = |h: &Host| h.target();
        self.hosts
            .iter()
            .enumerate()
            .filter(|(_, h)| {
                let target = target(h);
                filter::matches(
                    &self.query,
                    &[
                        &h.alias,
                        &target,
                        h.identity.as_deref().unwrap_or(""),
                        h.proxy_jump.as_deref().unwrap_or(""),
                    ],
                )
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub(super) fn key_rows(&self) -> Vec<usize> {
        self.keys
            .iter()
            .enumerate()
            .filter(|(_, k)| {
                let name = k.name();
                let used = k.used_by.join(" ");
                filter::matches(&self.query, &[&name, &k.kind, &k.comment, &used])
            })
            .map(|(i, _)| i)
            .collect()
    }

    pub(super) fn tunnel_rows(&self) -> Vec<usize> {
        self.tunnels
            .iter()
            .enumerate()
            .filter(|(_, t)| filter::matches(&self.query, &[&t.describe()]))
            .map(|(i, _)| i)
            .collect()
    }

    pub(super) fn mount_rows(&self) -> Vec<usize> {
        self.mounts
            .iter()
            .enumerate()
            .filter(|(_, m)| filter::matches(&self.query, &[&m.describe()]))
            .map(|(i, _)| i)
            .collect()
    }

    /// The settings the current query keeps. Every one is always relevant, so
    /// this filters the same way the other tabs do rather than specially.
    pub(super) fn settings_rows(&self) -> Vec<settings::Row> {
        self.settings
            .rows()
            .into_iter()
            .filter(|r| filter::matches(&self.query, &[r.label, r.key, &r.value, r.help]))
            .collect()
    }

    pub(super) fn selected_setting(&self) -> Option<settings::Row> {
        let i = self.settings_state.selected()?;
        self.settings_rows().into_iter().nth(i)
    }

    /// How many rows the current view is showing, after filtering.
    pub(super) fn row_count(&self) -> usize {
        match self.view {
            View::Hosts => self.host_rows().len(),
            View::Keys => self.key_rows().len(),
            View::Tunnels => self.tunnel_rows().len(),
            View::Mounts => self.mount_rows().len(),
            View::Settings => self.settings_rows().len(),
        }
    }

    /// Jump to another view the way a finished action does. The filter belongs
    /// to the list it was typed over, so it does not come along - and a mount
    /// made from a filtered Hosts list would otherwise land on a Mounts tab that
    /// hides it.
    pub(super) fn goto_view(&mut self, view: View) {
        self.view = view;
        if !self.query.is_empty() || self.searching {
            self.query.clear();
            self.searching = false;
            self.requery();
        }
    }

    /// Put the cursor on the tunnel with this pid: the one you just made is the
    /// one you are looking for, and on a busy list it is not row one.
    pub(super) fn select_tunnel(&mut self, pid: u32) {
        let at = self
            .tunnel_rows()
            .iter()
            .position(|&r| self.tunnels[r].pid == pid);
        if let Some(i) = at {
            self.tunnel_state.select(Some(i));
        }
    }

    /// The same for a mountpoint, once the kernel admits it exists.
    pub(super) fn select_mount(&mut self, local: &str) {
        let at = self
            .mount_rows()
            .iter()
            .position(|&r| self.mounts[r].local == local);
        if let Some(i) = at {
            self.mount_state.select(Some(i));
        }
    }

    /// Select the mount the last suspended `sshfs` was asked to make, now that
    /// the lists have been reloaded. Does nothing if it never appeared, which is
    /// what a failed mount looks like.
    pub(super) fn settle_new_mount(&mut self) {
        if let Some(local) = self.new_mount.take() {
            self.select_mount(&local);
        }
    }

    /// Re-clamp every list after the query changed, and put the selection back
    /// at the top so the first match is the one under the cursor.
    pub(super) fn requery(&mut self) {
        self.host_state.select(Some(0));
        self.key_state.select(Some(0));
        self.tunnel_state.select(Some(0));
        self.mount_state.select(Some(0));
        self.settings_state.select(Some(0));
        let n = self.host_rows().len();
        Self::clamp(&mut self.host_state, n);
        let n = self.key_rows().len();
        Self::clamp(&mut self.key_state, n);
        let n = self.tunnel_rows().len();
        Self::clamp(&mut self.tunnel_state, n);
        let n = self.mount_rows().len();
        Self::clamp(&mut self.mount_state, n);
        let n = self.settings_rows().len();
        Self::clamp(&mut self.settings_state, n);
    }

    /// Keep a list's selection in range (and set/clear it as the list fills/empties).
    pub(super) fn clamp(state: &mut ListState, len: usize) {
        if len == 0 {
            state.select(None);
        } else {
            let sel = state.selected().unwrap_or(0).min(len - 1);
            state.select(Some(sel));
        }
    }

    // A selection indexes the *filtered* rows, so every lookup goes through the
    // row list; indexing the raw vec would pick the wrong entry the moment `/`
    // hides anything.
    pub(super) fn selected_host(&self) -> Option<&Host> {
        let row = *self.host_rows().get(self.host_state.selected()?)?;
        self.hosts.get(row)
    }

    pub(super) fn selected_key(&self) -> Option<&keys::Key> {
        let row = *self.key_rows().get(self.key_state.selected()?)?;
        self.keys.get(row)
    }

    pub(super) fn selected_tunnel(&self) -> Option<&tunnels::Tunnel> {
        let row = *self.tunnel_rows().get(self.tunnel_state.selected()?)?;
        self.tunnels.get(row)
    }

    pub(super) fn selected_mount(&self) -> Option<&mounts::Mount> {
        let row = *self.mount_rows().get(self.mount_state.selected()?)?;
        self.mounts.get(row)
    }

    // --- input ------------------------------------------------------------

    pub(super) fn cycle_view(&mut self, delta: i32) {
        let i = VIEWS.iter().position(|v| *v == self.view).unwrap_or(0) as i32;
        let n = (i + delta).rem_euclid(VIEWS.len() as i32) as usize;
        self.view = VIEWS[n];
        // A `/` filter belongs to the list you typed it over; carrying it into
        // the next tab would silently hide rows you never searched.
        if !self.query.is_empty() || self.searching {
            self.query.clear();
            self.searching = false;
            self.requery();
        }
    }

    pub(super) fn move_sel(&mut self, delta: i32) {
        let len = self.row_count();
        match self.view {
            View::Hosts => Self::move_state(&mut self.host_state, len, delta),
            View::Keys => Self::move_state(&mut self.key_state, len, delta),
            View::Tunnels => Self::move_state(&mut self.tunnel_state, len, delta),
            View::Mounts => Self::move_state(&mut self.mount_state, len, delta),
            View::Settings => Self::move_state(&mut self.settings_state, len, delta),
        }
    }

    pub(super) fn move_state(state: &mut ListState, len: usize, delta: i32) {
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

        // While a status message is showing, or port probes are still landing,
        // we wake up periodically so the screen can catch up on its own;
        // otherwise block until the user actually presses a key, so an idle TUI
        // costs nothing.
        let timeout = if app.live_status().is_some() || app.probing() {
            Duration::from_millis(200)
        } else {
            Duration::from_secs(3600)
        };
        if !event::poll(timeout)? {
            app.drain_probes();
            continue; // nothing pressed: redraw so the stale status drops off
        }
        app.drain_probes();

        let Event::Key(key) = event::read()? else {
            continue;
        };
        // crossterm emits Press+Release on some platforms; act on Press only.
        if key.kind != KeyEventKind::Press {
            continue;
        }

        if let Some(run) = app.on_key(key) {
            let status = run_suspended(terminal, &run.argv)?;

            // A connect that fails with 255 may be a changed host key, which ssh
            // prints then wipes when we redraw. Probe non-interactively and, if so,
            // offer the fix in a modal instead of a status that flashes past.
            let failed_255 = matches!(status, Some(ref s) if s.code() == Some(255));
            if let Some(host) = run.connect.clone() {
                if failed_255 {
                    if let Some(target) = changed_host_key(&host) {
                        app.offer_known_hosts_fix(&host, target);
                    }
                } else {
                    // A connect that got as far as a session is what makes a
                    // host "recent"; a failed one leaves the ordering alone.
                    history::record(&host);
                    app.history = history::load();
                }
            }

            match status {
                Some(s) if s.success() => app.set_status(format!("{} ✓", run.label)),
                // It ran and came back non-zero: the program printed its own
                // reason on the screen we then drew over, but the exit code on
                // its own has nothing to explain, so it stays a status line.
                Some(s) => app.set_status(format!(
                    "{} failed (exit {})",
                    run.label,
                    s.code().unwrap_or(-1)
                )),
                // Nothing ran at all. This is the only account of why the
                // screen came back unchanged, so it gets a box rather than a
                // line that clears itself after STATUS_TTL.
                None => app.alert(
                    "could not run it",
                    format!(
                        "`{}` could not be started, so {} did not happen.\n\nCheck that it is installed and on your PATH.",
                        run.argv[0], run.label
                    ),
                ),
            }
            app.refresh_all();
            app.settle_new_mount();
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
