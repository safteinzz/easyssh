//! Key dispatch: the app-wide keys, then whichever view owns the rest.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::confirm::ConfirmAction;
use super::picker::PickerAction;
use super::*;

impl App {
    pub(super) fn on_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        if self.show_help {
            if matches!(
                key.code,
                KeyCode::Char('?') | KeyCode::Esc | KeyCode::Char('q')
            ) {
                self.show_help = false;
            }
            return None;
        }
        // An alert owns every key until it is dismissed: it is there because
        // something failed and the message is the only copy of why.
        if self.alert.is_some() {
            self.alert_key(key);
            return None;
        }
        if self.confirm.is_some() {
            return self.confirm_key(key);
        }
        if self.picker.is_some() {
            return self.picker_key(key);
        }
        if self.prompt.is_some() {
            return self.prompt_key(key);
        }
        if self.searching {
            return self.search_key(key);
        }
        self.nav_key(key)
    }

    /// Typing a `/` filter. Everything printable goes into the query, so this
    /// has to run before the per-view letters; the list keeps updating under it
    /// and the arrows still move, which is what makes "type then Enter" work.
    pub(super) fn search_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        if ctrl && key.code == KeyCode::Char('c') {
            self.should_quit = true;
            return None;
        }
        match key.code {
            // Esc drops the filter entirely; Enter keeps it and hands the keys
            // back to the list, so you can search then act on what you found.
            KeyCode::Esc => {
                self.query.clear();
                self.searching = false;
                self.requery();
            }
            KeyCode::Enter => self.searching = false,
            KeyCode::Backspace => {
                self.query.pop();
                self.requery();
            }
            KeyCode::Down => self.move_sel(1),
            KeyCode::Up => self.move_sel(-1),
            KeyCode::Char(c) if !ctrl => {
                self.query.push(c);
                self.requery();
            }
            _ => {}
        }
        None
    }

    pub(super) fn nav_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
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
            // `/` is the filter, the same key it is in vim, less and man.
            KeyCode::Char('/') if !ctrl => {
                self.query.clear();
                self.searching = true;
                self.requery();
                return None;
            }
            // Outside a search, Esc's only job is to undo one: clearing the
            // filter is the way back to the whole list.
            KeyCode::Esc if !self.query.is_empty() => {
                self.query.clear();
                self.requery();
                self.set_status("filter cleared");
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
            View::Settings => self.settings_key(key),
        }
    }

    /// The Settings tab. A cycled setting changes in place on Enter; a typed one
    /// opens a one-field wizard. Every change is written to the file at once,
    /// so there is no unsaved state to lose or a save key to remember.
    pub(super) fn settings_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        match key.code {
            KeyCode::Enter | KeyCode::Char('e') => {
                let row = self.selected_setting()?;
                match row.choices {
                    Some(_) => {
                        self.settings.cycle(row.key, 1);
                        let shown = self.selected_setting().map(|r| r.value).unwrap_or_default();
                        let msg = match self.settings.save() {
                            Ok(_) => format!("{} = {shown}", row.label),
                            Err(e) => format!("could not save settings: {e}"),
                        };
                        self.apply_settings();
                        self.set_status(msg);
                    }
                    None => self.prompt = Some(Prompt::edit_setting(&row)),
                }
                None
            }
            // `d` is "get rid of my answer", the same shape as delete elsewhere.
            // Trivially redone, so no confirm gate.
            KeyCode::Char('d') => {
                let row = self.selected_setting()?;
                self.settings.reset(row.key);
                let _ = self.settings.save();
                self.apply_settings();
                self.set_status(format!("{} back to {}", row.label, row.default));
                None
            }
            KeyCode::Char('r') => {
                self.settings = crate::settings::load();
                self.apply_settings();
                self.set_status(format!("reloaded {}", crate::settings::path().display()));
                None
            }
            _ => None,
        }
    }

    pub(super) fn hosts_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        match key.code {
            KeyCode::Enter => {
                let alias = self.selected_host()?.alias.clone();
                // Whatever Settings says runs a login - plain ssh, or a wrapper
                // like `kitten ssh` that wants its own arguments in front.
                let mut argv = self.settings.ssh_argv();
                argv.push(alias.clone());
                Some(PendingRun {
                    label: format!("{} {alias}", argv[0]),
                    argv,
                    connect: Some(alias),
                })
            }
            // `c` = create, the same key in every view (the tmux convention).
            KeyCode::Char('c') => {
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
                self.confirm = Some(Confirm::new(
                    "delete host",
                    format!("Delete host '{alias}' from ~/.ssh/config?"),
                    ConfirmAction::DeleteHost(alias),
                ));
                None
            }
            // Clear a host's stale key from known_hosts (the "REMOTE HOST
            // IDENTIFICATION HAS CHANGED" fix). Non-interactive, so run it inline.
            KeyCode::Char('R') => {
                let target = {
                    let h = self.selected_host()?;
                    h.hostname.clone().unwrap_or_else(|| h.alias.clone())
                };
                let msg = match Command::new("ssh-keygen").arg("-R").arg(&target).output() {
                    Ok(o) if o.status.success() => {
                        format!("ssh-keygen -R {target}: cleared known_hosts")
                    }
                    Ok(o) => format!(
                        "ssh-keygen -R failed: {}",
                        String::from_utf8_lossy(&o.stderr).trim()
                    ),
                    Err(e) => format!("could not run ssh-keygen: {e}"),
                };
                self.set_status(msg);
                None
            }
            // Yank the connect command, since the reason to leave the toolbox
            // for a host is almost always to paste `ssh <alias>` somewhere else.
            KeyCode::Char('y') => {
                let alias = self.selected_host()?.alias.clone();
                let cmd = format!("ssh {alias}");
                self.set_status(match crate::clip::copy(&cmd) {
                    Ok(tool) => format!("copied '{cmd}' to the clipboard ({tool})"),
                    Err(e) => format!("clipboard: {e}"),
                });
                None
            }
            KeyCode::Char('r') => {
                self.refresh_hosts();
                self.start_probes();
                self.set_status("reloaded ~/.ssh/config, re-checking ports");
                None
            }
            KeyCode::Char('m') => {
                let alias = self.selected_host()?.alias.clone();
                self.prompt = Some(Prompt::mount(alias, &self.settings));
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

    pub(super) fn keys_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        match key.code {
            // Ask the type first: ed25519 for anything modern, rsa only for boxes
            // too old to speak it. Picked, not typed.
            KeyCode::Char('c') => {
                self.picker = Some(Picker {
                    title: "Which key type?".into(),
                    items: vec![
                        "ed25519  (recommended: smaller, faster, modern)".into(),
                        "rsa 4096 (only for servers too old for ed25519)".into(),
                    ],
                    idx: 0,
                    action: PickerAction::NewKeyType,
                });
                None
            }
            // Pick the host from the config list; never make the user retype an
            // alias the app already knows.
            KeyCode::Char('y') => {
                let path = self.selected_key()?.path.clone();
                if self.hosts.is_empty() {
                    self.set_status("no hosts in ~/.ssh/config to copy to");
                    return None;
                }
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                self.picker = Some(Picker {
                    title: format!(
                        "Copy '{name}.pub' into which host's authorized_keys? (ssh-copy-id)"
                    ),
                    items: self.hosts.iter().map(|h| h.alias.clone()).collect(),
                    idx: 0,
                    action: PickerAction::CopyKeyTo { key: path },
                });
                None
            }
            // `Y` is the same yank one level louder: the public key text itself,
            // for the web form or the ticket that is asking for it.
            KeyCode::Char('Y') => {
                let path = self.selected_key()?.path.clone();
                let pubpath = path.with_extension("pub");
                self.set_status(match fs::read_to_string(&pubpath) {
                    Ok(text) => match crate::clip::copy(text.trim()) {
                        Ok(tool) => format!(
                            "copied {}.pub to the clipboard ({tool})",
                            path.file_name().unwrap_or_default().to_string_lossy()
                        ),
                        Err(e) => format!("clipboard: {e}"),
                    },
                    Err(e) => format!("cannot read {}: {e}", pubpath.display()),
                });
                None
            }
            KeyCode::Char('r') => {
                self.refresh_keys();
                self.set_status("reloaded ~/.ssh keys and the agent");
                None
            }
            _ => None,
        }
    }

    pub(super) fn tunnels_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        match key.code {
            KeyCode::Char('d') | KeyCode::Char('x') => {
                let pid = self.selected_tunnel()?.pid;
                let _ = tunnels::kill(pid);
                self.refresh_tunnels();
                self.set_status(format!("killed tunnel {pid}"));
                None
            }
            KeyCode::Char('r') => {
                self.refresh_tunnels();
                self.set_status("refreshed tunnels");
                None
            }
            _ => None,
        }
    }

    pub(super) fn mounts_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        match key.code {
            KeyCode::Char('d') | KeyCode::Char('x') => {
                let local = self.selected_mount()?.local.clone();
                match mounts::unmount(&local) {
                    Ok(_) => {
                        // Remove the now-empty mountpoint so it does not linger.
                        // `remove_dir` only deletes an empty dir, so a mount over
                        // a dir that had real content is left untouched.
                        let _ = fs::remove_dir(&local);
                        self.refresh_mounts();
                        self.set_status(format!("fusermount -u {local}: unmounted"));
                    }
                    Err(e) => {
                        self.confirm = Some(Confirm::new(
                            "unmount failed",
                            format!(
                                "{local}: {e}. Something is still using it (a shell cd'd in, or an open file). Force a lazy unmount (fusermount -u -z)? It detaches now and the kernel frees it once nothing uses it."
                            ),
                            ConfirmAction::LazyUnmount { local },
                        ));
                    }
                }
                None
            }
            KeyCode::Char('r') => {
                self.refresh_mounts();
                self.set_status("refreshed mounts");
                None
            }
            _ => None,
        }
    }
}
