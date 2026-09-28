//! What a submitted wizard does: the command it builds and the state it writes.

use ratatui::crossterm::event::{KeyCode, KeyEvent, KeyModifiers};

use super::picker::PickerAction;
use super::*;
use crate::sshcfg::NewHost;

impl App {
    pub(super) fn prompt_key(&mut self, key: KeyEvent) -> Option<PendingRun> {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let len = self.prompt.as_ref().map(|p| p.fields.len()).unwrap_or(0);
        if len == 0 {
            return None;
        }

        // Ctrl-o fills the field from what the app already knows: the keys in
        // ~/.ssh for IdentityFile, the config hosts for ProxyJump. Typing still
        // works for anything custom, and it is a no-op on every other field.
        if ctrl && key.code == KeyCode::Char('o') {
            let p = self.prompt.as_ref().unwrap();
            let idx = p.idx;
            let label = p.fields[idx].label.clone();
            if label.contains("IdentityFile") {
                let items: Vec<String> = keys::list()
                    .iter()
                    .map(|k| format!("~/.ssh/{}", k.name()))
                    .collect();
                if items.is_empty() {
                    self.set_failed("no keys in ~/.ssh to pick");
                } else {
                    self.picker = Some(Picker {
                        title: "Pick a key for IdentityFile".into(),
                        items,
                        idx: 0,
                        action: PickerAction::FillField { field: idx },
                    });
                }
            } else if label == "Host" {
                // The host a forward runs over is another entry in this same
                // config, so it is picked rather than spelled out again.
                let items: Vec<String> = self.hosts.iter().map(|h| h.alias.clone()).collect();
                if items.is_empty() {
                    self.set_failed("no hosts in ~/.ssh/config to forward through");
                } else {
                    self.picker = Some(Picker {
                        title: "Which host carries this forward?".into(),
                        items,
                        idx: 0,
                        action: PickerAction::FillField { field: idx },
                    });
                }
            } else if label.contains("ProxyJump") {
                // A jump host is another entry in this same config, so it is
                // picked from the list rather than spelled out again.
                let items: Vec<String> = self.hosts.iter().map(|h| h.alias.clone()).collect();
                if items.is_empty() {
                    self.set_failed("no hosts in ~/.ssh/config to jump through");
                } else {
                    self.picker = Some(Picker {
                        title: "Which host does ssh hop through? (ProxyJump)".into(),
                        items,
                        idx: 0,
                        action: PickerAction::FillField { field: idx },
                    });
                }
            }
            return None;
        }

        // A choice field types nothing, so the horizontal keys are free to switch
        // its answer: h/l, the arrows, and the Ctrl-chords all move it. You leave
        // it with Tab, Enter or the vertical keys.
        {
            let p = self.prompt.as_mut().unwrap();
            if p.fields[p.idx].is_choice() {
                let f = &mut p.fields[p.idx];
                let n = match &f.kind {
                    Kind::Choice(options) => options.len(),
                    _ => unreachable!(),
                };
                // Two options are drawn as buttons, where an arrow points at one
                // and stops at the edge; a longer list cycles.
                let buttons = n == 2;
                match key.code {
                    KeyCode::Right | KeyCode::Char('l') => {
                        f.choice = if buttons { 1 } else { (f.choice + 1) % n };
                        return None;
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        f.choice = if buttons { 0 } else { (f.choice + n - 1) % n };
                        return None;
                    }
                    _ => {}
                }
            }
        }

        // Plain h/j/k/l are typed text in a field, so fields move with the
        // Ctrl-chords, the arrows or Tab.
        let next = matches!(key.code, KeyCode::Tab | KeyCode::Down)
            || (ctrl
                && matches!(
                    key.code,
                    KeyCode::Char('j') | KeyCode::Char('l') | KeyCode::Right
                ));
        let prev = matches!(key.code, KeyCode::BackTab | KeyCode::Up)
            || (ctrl && matches!(key.code, KeyCode::Char('k') | KeyCode::Left));

        if next {
            self.split_pasted_target();
            let p = self.prompt.as_mut().unwrap();
            p.idx = p.step(1);
            return None;
        }
        if prev {
            let p = self.prompt.as_mut().unwrap();
            p.idx = p.step(-1);
            return None;
        }

        match key.code {
            KeyCode::Esc => {
                self.prompt = None;
                self.set_status("cancelled");
            }
            // Ctrl-C bails out of the wizard rather than typing a literal 'c'.
            KeyCode::Char('c') if ctrl => {
                self.prompt = None;
                self.set_status("cancelled");
            }
            KeyCode::Enter => {
                self.split_pasted_target();
                let p = self.prompt.as_mut().unwrap();
                if p.on_last_field() {
                    return self.submit_prompt();
                }
                p.idx = p.step(1);
            }
            KeyCode::Backspace => {
                let f = self.prompt.as_mut().unwrap().cur_mut();
                if !f.is_choice() {
                    f.value.pop();
                }
            }
            // Everything else with no Ctrl held is literal text - including h/j/k/l.
            // A choice field holds no text, so stray keys must not accumulate in it.
            KeyCode::Char(c) if !ctrl => {
                let f = self.prompt.as_mut().unwrap().cur_mut();
                if !f.is_choice() {
                    f.value.push(c);
                }
            }
            _ => {}
        }
        None
    }

