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
use std::fs;
use std::io::{self, Stdout};
use std::path::PathBuf;
use std::process::{Command, ExitStatus};
use std::time::{Duration, Instant};

type Term = Terminal<CrosstermBackend<Stdout>>;

/// The tabs. Order here is the left-to-right / Tab-cycle order.
mod confirm;
mod connect;
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
use mount_spec::{MountSpec, shell_join};
use picker::Picker;
use prompt::{Action, Field, Kind, Prompt, render_prompt};
use render::ui;
use widgets::{centered, empty, titled, wrapped_line_count};

#[derive(Clone, Copy, PartialEq)]
pub(super) enum View {
    Hosts,
    Keys,
    Tunnels,
    Mounts,
}

const VIEWS: [View; 4] = [View::Hosts, View::Keys, View::Tunnels, View::Mounts];

// The bottom bar is a terse reminder of this view's actions only; `?` opens the
// full cheat-sheet (navigation keys and the real command behind each action), so
// the bar stays short instead of restating everything and overflowing.
const HOSTS_HINTS: &str =
    "↵ connect · c new · e edit · d del · m mount · t/T tunnel · R fix-key · ? help";
const KEYS_HINTS: &str = "c new · y copy to host · ? help";
const TUNNELS_HINTS: &str = "d kill · r refresh · ? help";
const MOUNTS_HINTS: &str = "d unmount · r refresh · ? help";

/// How long a status message stays on screen before the hints return.
const STATUS_TTL: Duration = Duration::from_millis(1500);

/// An external command the event loop must run *suspended* (outside the TUI) so
/// it can own the terminal - connect, keygen, copy-id, mount.
pub(super) struct PendingRun {
    pub(super) argv: Vec<String>,
    pub(super) label: String,
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
    pub(super) status: String,
    /// When `status` was set; it stops showing after `STATUS_TTL` so an old
    /// message never sits there looking like it is still current.
    pub(super) status_at: Option<Instant>,
    pub(super) show_help: bool,
    pub(super) should_quit: bool,
}

impl App {
    /// An app with no data loaded - the base for both `new()` and the tests
    /// (which set the lists directly, so they never touch the real ~/.ssh).
    pub(super) fn empty() -> Self {
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
            status: String::new(),
            status_at: None,
            show_help: false,
            should_quit: false,
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
        app.refresh_all();
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
        Self::clamp(&mut self.host_state, self.hosts.len());
    }

    pub(super) fn refresh_keys(&mut self) {
        self.keys = keys::list();
        Self::clamp(&mut self.key_state, self.keys.len());
    }

    pub(super) fn refresh_tunnels(&mut self) {
        self.tunnels = tunnels::list();
        Self::clamp(&mut self.tunnel_state, self.tunnels.len());
    }

    pub(super) fn refresh_mounts(&mut self) {
        self.mounts = mounts::list();
        Self::clamp(&mut self.mount_state, self.mounts.len());
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

    pub(super) fn selected_host(&self) -> Option<&Host> {
        self.host_state.selected().and_then(|i| self.hosts.get(i))
    }

    pub(super) fn selected_key(&self) -> Option<&keys::Key> {
        self.key_state.selected().and_then(|i| self.keys.get(i))
    }

    pub(super) fn selected_tunnel(&self) -> Option<&tunnels::Tunnel> {
        self.tunnel_state
            .selected()
            .and_then(|i| self.tunnels.get(i))
    }

    pub(super) fn selected_mount(&self) -> Option<&mounts::Mount> {
        self.mount_state.selected().and_then(|i| self.mounts.get(i))
    }

    // --- input ------------------------------------------------------------

    pub(super) fn cycle_view(&mut self, delta: i32) {
        let i = VIEWS.iter().position(|v| *v == self.view).unwrap_or(0) as i32;
        let n = (i + delta).rem_euclid(VIEWS.len() as i32) as usize;
        self.view = VIEWS[n];
    }

    pub(super) fn move_sel(&mut self, delta: i32) {
        match self.view {
            View::Hosts => Self::move_state(&mut self.host_state, self.hosts.len(), delta),
            View::Keys => Self::move_state(&mut self.key_state, self.keys.len(), delta),
            View::Tunnels => Self::move_state(&mut self.tunnel_state, self.tunnels.len(), delta),
            View::Mounts => Self::move_state(&mut self.mount_state, self.mounts.len(), delta),
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

        // While a status message is showing we wake up periodically so it can
        // expire on its own; otherwise block until the user actually presses a
        // key, so an idle TUI costs nothing.
        let timeout = if app.live_status().is_some() {
            Duration::from_millis(250)
        } else {
            Duration::from_secs(3600)
        };
        if !event::poll(timeout)? {
            continue; // nothing pressed: redraw so the stale status drops off
        }

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
            let is_connect = run.argv.first().map(|a| a == "ssh").unwrap_or(false);
            let failed_255 = matches!(status, Some(ref s) if s.code() == Some(255));
            if is_connect && failed_255 && run.argv.len() >= 2 {
                let host = run.argv[1].clone();
                if let Some(target) = changed_host_key(&host) {
                    app.offer_known_hosts_fix(&host, target);
                }
            }

            let msg = match status {
                Some(s) if s.success() => format!("{} ✓", run.label),
                Some(s) => format!("{} failed (exit {})", run.label, s.code().unwrap_or(-1)),
                None => format!("{}: could not run '{}'", run.label, run.argv[0]),
            };
            app.set_status(msg);
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
