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
                    self.set_status("no keys in ~/.ssh to pick");
                } else {
                    self.picker = Some(Picker {
                        title: "Pick a key for IdentityFile".into(),
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
                    self.set_status("no hosts in ~/.ssh/config to jump through");
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
        // its answer: h/l, the arrows, and the Ctrl-chords all cycle it. You leave
        // it with Tab, Enter or the vertical keys.
        {
            let p = self.prompt.as_mut().unwrap();
            if p.fields[p.idx].is_choice() {
                let f = &mut p.fields[p.idx];
                let n = match &f.kind {
                    Kind::Choice(options) => options.len(),
                    _ => unreachable!(),
                };
                match key.code {
                    KeyCode::Right | KeyCode::Char('l') => {
                        f.choice = (f.choice + 1) % n;
                        return None;
                    }
                    KeyCode::Left | KeyCode::Char('h') => {
                        f.choice = (f.choice + n - 1) % n;
                        return None;
                    }
                    _ => {}
                }
            }
        }

        // Field navigation. Plain h/j/k/l are *typed* in a text field, so a vim
        // user moves between fields with the Ctrl-chords or the arrows/Tab -
        // exactly the "submenu" rule. Ctrl-Down/Up also carry ctrl, but their arm
        // matches on the code so they land as next/prev too.
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
        let prompt = self.prompt.take()?;
        let v: Vec<String> = prompt
            .fields
            .iter()
            .map(|f| f.value.trim().to_string())
            .collect();
        let action = prompt.action.clone();

        match action {
            Action::AddHost => {
                if v[0].is_empty() {
                    self.set_status("add host: an alias is required");
                    self.prompt = Some(prompt);
                    return None;
                }
                let port = if v[3].is_empty() {
                    "22".into()
                } else {
                    v[3].clone()
                };
                let nh = NewHost {
                    alias: v[0].clone(),
                    hostname: v[1].clone(),
                    user: v[2].clone(),
                    port,
                    identity: v[4].clone(),
                    proxy_jump: v[5].clone(),
                    remote_command: v[6].clone(),
                };
                match sshcfg::add_host(&nh) {
                    Ok(_) => {
                        self.refresh_hosts();
                        self.set_status(format!("added host '{}' (config backed up)", v[0]));
                    }
                    Err(e) => self.set_status(format!("add host failed: {e}")),
                }
                None
            }

            Action::EditHost { original } => {
                if v[0].is_empty() {
                    self.set_status("edit host: an alias is required");
                    self.prompt = Some(prompt);
                    return None;
                }
                let port = if v[3].is_empty() {
                    "22".into()
                } else {
                    v[3].clone()
                };
                let nh = NewHost {
                    alias: v[0].clone(),
                    hostname: v[1].clone(),
                    user: v[2].clone(),
                    port,
                    identity: v[4].clone(),
                    proxy_jump: v[5].clone(),
                    remote_command: v[6].clone(),
                };
                match sshcfg::update_host(&original, &nh) {
                    Ok(_) => {
                        self.refresh_hosts();
                        self.set_status(format!("updated host '{}' (config backed up)", v[0]));
                    }
                    Err(e) => self.set_status(format!("edit failed: {e}")),
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

            Action::Mount { host } => {
                // Fail early with an install hint rather than a cryptic spawn error.
                if !mounts::sshfs_installed() {
                    self.set_status("sshfs is not installed (apt install sshfs · pacman -S sshfs · dnf install fuse-sshfs)");
                    return None;
                }
                // Blank remote path → sshfs mounts the login home directory.
                let spec = MountSpec::from_fields(&host, &prompt.fields);
                if let Some(problem) = spec.problem() {
                    self.set_status(problem);
                    self.prompt = Some(prompt);
                    return None;
                }
                let local = spec.local.clone();
                if let Err(e) = fs::create_dir_all(&local) {
                    self.set_status(format!("mount: cannot create {local}: {e}"));
                    return None;
                }
                // Jump to the Mounts tab so the result (success or empty) is
                // visible, and remember which one to land on once it exists.
                self.goto_view(View::Mounts);
                self.new_mount = Some(local.clone());
                Some(PendingRun {
                    argv: spec.argv(),
                    label: format!("sshfs {host}: → {local}"),
                    connect: None,
                })
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
                let msg = match self.settings.save() {
                    Ok(_) => format!("{label} = {value}"),
                    Err(e) => format!("could not save settings: {e}"),
                };
                self.apply_settings();
                self.set_status(msg);
                None
            }

            Action::Forward { host } => {
                if v[0].is_empty() {
                    self.set_status("forward: a remote port is required");
                    self.prompt = Some(prompt);
                    return None;
                }
                let remote = v[0].clone();
                let local = if v[1].is_empty() {
                    remote.clone()
                } else {
                    v[1].clone()
                };
                let spec = format!("{local}:localhost:{remote}");
                match tunnels::open('L', &spec, &host) {
                    Ok(t) => {
                        self.goto_view(View::Tunnels);
                        self.refresh_tunnels();
                        self.select_tunnel(t.pid);
                        self.set_status(format!(
                            "localhost:{local} → {host}:{remote}  (pid {})",
                            t.pid
                        ));
                    }
                    // ssh refused it - usually the local port is already taken.
                    // Leave the wizard open on the port that failed, so the fix
                    // is editing one number rather than starting again.
                    Err(e) => {
                        self.set_status(format!("forward failed: {e}"));
                        self.prompt = Some(prompt);
                    }
                }
                None
            }

            Action::Reverse { host } => {
                if v[0].is_empty() {
                    self.set_status("expose: a local port is required");
                    self.prompt = Some(prompt);
                    return None;
                }
                let local = v[0].clone();
                let remote = if v[1].is_empty() {
                    local.clone()
                } else {
                    v[1].clone()
                };
                let spec = format!("{remote}:localhost:{local}");
                match tunnels::open('R', &spec, &host) {
                    Ok(t) => {
                        self.goto_view(View::Tunnels);
                        self.refresh_tunnels();
                        self.select_tunnel(t.pid);
                        self.set_status(format!(
                            "{host}:{remote} → localhost:{local}  (pid {})",
                            t.pid
                        ));
                    }
                    Err(e) => {
                        self.set_status(format!("expose failed: {e}"));
                        self.prompt = Some(prompt);
                    }
                }
                None
            }
        }
    }
}