    /// Re-do whatever a changed setting decides, so a new value is visible in
    /// the same frame rather than at the next launch.
    pub(super) fn apply_settings(&mut self) {
        self.sort_hosts();
        self.start_probes();
        let n = self.host_rows().len();
        Self::clamp(&mut self.host_state, n);
    }

    /// A destination pasted into the add-host Alias field (`deploy@10.0.0.4:2222`,
    /// the thing a console or a colleague hands you) is split across the fields
    /// it actually describes, so the fastest way to add a host is one paste.
    /// Runs when you leave the field, and only for a value that really is a
    /// destination: a plain word is somebody typing an alias.
    pub(super) fn split_pasted_target(&mut self) {
        let Some(p) = self.prompt.as_mut() else {
            return;
        };
        // Only the add wizard: rewriting the alias of a host that already exists
        // would rename it behind the user's back.
        if !matches!(p.action, Action::AddHost) || p.idx != 0 {
            return;
        }
        let Some(t) = sshcfg::parse_target(&p.fields[0].value) else {
            return;
        };
        p.fields[0].value = t.alias;
        p.fields[1].value = t.hostname.clone();
        if !t.user.is_empty() {
            p.fields[2].value = t.user;
        }
        if !t.port.is_empty() {
            p.fields[3].value = t.port;
        }
        self.set_status(format!(
            "split the pasted destination into fields ({})",
            t.hostname
        ));
    }

