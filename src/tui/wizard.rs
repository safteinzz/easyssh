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

        // On the IdentityFile field, Ctrl-o opens a picker of ~/.ssh keys so you
        // choose one instead of typing the path; typing still works for anything
        // custom. Only meaningful on that field, so it is a no-op elsewhere.
        if ctrl && key.code == KeyCode::Char('o') {
            let p = self.prompt.as_ref().unwrap();
            if p.fields[p.idx].label.contains("IdentityFile") {
                let idx = p.idx;
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
                // Jump to the Mounts tab so the result (success or empty) is visible.
                self.view = View::Mounts;
                Some(PendingRun {
                    argv: spec.argv(),
                    label: format!("sshfs {host}: → {local}"),
                })
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
                        self.view = View::Tunnels;
                        self.refresh_tunnels();
                        self.set_status(format!(
                            "localhost:{local} → {host}:{remote}  (pid {})",
                            t.pid
                        ));
                    }
                    Err(e) => self.set_status(format!("forward failed: {e}")),
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
                        self.view = View::Tunnels;
                        self.refresh_tunnels();
                        self.set_status(format!(
                            "{host}:{remote} → localhost:{local}  (pid {})",
                            t.pid
                        ));
                    }
                    Err(e) => self.set_status(format!("expose failed: {e}")),
                }
                None
            }
        }
    }
}
