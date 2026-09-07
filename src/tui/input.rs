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
                        let (ok, msg) = match self.settings.save() {
                            Ok(_) => (true, format!("{} = {shown}", row.label)),
                            Err(e) => (false, format!("could not save settings: {e}")),
                        };
                        self.apply_settings();
                        self.set_result(ok, msg);
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
                let (ok, msg) = match Command::new("ssh-keygen").arg("-R").arg(&target).output() {
                    Ok(o) if o.status.success() => {
                        (true, format!("ssh-keygen -R {target}: cleared known_hosts"))
                    }
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
                // Asked once, here, because the preview and the command that
                // runs must agree and neither can read the config later.
                let forced = sshcfg::forces_command(&alias);
                self.prompt = Some(Prompt::mount(alias, forced, &self.settings));
                None
            }
            KeyCode::Char('t') => {
                let alias = self.selected_host()?.alias.clone();
                self.prompt = Some(Prompt::tunnel('L', &alias));
                None
            }
            KeyCode::Char('T') => {
                let alias = self.selected_host()?.alias.clone();
                self.prompt = Some(Prompt::tunnel('R', &alias));
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
            // `Y` is the loud one: it writes on another machine. Pick the host
            // from the config list; never make the user retype an alias the app
            // already knows.
            KeyCode::Char('Y') => {
                let path = self.selected_key()?.path.clone();
                if self.hosts.is_empty() {
                    self.set_failed("no hosts in ~/.ssh/config to copy to");
                    return None;
                }
                let name = path
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                // ssh-copy-id writes into the authorized_keys of whoever you log
                // in as, so the login the alias resolves to is shown beside it:
                // `admin@box` and `pi@box` are two different files.
                let width = self.hosts.iter().map(|h| h.alias.len()).max().unwrap_or(0);
                self.picker = Some(Picker {
                    title: format!("Install '{name}.pub' in whose authorized_keys? (ssh-copy-id)"),
                    items: self
                        .hosts
                        .iter()
                        .map(|h| format!("{:width$}  {}", h.alias, h.target()))
                        .collect(),
                    idx: 0,
                    action: PickerAction::CopyKeyTo { key: path },
                });
                None
            }
            // `y` is the clipboard one, as everywhere else: the public key text
            // itself, for the web form or the ticket asking for it.
            KeyCode::Char('y') => {
                let path = self.selected_key()?.path.clone();
                let pubpath = path.with_extension("pub");
                let (ok, msg) = match fs::read_to_string(&pubpath) {
                    Ok(text) => match crate::clip::copy(text.trim()) {
                        Ok(tool) => (
                            true,
                            format!(
                                "copied {}.pub to the clipboard ({tool})",
                                path.file_name().unwrap_or_default().to_string_lossy()
                            ),
                        ),
                        Err(e) => (false, format!("clipboard: {e}")),
                    },
                    Err(e) => (false, format!("cannot read {}: {e}", pubpath.display())),
                };
                self.set_result(ok, msg);
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

    /// The Tunnels tab. Every row is a forward you keep, so the keys are the
    /// same for all of them: Enter is the off switch a background `ssh -N` never
    /// had, and `d` stops one that is up, or deletes the line of one that is
    /// already stopped. Two presses to be rid of it, and no gate in front of the
    /// one you do all day.
    pub(super) fn tunnels_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        match key.code {
            KeyCode::Enter => {
                self.toggle_tunnel();
                None
            }
            // `c` = create, the same key in every view. The host is a field
            // here rather than the selected row, since this tab has no host.
            KeyCode::Char('c') => {
                self.prompt = Some(Prompt::tunnel('L', ""));
                None
            }
            KeyCode::Char('e') => {
                // A spec with a bind address in front is four fields, which the
                // form has three of: say so rather than rewriting it wrong.
                if self.selected_tunnel()?.ports().is_none() {
                    self.set_status("this spec has a bind address - edit the file by hand");
                    return None;
                }
                let prompt = Prompt::edit_tunnel(self.selected_tunnel()?);
                self.prompt = Some(prompt);
                None
            }
            KeyCode::Char('d') | KeyCode::Char('x') => {
                let (label, kind, spec, host, pid) = {
                    let t = self.selected_tunnel()?;
                    (t.label(), t.kind, t.spec.clone(), t.host.clone(), t.pid())
                };
                match pid {
                    // Stopping is trivially undone by the very next keypress, so
                    // it happens at once.
                    Some(pid) => {
                        let _ = tunnels::kill(pid);
                        self.refresh_tunnels();
                        self.set_status(format!("stopped '{label}'"));
                    }
                    // Losing the line is not, so that one asks first.
                    None => {
                        self.confirm = Some(Confirm::new(
                            "delete tunnel",
                            format!("Delete '{label}' from the forwards you keep?"),
                            ConfirmAction::DeleteTunnel {
                                kind,
                                spec,
                                host,
                                label,
                            },
                        ));
                    }
                }
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

    /// Enter: off when it is up, on when it is not.
    fn toggle_tunnel(&mut self) {
        let Some(t) = self.selected_tunnel() else {
            return;
        };
        let (label, kind, spec, host, pid) =
            (t.label(), t.kind, t.spec.clone(), t.host.clone(), t.pid());
        match pid {
            Some(pid) => {
                let _ = tunnels::kill(pid);
                self.refresh_tunnels();
                self.set_status(format!("stopped '{label}'"));
            }
            None => match tunnels::open(kind, &spec, &host) {
                Ok(t) => {
                    self.refresh_tunnels();
                    self.select_tunnel(&spec, &host);
                    self.set_status(format!("started '{label}' (pid {})", t.pid));
                }
                // Nothing to offer: the port is taken, or the host said no. It
                // is ssh's own words, and they have to be read.
                Err(e) => self.alert(
                    "tunnel failed",
                    format!("ssh -N -{kind} {spec} {host}\n\n{e}"),
                ),
            },
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