    /// Consume the active prompt and carry out its action. Returns a `PendingRun`
    /// for the interactive commands; mutates in place for the rest. On a
    /// validation error it puts the prompt back so the user can fix the field.
    pub(super) fn submit_prompt(&mut self) -> Option<PendingRun> {
        let mut prompt = self.prompt.take()?;
        let v: Vec<String> = prompt
            .fields
            .iter()
            .map(|f| f.value.trim().to_string())
            .collect();
        let action = prompt.action.clone();
        match action {
            Action::AddHost => {
                if v[0].is_empty() {
                    self.set_failed("add host: an alias is required");
                    self.prompt = Some(prompt);
                    return None;
                }
                if keys::is_public_key(&sshcfg::expand_tilde(&v[4])) {
                    self.set_failed(public_key_refusal(&v[4]));
                    prompt.idx = 4;
                    self.prompt = Some(prompt);
                    return None;
                }
                let nh = NewHost {
                    alias: v[0].clone(),
                    hostname: v[1].clone(),
                    user: v[2].clone(),
                    port: v[3].clone(),
                    identity: v[4].clone(),
                    proxy_jump: v[5].clone(),
                    remote_command: v[6].clone(),
                    forward_agent: prompt.fields[7].answer().to_string(),
                };
                match sshcfg::add_host(&nh) {
                    Ok(_) => {
                        self.refresh_hosts();
                        self.saved_host("added", &v[0], &v[4]);
                    }
                    Err(e) => self.set_failed(format!("add host failed: {e}")),
                }
                None
            }

            Action::EditHost { original } => {
                if v[0].is_empty() {
                    self.set_failed("edit host: an alias is required");
                    self.prompt = Some(prompt);
                    return None;
                }
                if keys::is_public_key(&sshcfg::expand_tilde(&v[4])) {
                    self.set_failed(public_key_refusal(&v[4]));
                    prompt.idx = 4;
                    self.prompt = Some(prompt);
                    return None;
                }
                let nh = NewHost {
                    alias: v[0].clone(),
                    hostname: v[1].clone(),
                    user: v[2].clone(),
                    port: v[3].clone(),
                    identity: v[4].clone(),
                    proxy_jump: v[5].clone(),
                    remote_command: v[6].clone(),
                    forward_agent: prompt.fields[7].answer().to_string(),
                };
                match sshcfg::update_host(&original, &nh) {
                    Ok(_) => {
                        self.refresh_hosts();
                        self.saved_host("updated", &v[0], &v[4]);
                    }
                    Err(e) => self.set_failed(format!("edit failed: {e}")),
                }
                None
            }

            Action::NewKey { kind } => {
                let name = if v[0].is_empty() {
                    format!("id_{kind}")
                } else {
                    v[0].clone()
                };
                let path = sshcfg::ssh_dir().join(name).to_string_lossy().into_owned();
                let mut argv = vec![
                    "ssh-keygen".into(),
                    "-t".into(),
                    kind.clone(),
                    "-f".into(),
                    path,
                ];
                // RSA has no safe default size; anything under 4096 is not worth
                // generating today. ed25519 has one fixed size, so no -b for it.
                if kind == "rsa" {
                    argv.push("-b".into());
                    argv.push("4096".into());
                }
                if !v[1].is_empty() {
                    argv.push("-C".into());
                    argv.push(v[1].clone());
                }
                Some(PendingRun {
                    argv,
                    label: format!("ssh-keygen -t {kind}"),
                    connect: None,
                })
            }

            Action::Mount {
                host,
                forced_command,
            } => {
                // Blank remote path → sshfs mounts the login home directory.
                let spec = MountSpec::from_fields(&host, forced_command, &prompt.fields);
                if let Some(problem) = spec.problem() {
                    self.set_status(problem);
                    self.prompt = Some(prompt);
                    return None;
                }
                self.start_mount(spec, true)
            }

            Action::EditSetting { key, label } => {
                // Blank means "back to how it ships", which is the same thing
                // deleting the line from the file does.
                let value = if v[0].is_empty() {
                    prompt.fields[0].default.clone()
                } else {
                    v[0].clone()
                };
                self.settings.set(&key, &value);
                let (ok, msg) = match self.settings.save() {
                    Ok(_) => (true, format!("{label} = {value}")),
                    Err(e) => (false, format!("could not save settings: {e}")),
                };
                self.apply_settings();
                self.set_result(ok, msg);
                None
            }

            Action::Tunnel { editing } => {
                if let Some(problem) = prompt::tunnel_problem(&prompt.fields) {
                    self.set_status(problem);
                    self.prompt = Some(prompt);
                    return None;
                }
                let (kind, spec, host) = prompt::tunnel_spec(&prompt.fields);
                let name = prompt::tunnel_name(&prompt.fields);

                // Rewriting a row: its process is carrying the old forward, and
                // the old line is keyed by that forward, so a changed spec would
                // otherwise leave it behind as a second row.
                if let Some(was) = &editing {
                    if let Some(pid) = was.pid {
                        let _ = tunnels::kill(pid);
                    }
                    if !(was.kind == kind && was.spec == spec && was.host == host) {
                        let _ = tunnels::forget(was.kind, &was.spec, &was.host);
                    }
                }
                // Kept before it is started, because the line is the part you
                // keep: a forward that will not come up is exactly the one you
                // want left in the list to fix.
                if let Err(e) = tunnels::remember(&tunnels::Saved {
                    kind,
                    spec: spec.clone(),
                    host: host.clone(),
                    name: name.clone(),
                }) {
                    self.set_failed(format!("could not save it: {e}"));
                }

                let row = tunnels::Entry {
                    name: name.clone(),
                    kind,
                    spec: spec.clone(),
                    host: host.clone(),
                    live: None,
                };
                match tunnels::open(kind, &spec, &host) {
                    Ok(t) => {
                        self.goto_view(View::Tunnels);
                        self.refresh_tunnels();
                        self.select_tunnel(&spec, &host);
                        self.set_status(format!("{} (pid {})", row.explain(), t.pid));
                    }
                    // ssh refused it, usually because the local port is taken, so the
                    // wizard stays open on the number to fix. Its line is already
                    // written, so from here the wizard edits that line rather than
                    // adding a second one.
                    Err(e) => {
                        prompt.action = Action::Tunnel {
                            editing: Some(prompt::Edited {
                                kind,
                                spec: spec.clone(),
                                host: host.clone(),
                                pid: None,
                            }),
                        };
                        self.refresh_tunnels();
                        self.set_failed(format!("tunnel failed: {e}"));
                        self.prompt = Some(prompt);
                    }
                }
                None
            }
        }
    }
}

/// What the host form says when IdentityFile names a public key.
fn public_key_refusal(identity: &str) -> String {
    match identity.strip_suffix(".pub") {
        Some(private) => format!(
            "IdentityFile `{identity}` is the public key: use `{private}` (ctrl-o picks one)"
        ),
        None => format!(
            "IdentityFile `{identity}` is a public key: use its private half (ctrl-o picks one)"
        ),
    }
}

impl App {
    /// The status after a host is written, yellow when its IdentityFile does
    /// not exist yet, since ssh will skip it at login without a word.
    fn saved_host(&mut self, verb: &str, alias: &str, identity: &str) {
        // ssh expands `%d`, `${HOME}` and quotes itself, so such a path cannot
        // be checked from here and gets no warning.
        let unexpanded = identity.contains(['%', '$', '"']);
        if !identity.is_empty() && !unexpanded && !sshcfg::expand_tilde(identity).exists() {
            self.set_failed(format!(
                "{verb} host '{alias}', but IdentityFile `{identity}` does not exist yet"
            ));
        } else {
            self.set_status(format!("{verb} host '{alias}' (config backed up)"));
        }
    }
}
